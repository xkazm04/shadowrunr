# SPDX-License-Identifier: Apache-2.0
"""Review renders: a candidate's turntable, and contact sheets.

A turntable is eight views a yaw of 45 degrees apart, a little from above, after the candidate is
oriented by :mod:`.fit`. The frames and a 4 x 2 sheet go under ``<runs>/<candidate>/``. The
orientation found is saved beside the candidate (``orient.json``) so ``build`` starts from it.
"""

from __future__ import annotations

import json
import math
from pathlib import Path

import numpy as np

from . import fit
from .render import Renderer, look_at, perspective


def load_candidate(cand: Path) -> tuple[np.ndarray, np.ndarray, np.ndarray, dict]:
    z = np.load(cand / "mesh.npz")
    meta = json.loads((cand / "meta.json").read_text(encoding="utf-8"))
    return z["v"].astype(np.float64), z["f"].astype(np.int64), z["c"].astype(np.float32), meta


def reference(work: Path, which: str) -> np.ndarray:
    from PIL import Image

    return np.asarray(Image.open(work / f"input_{which}.png").convert("RGBA"))


def oriented(cand: Path, log=print) -> tuple[np.ndarray, np.ndarray, np.ndarray, dict]:
    """The candidate in the contract frame (faces +Z, up +Y, one unit tall, centred)."""
    v, f, c, meta = load_candidate(cand)
    path = cand / "orient.json"
    if path.is_file():
        m = np.array(json.loads(path.read_text(encoding="utf-8"))["rotation"])
    else:
        work = cand.parent.parent
        m, score = fit.orient(Renderer(fit.FIT), v, f, c, reference(work, meta["input"]), log)
        path.write_text(json.dumps({"rotation": m.tolist(), "score": round(score, 4)}), encoding="utf-8")
    return fit.normalize(v @ m.T), f, c, meta


def sheet(frames: list[np.ndarray], cols: int) -> np.ndarray:
    h, w = frames[0].shape[:2]
    rows = math.ceil(len(frames) / cols)
    out = np.zeros((rows * h, cols * w, 3), np.uint8)
    for i, fr in enumerate(frames):
        r, c = divmod(i, cols)
        out[r * h : (r + 1) * h, c * w : (c + 1) * w] = fr
    return out


def turntable_frames(r: Renderer, meshes: list[dict], dist: float = 3.2, pitch: float = 10.0,
                     target=(0.0, 0.0, 0.0), yaws=None) -> list[np.ndarray]:
    frames = []
    tgt = np.asarray(target, float)
    for deg in (yaws if yaws is not None else [45 * k for k in range(8)]):
        yaw = math.radians(deg)
        p = math.radians(pitch)
        eye = np.array([math.sin(yaw) * math.cos(p), math.sin(p), math.cos(yaw) * math.cos(p)]) * dist + tgt
        rgb, _ = r.draw(meshes, look_at(eye, tgt), perspective(fit.FOV + 2), eye=eye,
                        light=eye + np.array([-1.5, 2.0, 0.5]))
        frames.append(rgb)
    return frames


def turntable(cand: Path, runs: Path) -> Path:
    from PIL import Image

    v, f, c, meta = oriented(cand)
    out = runs / cand.name
    out.mkdir(parents=True, exist_ok=True)
    frames = turntable_frames(Renderer(384), [{"v": v, "f": f, "c": c}])
    for k, fr in enumerate(frames):
        Image.fromarray(fr).save(out / f"turntable_{k * 45:03d}.png")
    Image.fromarray(sheet(frames, 4)).save(out / "turntable.png")
    print(f"turntable: {out / 'turntable.png'} ({meta['tris']} tris, seed {meta['seed']})")
    return out


def glb_meshes(path: Path) -> list[dict]:
    """The meshes of a ``lich.glb`` for the review renderer, in world space (node translations
    applied, the jaw closed), with their atlases and normal maps decoded."""
    import io

    from PIL import Image

    from .glb import read_glb

    g = read_glb(Path(path).read_bytes())
    cache: dict[int, np.ndarray] = {}

    def image(tex_ref):
        idx = g.json["textures"][tex_ref["index"]]["source"]
        if idx not in cache:
            view = g.json["bufferViews"][g.json["images"][idx]["bufferView"]]
            raw = g.bin[view.get("byteOffset", 0) : view.get("byteOffset", 0) + view["byteLength"]]
            cache[idx] = np.asarray(Image.open(io.BytesIO(raw)).convert("RGB"))
        return cache[idx]

    out = []
    for n in g.json["nodes"]:
        if "mesh" not in n:
            continue
        prim = g.json["meshes"][n["mesh"]]["primitives"][0]
        v = g.accessor(prim["attributes"]["POSITION"]).astype(np.float64) + np.array(n.get("translation", [0, 0, 0]))
        f = g.accessor(prim["indices"]).astype(np.int64).reshape(-1, 3)
        uv = g.accessor(prim["attributes"]["TEXCOORD_0"]).astype(np.float32)
        mat = g.json["materials"][prim["material"]]
        m = {"v": v, "f": f, "uv": uv, "tex": image(mat["pbrMetallicRoughness"]["baseColorTexture"])}
        if "normalTexture" in mat:
            m["nmap"] = image(mat["normalTexture"])
            m["n"] = g.accessor(prim["attributes"]["NORMAL"]).astype(np.float32)
        out.append(m)
    return out


def candidate_sheet(entries: list[tuple[str, Path]], out: Path, size: int = 256) -> Path:
    """One row of ten views per asset (yaw -35, +35, 0, 45 ... 315) from the GLBs themselves,
    labelled on the left; the stage's own look, normal maps included."""
    from PIL import Image, ImageDraw

    yaws = [-35, 35, 0, 45, 90, 135, 180, 225, 270, 315]
    r = Renderer(size)
    rows = []
    for label, path in entries:
        frames = turntable_frames(r, glb_meshes(path), dist=4.6, pitch=8.0, target=(0.0, -0.3, 0.0), yaws=yaws)
        row = Image.fromarray(np.concatenate(frames, axis=1))
        ImageDraw.Draw(row).text((6, 6), label, fill=(255, 220, 120))
        rows.append(np.asarray(row))
    out.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(np.concatenate(rows, axis=0)).save(out)
    return out
