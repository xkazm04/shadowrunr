# SPDX-License-Identifier: Apache-2.0
"""From a generated candidate to ``lich.glb`` and ``lich.json``.

1. **Orient** (:mod:`.fit`): face +Z, up +Y, one unit tall; stray voxels dropped.
2. **Camera**: a fixed-FOV camera on +Z moved until the model's silhouette covers the portrait's
   skull matte (the rig's head and jaw layers). It is ``lich.json``'s ``camera``, and it is how
   every portrait pixel below finds its place on the model.
3. **Jaw cut**: a face is jaw when its centre, seen through that camera, falls inside the 2D
   rig's jaw polygon (``m8.spec.json``) and it lies in front of the hinge plane. Both pieces are
   capped (a fan over each hole), so an open mouth shows dark bone, not a hole. The hinge is
   centred in X, at the polygon's top corners in Y, at the hinge plane in Z.
4. **Anchors**: each eye socket's centre is the socket polygon's centre pushed in from the rim's
   depth toward the socket floor; the monocle is the ring's centre at the rim; the neck is under
   the skull at the 2D rig's head pivot.
5. **Decimate** each piece to its share of the budget, unwrap one shared atlas (xatlas), and
   **bake** it: what the portrait shows where the surface faces the camera (the clean rig layers:
   no eye fire, no ring), the model's own colour elsewhere (matched to the portrait's tones),
   and near-black on the caps.
6. **Export** and **check** against :mod:`.contract`, then render the review pictures.
"""

from __future__ import annotations

import argparse
import io
import json
import math
import time
from pathlib import Path

import numpy as np

from . import contract, fit, geom, prep, review, tex
from .glb import Mesh, Node, write_glb
from .render import Renderer, look_at, perspective, project

CAP_RGB = np.array([0.025, 0.03, 0.03])
HINGE_Z = -0.02  # the hinge plane: a little behind the head's middle (condyles sit before the ear)
RAMUS_RISE = 52  # portrait px from the bite line's outer corners up to the condyles
RAMUS_WIDTH = 0.22  # of the jaw polygon's width: how far in from each side the rami reach
EYE_INSET = 0.35
BACK_Z = 0.12  # model Z behind which unseen colour is taken as bone (the face is at +0.3..0.5)  # of the way from the socket rim's depth to its floor


def _log(msg: str) -> None:
    print(msg, flush=True)


def jaw_regions(poly: np.ndarray) -> tuple[list[np.ndarray], float]:
    """The 2D rig's jaw polygon stops at the bite line's corners; the mandible's rami climb on
    from there to the condyles at the outer edges. Returns the polygons whose union is the jaw
    seen from the front, and the condyles' height (portrait px), where the hinge goes."""
    x0, x1 = poly[:, 0].min(), poly[:, 0].max()
    top = float(np.median(poly[np.isin(poly[:, 0], [x0, x1]), 1]))
    cond = top - RAMUS_RISE
    w = RAMUS_WIDTH * (x1 - x0)
    quads = [
        np.array([[x0 - 30, cond], [x0 + w, cond], [x0 + w, top + 4], [x0 - 30, top + 4]]),
        np.array([[x1 - w, cond], [x1 + 30, cond], [x1 + 30, top + 4], [x1 - w, top + 4]]),
    ]
    return [poly] + quads, cond


def jaw_mask(v, f, view, proj, size, regions, hinge_z) -> np.ndarray:
    cent = v[f].mean(axis=1)
    px = project(cent, view, proj, size)[:, :2]
    inside = np.zeros(len(f), bool)
    for poly in regions:
        inside |= geom.point_in_poly(px, poly)
    return inside & (cent[:, 2] > hinge_z)


def surface_at(posmap, cover, pts_px) -> np.ndarray:
    """Model-space positions under pixel points (nearest pixel), NaN off the model."""
    out = np.full((len(pts_px), 3), np.nan)
    for i, (x, y) in enumerate(np.asarray(pts_px)):
        xi, yi = int(round(x)), int(round(y))
        if 0 <= yi < cover.shape[0] and 0 <= xi < cover.shape[1] and cover[yi, xi]:
            out[i] = posmap[yi, xi]
    return out


def ray_at_z(px, view, proj, size, z) -> np.ndarray:
    from .render import unproject_ray

    o, d = unproject_ray(px[0], px[1], view, proj, size)
    t = (z - o[2]) / d[2]
    return o + t * d


def poly_ring(poly: np.ndarray, grow: float, n: int = 48) -> np.ndarray:
    """Points just outside a polygon's outline (its centre pushed out by ``grow``)."""
    poly = np.asarray(poly, float)
    c = poly.mean(axis=0)
    pts = []
    for i in range(len(poly)):
        a, b = poly[i], poly[(i + 1) % len(poly)]
        for t in np.linspace(0, 1, max(2, n // len(poly)), endpoint=False):
            p = a + t * (b - a)
            pts.append(c + (p - c) * grow)
    return np.array(pts)


def anchors(spec: dict, posmap, cover, view, proj, size, skull_v) -> tuple[dict, float]:
    by = {e["name"]: e for e in spec["elements"]}
    out = {}
    radii = []
    for name in ("eye_l", "eye_r"):
        poly = np.array(by[name]["region"]["poly"], float)
        c = poly.mean(axis=0)
        rim = surface_at(posmap, cover, poly_ring(poly, 1.12))
        rim_z = float(np.nanmedian(rim[:, 2]))
        floor = surface_at(posmap, cover, [c])[0]
        floor_z = float(floor[2]) if np.isfinite(floor[2]) else rim_z - 0.08
        z = rim_z + EYE_INSET * (floor_z - rim_z)
        out[name] = ray_at_z(c, view, proj, size, z)
        # The socket's equivalent radius, in scene units at that depth.
        x, y = poly[:, 0], poly[:, 1]
        area = 0.5 * abs(np.dot(x, np.roll(y, -1)) - np.dot(y, np.roll(x, -1)))
        r_px = math.sqrt(area / math.pi)
        edge = ray_at_z(c + np.array([r_px, 0]), view, proj, size, z)
        radii.append(float(np.linalg.norm(edge - out[name])))
        out[f"{name}_rim_z"] = rim_z
    ring = by["monocle"]["region"]["ring"]
    out["monocle"] = ray_at_z(np.array(ring[:2], float), view, proj, size, out["eye_r_rim_z"] + 0.01)
    head = spec["head"]
    neck = ray_at_z(np.array([head["center"][0], head["pivot_y"]], float), view, proj, size, 0.0)
    neck[0] = 0.0
    neck[2] = float(skull_v[:, 2].mean())
    out["neck"] = neck
    for k in ("eye_l_rim_z", "eye_r_rim_z"):
        out.pop(k)
    return out, float(np.mean(radii))


def decimate(v, f, target: int):
    import fast_simplification

    if len(f) <= target:
        return v, f
    pv, pf = fast_simplification.simplify(v.astype(np.float32), f.astype(np.int32), 1 - target / len(f))
    return np.asarray(pv, np.float64), np.asarray(pf, np.int64)


def dilate(img: np.ndarray, filled: np.ndarray, px: int = 8) -> np.ndarray:
    """Spread each island's edge colour outward so mip levels do not bleed black into seams."""
    from scipy import ndimage

    dist, (iy, ix) = ndimage.distance_transform_edt(~filled, return_indices=True)
    out = img.copy()
    grow = (~filled) & (dist <= px)
    out[grow] = img[iy[grow], ix[grow]]
    return out


# Boxes (x0, y0, x1, y1 in ``input_clean`` pixels) of lit bone with cracks and grain and no socket,
# edge or stroke of paint in them: the cranium dome and forehead. The unseen bone is cut
# from these.
SWATCH_BOXES = ((540, 130, 650, 230), (345, 175, 470, 275))
SWATCH_PX_PER_UNIT = 858.0  # input pixels per scene unit: the front's own scale (skull+jaw 1 unit)


def bone_swatches(work: Path) -> list[np.ndarray]:
    """The portrait's bone, cut into swatches (float 0..1), where the matte is fully opaque."""
    from PIL import Image

    img = np.asarray(Image.open(work / "input_clean2.png").convert("RGBA")).astype(np.float64) / 255
    out = []
    for x0, y0, x1, y1 in SWATCH_BOXES:
        crop = img[y0:y1, x0:x1]
        if crop[..., 3].min() > 0.98:
            out.append(crop[..., :3])
    if not out:
        raise SystemExit("no bone swatch is fully opaque: SWATCH_BOXES does not fit this prep")
    return out


def bake(parts, tex_size, hi_v, hi_c, hi_f, portrait_rgb, portrait_a, posmap, cover, view, proj, size, r,
         swatches=None, ao_strength=0.5, normals=True, log=_log):
    """One atlas for both pieces. ``parts``: dicts with v, f, uv, cap (per face). Returns the
    base-colour image, the tangent-space normal image (None when ``normals`` is off) and the share
    of texels the portrait painted."""
    from scipy.spatial import cKDTree

    tree = cKDTree(hi_v)
    img = np.zeros((tex_size, tex_size, 3))
    nimg = np.zeros((tex_size, tex_size, 3), np.uint8)
    nimg[...] = (128, 128, 255)
    filled = np.zeros((tex_size, tex_size), bool)
    cam_eye = np.linalg.inv(view)[:3, 3]
    hi_n = geom.vertex_normals(hi_v, hi_f).astype(np.float64) if normals or ao_strength > 0 else None
    if ao_strength > 0:
        cf = tex.cavity(hi_v, hi_n, k=10)
        cw = tex.cavity(hi_v, hi_n, k=10, subsample=max(len(hi_v) // 12, 20000))
        ao_hi = tex.cavity_factor(cf, cw, ao_strength)
        log(f"bake: cavity term over {len(hi_v)} vertices (strength {ao_strength})")
    detail = [tex.detail_map(sw) for sw in swatches] if swatches else []
    samples = []
    for p in parts:
        pos, face = r.bake_positions(p["v"], p["f"], p["uv"], tex_size)
        m = face >= 0
        pts = pos[m].astype(np.float64)
        fidx = face[m]
        fn = np.cross(p["v"][p["f"][:, 1]] - p["v"][p["f"][:, 0]], p["v"][p["f"][:, 2]] - p["v"][p["f"][:, 0]])
        fn /= np.maximum(np.linalg.norm(fn, axis=1, keepdims=True), 1e-12)
        n = fn[fidx]
        _, nn = tree.query(pts, k=4)
        model_c = hi_c[nn].mean(axis=1)
        # The portrait where the camera saw this point: facing it and not hidden.
        q = project(pts, view, proj, size)
        xi = np.clip(q[:, 0].astype(int), 0, size - 1)
        yi = np.clip(q[:, 1].astype(int), 0, size - 1)
        seen = cover[yi, xi] & (np.linalg.norm(posmap[yi, xi] - pts, axis=1) < 0.012)
        to_cam = cam_eye - pts
        to_cam /= np.linalg.norm(to_cam, axis=1, keepdims=True)
        facing = np.einsum("ij,ij->i", n, to_cam)
        w = np.clip((facing - 0.15) / 0.35, 0, 1) * seen * portrait_a[yi, xi]
        port_c = portrait_rgb[yi, xi]
        ao = ao_hi[nn].mean(axis=1) if ao_strength > 0 else np.ones(len(pts))
        if normals:
            vn = geom.vertex_normals(p["v"], p["f"]).astype(np.float64)
            tri = p["f"][fidx]
            bary = tex.barycentric(pts, p["v"][tri[:, 0]], p["v"][tri[:, 1]], p["v"][tri[:, 2]])
            n_surf = (vn[tri] * bary[..., None]).sum(axis=1)
            tt, bb = tex.face_tangents(p["v"], p["f"], p["uv"])
            n_hi = hi_n[nn[:, :3]].mean(axis=1)
            n_hi /= np.maximum(np.linalg.norm(n_hi, axis=1, keepdims=True), 1e-12)
            nt = tex.to_tangent_space(n_hi, n_surf, tt[fidx], bb[fidx])
            nt[p["cap"][fidx]] = (0.0, 0.0, 1.0)
            nimg[m] = tex.encode_normals(nt)
        samples.append((model_c, port_c, w, m, p["cap"][fidx], pts[:, 2], pts, n, ao))
    # Match the model's colours to the portrait's on the points both see well.
    mc = np.concatenate([s[0][s[2] > 0.9] for s in samples])
    pc = np.concatenate([s[1][s[2] > 0.9] for s in samples])
    gain = pc.std(axis=0) / np.maximum(mc.std(axis=0), 1e-3)
    off = pc.mean(axis=0) - mc.mean(axis=0) * gain
    # The models paint what they cannot see (the back of the head) near black, from the black
    # ground behind the input. Behind the face that becomes bone: the portrait's mean bone colour,
    # keeping the model's own light and dark variation as texture.
    bone = pc.mean(axis=0)
    all_l = np.concatenate([s[0].mean(axis=1)[s[5] < BACK_Z] for s in samples])
    l_mid, l_sd = float(np.median(all_l)), float(all_l.std()) + 1e-3
    for model_c, port_c, w, m, cap, z, pts, n, ao in samples:
        lum = model_c.mean(axis=1)
        model_c = np.clip(model_c * gain + off, 0, 1)
        back = np.clip((BACK_Z - z) / 0.25, 0, 1) * (1 - w)
        if detail:
            # Bone cut from the front, wherever the camera did not see and the model has bone.
            synth = np.zeros((len(pts), 3))
            for i, d in enumerate(detail):  # one swatch per equal share of the points (by index)
                sel = np.arange(len(pts)) % len(detail) == i
                synth[sel] = tex.synth_detail(d, pts[sel], n[sel], SWATCH_PX_PER_UNIT, seed=7 + i)
            synth = np.clip(bone[None, :] * synth, 0, 1)
            boned = np.clip((model_c.mean(axis=1) - 0.22) / 0.2, 0, 1)  # model says bone here (not a dark cavity)
            unseen = np.maximum(boned, back)
            model_c = unseen[:, None] * synth + (1 - unseen[:, None]) * model_c
        else:
            back_c = bone[None, :] * np.clip(0.9 + 0.15 * (lum - l_mid) / l_sd, 0.6, 1.1)[:, None]
            model_c = back[:, None] * back_c + (1 - back[:, None]) * model_c
        c = w[:, None] * port_c + (1 - w[:, None]) * model_c
        c = c * (1 + (ao[:, None] - 1) * (1 - 0.5 * w[:, None]))  # cavity: full on the unseen, half on the painted
        c[cap] = CAP_RGB
        img[m] = np.clip(c, 0, 1)
        filled |= m
    img = dilate(img, filled)
    nimg = np.where(filled[..., None], nimg, np.array((128, 128, 255), np.uint8))
    nimg = dilate(nimg.astype(np.float64), filled).round().astype(np.uint8) if normals else None
    front = float(np.mean(np.concatenate([s[2] for s in samples]) > 0.5))
    return (np.clip(img, 0, 1) * 255).round().astype(np.uint8), nimg, front


def unwrap(parts, resolution: int = 2048):
    import xatlas

    atlas = xatlas.Atlas()
    for p in parts:
        atlas.add_mesh(p["v"].astype(np.float32), p["f"].astype(np.uint32))
    co = xatlas.ChartOptions()
    po = xatlas.PackOptions()
    po.padding = 4
    po.resolution = resolution
    po.bilinear = True
    atlas.generate(co, po)
    out = []
    for i, p in enumerate(parts):
        vmap, idx, uv = atlas[i]
        cap_v = p["cap"]
        out.append({"v": p["v"][vmap], "f": idx.astype(np.int64), "uv": uv.astype(np.float32), "cap": cap_v})
    return out


def image_bytes(img: np.ndarray, fmt: str = "png", quality: int = 92) -> tuple[bytes, str]:
    """An (h, w, 3) uint8 image as PNG or JPEG bytes and its glTF mime type."""
    from PIL import Image

    buf = io.BytesIO()
    if fmt == "jpeg":
        Image.fromarray(img).save(buf, format="JPEG", quality=quality, subsampling=0, optimize=True)
        return buf.getvalue(), "image/jpeg"
    Image.fromarray(img).save(buf, format="PNG", optimize=True)
    return buf.getvalue(), "image/png"


def jaw_rotate(v: np.ndarray, rad: float) -> np.ndarray:
    """Open the jaw: +rad about the hinge's +X drops the chin."""
    return v @ fit.rot("x", math.degrees(rad)).T


def build(a: argparse.Namespace) -> int:
    from PIL import Image

    t0 = time.time()
    cand = a.work / "cand" / a.cand
    spec = json.loads(a.spec.read_text(encoding="utf-8"))
    v, f, c, meta = review.oriented(cand, _log)
    v, f, used = geom.remove_floaters(v, f)
    c = c[used]
    _log(f"build: {a.cand}: {len(f)} tris after dropping floaters")
    v = fit.normalize(v)

    portrait = np.asarray(Image.open(a.portrait).convert("RGB")).astype(np.float64) / 255
    size = portrait.shape[0]
    raw_layers = prep.skull_layers(a.rig, (portrait.shape[1], portrait.shape[0]))
    # The rig's head layer keeps a smear of the monocle ring's fire: the bake must not read it.
    layers = prep.desmear(raw_layers)[0].astype(np.float64) / 255
    matte = layers[..., 3]
    r = Renderer(size)
    pos, target, fov, iou = fit.fit_camera(r, v, f, matte, log=_log)
    view, proj = look_at(pos, target), perspective(fov)
    posmap, cover = r.positions(v, f, view, proj)

    by = {e["name"]: e for e in spec["elements"]}
    poly = np.array(by["jaw"]["region"]["poly"], float)
    regions, cond_y = jaw_regions(poly)
    jaw = jaw_mask(v, f, view, proj, size, regions, HINGE_Z)
    (sv, sf, scap), (jv, jf, jcap) = geom.split(v, f, jaw)
    _log(f"jaw: {int(jaw.sum())} of {len(f)} faces; skull closed {geom.is_closed(sf)}, jaw closed {geom.is_closed(jf)}")

    # The hinge: centred in X, at the condyles' height in Y, at the jaw's rearmost point in Z.
    hinge = ray_at_z(np.array([spec["head"]["center"][0], cond_y]), view, proj, size, HINGE_Z)
    hinge[0] = 0.0
    hinge[1] = min(hinge[1], float(jv[:, 1].max()))
    hinge[2] = float(jv[:, 2].min())  # the jaw's rearmost point: the condyles' plane

    anc, eye_radius = anchors(spec, posmap, cover, view, proj, size, sv)

    # Decimate each piece to its share of the budget, then one atlas for both.
    share = len(jf) / (len(sf) + len(jf))
    budget_j = max(1500, int(a.budget * min(max(share, 0.12), 0.3)))
    parts = []
    for pv, pf, cap, tgt in ((sv, sf, scap, a.budget - budget_j), (jv, jf, jcap, budget_j)):
        # Decimate without the caps, then cap again: the caps are fans the decimator would mangle.
        body_v, body_f, _ = geom.submesh(pv, pf, ~cap)
        dv, df = decimate(body_v, body_f, tgt - 200)
        dv, df, dcap = geom.cap_holes(dv, df)
        parts.append({"v": dv, "f": df, "cap": dcap})
    tris = sum(len(p["f"]) for p in parts)
    _log(f"decimate: skull {len(parts[0]['f'])} + jaw {len(parts[1]['f'])} = {tris} tris")
    t1 = time.time()
    parts = unwrap(parts, a.tex)
    _log(f"unwrap: {time.time() - t1:.0f}s")
    swatches = bone_swatches(a.work) if a.unseen == "synth" else None
    atlas, nmap, front_share = bake(parts, a.tex, v, c, f, layers[..., :3], matte, posmap, cover,
                                    view, proj, size, r, swatches=swatches, ao_strength=a.ao,
                                    normals=a.normal)
    base_png, base_mime = image_bytes(atlas, a.tex_format, a.jpeg_quality)
    norm_png, norm_mime = (image_bytes(nmap, a.tex_format, 95) if nmap is not None else (None, "image/png"))

    skull_p, jaw_p = parts

    def mesh(p, rel=(0.0, 0.0, 0.0)):
        return Mesh(p["v"] - rel, p["f"], geom.vertex_normals(p["v"], p["f"]), p["uv"], base_png,
                    texture_mime=base_mime, normal_texture=norm_png, normal_mime=norm_mime)

    nodes = [Node("skull", (0.0, 0.0, 0.0), mesh(skull_p)),
             Node("jaw", tuple(hinge), mesh(jaw_p, hinge))]
    nodes += [Node(n, tuple(float(x) for x in anc[n])) for n in ("eye_l", "eye_r", "monocle", "neck")]
    collar = None
    if a.collar != "none":
        from . import collar as collar_mod

        collar = collar_mod.make(a, r, v, f, view, proj, pos, target, size, portrait, _log)
        if collar is not None:
            nodes.append(Node("collar", tuple(float(x) for x in anc["neck"]),
                              Mesh(collar["v"] - anc["neck"], collar["f"],
                                   geom.vertex_normals(collar["v"], collar["f"]), collar["uv"],
                                   collar["png"], texture_mime=collar["mime"])))
            tris += len(collar["f"])
    data = write_glb(nodes)
    out = a.out or a.rig
    out.mkdir(parents=True, exist_ok=True)
    (out / "lich.glb").write_bytes(data)
    lich = {
        "version": 1,
        "source_model": f"{meta['model']}:{meta['input']}:seed{meta['seed']}",
        "portrait_sha256": prep.sha256(a.portrait),
        "tris": tris,
        "jaw_open_rad": a.jaw_open,
        "eye_radius": round(eye_radius, 4),
        "camera": {"fov": fov, "pos": [round(float(x), 4) for x in pos], "target": [round(float(x), 4) for x in target]},
    }
    (out / "lich.json").write_text(json.dumps(lich, indent=1), encoding="utf-8")
    rep = contract.check(out / "lich.glb")
    _log(f"export: {len(data)} bytes, {tris} tris, camera IoU {iou:.3f}, portrait covers {front_share:.0%} of the atlas, "
         f"{time.time() - t0:.0f}s")
    for e in rep.errors:
        _log(f"error: {e}")
    if a.runs:
        renders(a.runs, parts, atlas, hinge, anc, lich, portrait, view, proj, size, nmap, collar)
        if nmap is not None:
            from PIL import Image as _I

            _I.fromarray(nmap).save(a.runs / "normal.png")
    return 0 if rep.ok else 1


def renders(runs: Path, parts, tex, hinge, anc, lich, portrait, view, proj, size, nmap=None, collar=None) -> None:
    from PIL import Image

    runs.mkdir(parents=True, exist_ok=True)
    skull, jaw = parts

    def meshes(open_rad: float, shaded: bool = False, with_collar: bool = True):
        """The pieces for the review renderer; ``shaded`` bends the lighting through the normal map."""
        jv = jaw_rotate(jaw["v"] - hinge, open_rad) + hinge
        out = []
        for p, v in ((skull, skull["v"]), (jaw, jv)):
            m = {"v": v, "f": p["f"], "uv": p["uv"], "tex": tex}
            if shaded and nmap is not None:
                m.update(nmap=nmap, n=geom.vertex_normals(p["v"], p["f"]))
            out.append(m)
        if collar is not None and with_collar:
            out.append({"v": collar["v"], "f": collar["f"], "uv": collar["uv"], "tex": collar["img"]})
        return out

    r = Renderer(size)
    eye = np.array(lich["camera"]["pos"])
    front, _ = r.draw(meshes(0.0), view, proj, eye=eye, light=eye + np.array([-1, 1.5, 0]), lit=0.0)
    lit, _ = r.draw(meshes(0.0, True), view, proj, eye=eye, light=eye + np.array([-1, 1.5, 0]), lit=1.0)
    side = np.concatenate([(portrait * 255).astype(np.uint8), front, lit], axis=1)
    Image.fromarray(side).save(runs / "front_vs_portrait.png")
    small = Renderer(512)
    frames = []
    for rad in (0.0, lich["jaw_open_rad"]):
        for yaw in (0, 35, 90):
            e = np.array([math.sin(math.radians(yaw)), 0.05, math.cos(math.radians(yaw))]) * 4.2
            tgt = np.array([0.0, -0.08, 0.0])
            rgb, _ = small.draw(meshes(rad, True), look_at(e + tgt, tgt), perspective(22), eye=e + tgt,
                                light=e + np.array([-1, 1.5, 0.3]))
            frames.append(rgb)
    Image.fromarray(review.sheet(frames, 3)).save(runs / "jaw_closed_open.png")
    # The cut itself: the jaw in red, the hinge as a yellow pin through it.
    pin = np.array([[-0.45, -0.01, -0.01], [0.45, -0.01, -0.01], [0.45, 0.01, 0.01], [-0.45, 0.01, 0.01]]) + hinge
    split = []
    for yaw in (0, 60, 90):
        e = np.array([math.sin(math.radians(yaw)), 0.05, math.cos(math.radians(yaw))]) * 4.2
        rgb, _ = small.draw(
            [{"v": skull["v"], "f": skull["f"], "c": np.full((len(skull["v"]), 3), 0.8)},
             {"v": jaw["v"], "f": jaw["f"], "c": np.tile([0.85, 0.2, 0.15], (len(jaw["v"]), 1))},
             {"v": pin, "f": np.array([[0, 1, 2], [0, 2, 3]]), "c": np.tile([1.0, 0.9, 0.0], (4, 1))}],
            look_at(e, (0, -0.08, 0)), perspective(22), eye=e, light=e + np.array([-1, 1.5, 0.3]))
        split.append(rgb)
    Image.fromarray(review.sheet(split, 3)).save(runs / "jaw_split.png")
    # Anchors drawn as dots on the front render.
    img = front.copy()
    for name in ("eye_l", "eye_r", "monocle", "neck"):
        x, y, _ = project(np.array([anc[name]]), view, proj, size)[0]
        xi, yi = int(x), int(y)
        img[max(yi - 4, 0) : yi + 5, max(xi - 4, 0) : xi + 5] = (255, 0, 255) if name != "neck" else (0, 255, 0)
    hx, hy, _ = project(np.array([hinge]), view, proj, size)[0]
    img[int(hy) - 4 : int(hy) + 5, int(hx) - 4 : int(hx) + 5] = (255, 255, 0)
    Image.fromarray(img).save(runs / "anchors.png")
    tt = review.turntable_frames(Renderer(384), meshes(0.0, True), **({"dist": 4.6, "target": (0.0, -0.3, 0.0)} if collar is not None else {}))
    Image.fromarray(review.sheet(tt, 4)).save(runs / "final_turntable.png")
    # The lineup's side views: yaw -35 and +35 from the front, close, lit through the normal map.
    big = Renderer(640)
    yaws = []
    for yaw in (-35, 35):
        e = np.array([math.sin(math.radians(yaw)), 0.05, math.cos(math.radians(yaw))]) * 4.2
        tgt = np.array([0.0, -0.08, 0.0])
        rgb, _ = big.draw(meshes(0.0, True), look_at(e + tgt, tgt), perspective(fit.FOV + 2), eye=e + tgt,
                          light=e + np.array([-1.5, 2.0, 0.5]))
        yaws.append(rgb)
    Image.fromarray(review.sheet(yaws, 2)).save(runs / "yaw35.png")
    Image.fromarray(tex).save(runs / "atlas.png")
    _log(f"renders: {runs}")
