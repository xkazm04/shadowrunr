# SPDX-License-Identifier: Apache-2.0
"""The region spec the splitter reads and the ``layers.json`` it writes, standard library only.

**The region spec** (``characters/<who>/rig/<portrait>.spec.json``) is hand-measured geometry on
one portrait: polygons and points in source pixels, never image data. It lists the portrait's
elements front to back; each is lifted off a working copy in that order (see ``split.py``).

**layers.json** is the glow rig's ``FormSpec``, the file the stage's face renderer reads
(``apps/stage/src/avatar/layers.ts``): layers back to front, each with a kind, a file, a bbox in
source pixels and the renderer's per-layer knobs, plus the rig's head, aperture, ground and lights.
:func:`check_formspec` applies the same checks as the stage's ``checkSpec`` so a rig that the
stage would refuse is never written. Optional fields are omitted, never null.
"""

from __future__ import annotations

import json
import math
import re
from pathlib import Path
from typing import Any

#: The layer kinds a rig may hold (the stage's ``LayerKind``, less the ground line it adds itself).
KINDS = frozenset(
    {
        "static",
        "body",
        "head",
        "float",
        "eye",
        "seam",
        "aperture_upper",
        "aperture_lower",
        "aperture_glow",
        "halo",
        "pulse",
    }
)
#: How an element leaves the working copy: as light, as a solid cut, or not at all (procedural).
MODES = frozenset({"light", "cut", "none"})
#: What is left under an element once it is lifted off.
FILLS = frozenset({"inpaint", "key", "ground", "keep"})
#: Region shapes.
REGIONS = frozenset({"poly", "ring", "rest"})
#: The renderer's per-layer fields an element passes through unchanged.
PASS_THROUGH = (
    "follow",
    "occl",
    "gain",
    "bloom",
    "voice_gain",
    "shimmer",
    "glint",
    "blink",
    "float",
    "pulse",
    "ring_w",
)
#: A layer's file: one path segment ending in .png (the stage's FILE_RE).
FILE_RE = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.-]*\.png$")
NAME_RE = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_-]*$")


class SpecError(ValueError):
    """A region spec or a FormSpec that cannot be used, with the reason."""


def _num(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def _tuple(v: Any, n: int) -> bool:
    return isinstance(v, list) and len(v) == n and all(_num(x) for x in v)


def _points(v: Any) -> bool:
    return isinstance(v, list) and len(v) >= 3 and all(_tuple(p, 2) for p in v)


def _rgb(v: Any) -> bool:
    return _tuple(v, 3) and all(0 <= x <= 1 for x in v)


# ---------------------------------------------------------------- the region spec


def check_region(name: str, reg: Any) -> None:
    if not isinstance(reg, dict):
        raise SpecError(f"{name}: region is not an object")
    shapes = [k for k in REGIONS if k in reg]
    if len(shapes) != 1:
        raise SpecError(f"{name}: a region names exactly one of {sorted(REGIONS)}")
    if "poly" in reg and not _points(reg["poly"]):
        raise SpecError(f"{name}: poly needs at least three [x, y] points")
    if "ring" in reg and not (_tuple(reg["ring"], 3) and _num(reg.get("width", 8))):
        raise SpecError(f"{name}: ring is [cx, cy, r] with a numeric width")
    for extra in ("also", "minus"):
        if extra in reg and not (
            isinstance(reg[extra], list) and all(_points(p) for p in reg[extra])
        ):
            raise SpecError(f"{name}: {extra} is a list of polygons")
    for k in ("grow", "feather", "lit", "close", "y_min"):
        if k in reg and not _num(reg[k]):
            raise SpecError(f"{name}: region {k} is not a number")


def check_spec(spec: Any) -> dict:
    """Check a region spec and return it. Raises :class:`SpecError` with the first problem."""
    if not isinstance(spec, dict):
        raise SpecError("the spec is not an object")
    if spec.get("rig_type") != "glow":
        raise SpecError("the spec is not for a glow rig")
    if not (_num(spec.get("width")) and _num(spec.get("height"))):
        raise SpecError("the spec has no size")
    head = spec.get("head")
    if not (
        isinstance(head, dict) and _tuple(head.get("center"), 2) and _tuple(head.get("radius"), 2)
    ):
        raise SpecError("the spec's head needs a center and a radius")
    if not _num(head.get("chin_y")):
        raise SpecError("the spec's head needs chin_y")
    if not _tuple(spec.get("ground_rgb", [0, 0, 0]), 3):
        raise SpecError("ground_rgb is [r, g, b] in 0..255")
    ap = spec.get("aperture")
    if ap is not None and not all(_num(ap.get(k)) for k in ("seam_y", "top", "bottom", "x0", "x1")):
        raise SpecError("the aperture needs seam_y, top, bottom, x0 and x1")
    colors = spec.get("colors", {})
    if not isinstance(colors, dict) or not all(
        k in ("mouth", "line_core", "line_glow") and _rgb(v) for k, v in colors.items()
    ):
        raise SpecError("colors are mouth / line_core / line_glow, each [r, g, b] in 0..1")
    ground = spec.get("ground")
    if ground is not None and not all(
        _num(ground.get(k)) for k in ("y", "half_width", "fade", "curve")
    ):
        raise SpecError("the ground needs y, half_width, fade and curve")
    els = spec.get("elements")
    if not isinstance(els, list) or not els:
        raise SpecError("the spec has no elements")
    seen: set[str] = set()
    for el in els:
        name = el.get("name") if isinstance(el, dict) else None
        if not isinstance(name, str) or not NAME_RE.match(name):
            raise SpecError(f"an element has no usable name: {name!r}")
        if name in seen or name == "background":
            raise SpecError(f"{name}: the name is taken")
        seen.add(name)
        if el.get("kind") not in KINDS or el["kind"] == "static":
            raise SpecError(f"{name}: unknown kind {el.get('kind')!r}")
        mode = el.get("mode")
        if mode not in MODES:
            raise SpecError(f"{name}: unknown mode {mode!r}")
        if mode == "none":
            if "region" in el:
                raise SpecError(f"{name}: a procedural element has no region")
            continue
        check_region(name, el.get("region"))
        fill = el.get("fill", "inpaint")
        if fill not in FILLS:
            raise SpecError(f"{name}: unknown fill {fill!r}")
        if fill == "key":
            key = el.get("key")
            if mode != "light" or not (isinstance(key, dict) and _tuple(key.get("base"), 3)):
                raise SpecError(f"{name}: a key fill is a light with key.base")
        if "inpaint_within" in el and not (
            isinstance(el["inpaint_within"], list) and all(_points(q) for q in el["inpaint_within"])
        ):
            raise SpecError(f"{name}: inpaint_within is a list of polygons")
        if "interior" in el:
            it = el["interior"]
            if mode != "cut" or not (
                isinstance(it, dict) and _points(it.get("poly")) and _tuple(it.get("color"), 3)
            ):
                raise SpecError(f"{name}: an interior is a cut's poly and color")
        depth = el.get("depth", 0)
        if not (_num(depth) or depth == "surface"):
            raise SpecError(f'{name}: depth is a number or "surface"')
        if "depth_at" in el and not _tuple(el["depth_at"], 2):
            raise SpecError(f"{name}: depth_at is [x, y]")
    if not any(el["kind"] == "head" for el in els):
        raise SpecError("the spec has no head element")
    return spec


def load_spec(path: Path) -> dict:
    try:
        raw = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        raise SpecError(f"{path}: {e}") from e
    return check_spec(raw)


# ---------------------------------------------------------------- layers.json


def surface_depth(anchor: tuple[float, float], head: dict) -> float:
    """The depth that keeps a layer on the head's surface when it turns.

    The renderer warps the head as an orthographic sphere whose depth at a point is
    0.6 x sqrt(1 - |u|^2) (u: the point relative to the head's centre, in radii); any other layer
    turns at its own fixed depth. A layer given the head's depth at its anchor turns with the bone
    under it instead of sliding over it."""
    ux = (anchor[0] - head["center"][0]) / head["radius"][0]
    uy = (anchor[1] - head["center"][1]) / head["radius"][1]
    return round(0.6 * math.sqrt(max(0.0, 1.0 - ux * ux - uy * uy)), 3)


def layer_entry(
    el: dict, z: int, bbox: tuple[int, int, int, int] | None, head: dict, file: str | None
) -> dict:
    """One layers.json entry for an element written at `z` with its picture's `bbox`."""
    entry: dict[str, Any] = {"name": el["name"], "z": z, "kind": el["kind"]}
    if file is not None:
        entry["file"] = file
    if bbox is not None:
        x, y, w, h = bbox
        entry["bbox"] = [int(x), int(y), int(w), int(h)]
        entry["anchor"] = [round(x + w / 2, 1), round(y + h / 2, 1)]
    depth = el.get("depth", 0)
    if depth == "surface":
        at = el.get("depth_at") or entry.get("anchor")
        if at is None:
            raise SpecError(f"{el['name']}: a surface depth needs a picture or depth_at")
        depth = surface_depth((at[0], at[1]), head)
    if depth:
        entry["depth"] = round(float(depth), 3)
    if el.get("mode") in ("light", "cut", "none"):
        entry["mode"] = el["mode"]
    for k in PASS_THROUGH:
        if k in el:
            entry[k] = el[k]
    return entry


def rig_section(spec: dict) -> dict:
    """The rig block: head, aperture, ground and lights, as the renderer reads them."""
    h = spec["head"]
    head: dict[str, Any] = {"center": list(h["center"]), "radius": list(h["radius"])}
    if "pivot_y" in h:
        head["pivot_y"] = h["pivot_y"]
    head["chin_y"] = h["chin_y"]
    rig: dict[str, Any] = {"head": head}
    if "aperture" in spec:
        a = spec["aperture"]
        rig["aperture"] = {
            k: a[k] for k in ("seam_y", "open_frac", "top", "bottom", "x0", "x1") if k in a
        }
    if "ground" in spec:
        rig["ground"] = dict(spec["ground"])
    if spec.get("colors"):
        rig["colors"] = {k: list(v) for k, v in spec["colors"].items()}
    return rig


def formspec(spec: dict, portrait_id: str, entries: list[dict]) -> dict:
    return {
        "version": 1,
        "portrait_id": portrait_id,
        "source": "glow",
        "rig_type": "glow",
        "width": int(spec["width"]),
        "height": int(spec["height"]),
        "layers": entries,
        "rig": rig_section(spec),
    }


def check_formspec(fs: Any) -> dict:
    """The stage's ``checkSpec``, so a rig it would refuse is never written."""
    if not isinstance(fs, dict):
        raise SpecError("layers.json is not an object")
    if fs.get("rig_type") != "glow":
        raise SpecError(f"layers.json is a {fs.get('rig_type')} rig, not a glow rig")
    if not (
        _num(fs.get("width")) and fs["width"] > 0 and _num(fs.get("height")) and fs["height"] > 0
    ):
        raise SpecError("layers.json has no size")
    rig = fs.get("rig")
    if not isinstance(rig, dict) or not isinstance(rig.get("head"), dict):
        raise SpecError("layers.json has no head")
    head = rig["head"]
    if not (
        _tuple(head.get("center"), 2) and _tuple(head.get("radius"), 2) and _num(head.get("chin_y"))
    ):
        raise SpecError("layers.json's head is malformed")
    layers = fs.get("layers")
    if not isinstance(layers, list) or not layers:
        raise SpecError("layers.json has no layers")
    for i, layer in enumerate(layers):
        if not isinstance(layer, dict) or not isinstance(layer.get("name"), str):
            raise SpecError(f"layer {i} has no name")
        if layer.get("kind") not in KINDS:
            raise SpecError(f"layer {layer['name']} has an unknown kind {layer.get('kind')}")
        if "bbox" in layer and not _tuple(layer["bbox"], 4):
            raise SpecError(f"layer {layer['name']} has a malformed bbox")
        if "file" in layer and not (
            isinstance(layer["file"], str) and FILE_RE.match(layer["file"])
        ):
            raise SpecError(f"layer {layer['name']} names a file that is not a single .png")
        if any(v is None for v in layer.values()):
            raise SpecError(f"layer {layer['name']} has a null field")
    ap = rig.get("aperture")
    if ap is not None and not (
        isinstance(ap, dict)
        and all(_num(ap.get(k)) for k in ("seam_y", "top", "bottom", "x0", "x1"))
    ):
        raise SpecError("layers.json's aperture is malformed")
    return fs


def write_layers_json(fs: dict, out_dir: Path) -> Path:
    check_formspec(fs)
    p = Path(out_dir) / "layers.json"
    p.write_text(json.dumps(fs, indent=1) + "\n", encoding="utf-8")
    return p
