# SPDX-License-Identifier: Apache-2.0
"""The lich asset contract: what ``lich.glb`` and ``lich.json`` promise the stage's 3D face.

**The GLB.** Scene 0 holds six root nodes, named exactly:

- ``skull``: the cranium and upper teeth, a textured mesh in model space (node at the origin).
- ``jaw``: the mandible and lower teeth, a textured mesh whose node translation is the hinge
  (between the condyles). Its vertices are relative to the hinge, so rotating the node about
  its local +X opens the mouth. The jaw hangs below the hinge (+Y up) and reaches forward to the
  chin (+Z), so the hinge sits at the top and back of the jaw mesh.
- ``eye_l``, ``eye_r``: empties at the eye sockets' centres, slightly inside the bone. As in the
  2D rig, ``_l`` and ``_r`` are the image's left and right: ``eye_l`` has the smaller X.
- ``monocle``: an empty at the monocle ring's centre, on the image's right eye (``eye_r``).
- ``neck``: an empty at the head's pivot under the skull; head turns rotate about it.

The model faces +Z with +Y up, and is scaled so the skull and the closed jaw together are one
unit tall. Units are metres in glTF's eyes; they mean nothing here beyond that scale.

**lich.json** (version 1)::

    {"version": 1, "source_model": "<name>", "portrait_sha256": "<hex>", "tris": <n>,
     "jaw_open_rad": <max>, "eye_radius": <scene units>,
     "camera": {"fov": <deg, vertical>, "pos": [x, y, z], "target": [x, y, z]}}

``tris`` counts the triangles of ``skull`` and ``jaw`` together. ``camera`` is the square
(aspect 1) view whose render of the model best matches the portrait's framing of the head.

Everything here is the standard library and numpy, so the stage's builders and the tests can run
the check without the model stack.
"""

from __future__ import annotations

import json
import math
import re
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np

from .glb import GlbError, read_glb

NODE_NAMES = ("skull", "jaw", "eye_l", "eye_r", "monocle", "neck")
MESH_NODES = ("skull", "jaw")
EMPTY_NODES = ("eye_l", "eye_r", "monocle", "neck")
VERSION = 1
MAX_BYTES = 8 * 1024 * 1024
MAX_TRIS = 40_000

_HEX64 = re.compile(r"^[0-9a-f]{64}$")


@dataclass
class Report:
    errors: list[str] = field(default_factory=list)
    facts: dict = field(default_factory=dict)

    @property
    def ok(self) -> bool:
        return not self.errors

    def fail(self, msg: str) -> None:
        self.errors.append(msg)


def _num(v) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def _vec3(v) -> bool:
    return isinstance(v, list) and len(v) == 3 and all(_num(c) for c in v)


def check_meta(meta: dict, rep: Report) -> None:
    """lich.json on its own: fields, types and ranges."""
    if not isinstance(meta, dict):
        rep.fail("lich.json: not an object")
        return
    if meta.get("version") != VERSION:
        rep.fail(f"lich.json: version {meta.get('version')!r}, expected {VERSION}")
    if not isinstance(meta.get("source_model"), str) or not meta["source_model"]:
        rep.fail("lich.json: source_model must be a non-empty string")
    if not isinstance(meta.get("portrait_sha256"), str) or not _HEX64.match(meta["portrait_sha256"]):
        rep.fail("lich.json: portrait_sha256 must be 64 lowercase hex digits")
    tris = meta.get("tris")
    if not isinstance(tris, int) or isinstance(tris, bool) or tris <= 0:
        rep.fail("lich.json: tris must be a positive integer")
    jaw = meta.get("jaw_open_rad")
    if not _num(jaw) or not 0 < jaw <= 1.0:
        rep.fail("lich.json: jaw_open_rad must be in (0, 1] radians")
    er = meta.get("eye_radius")
    if not _num(er) or not 0 < er < 0.5:
        rep.fail("lich.json: eye_radius must be in (0, 0.5) scene units")
    cam = meta.get("camera")
    if not isinstance(cam, dict):
        rep.fail("lich.json: camera must be an object")
    else:
        if not _num(cam.get("fov")) or not 1 <= cam["fov"] <= 120:
            rep.fail("lich.json: camera.fov must be in [1, 120] degrees")
        for k in ("pos", "target"):
            if not _vec3(cam.get(k)):
                rep.fail(f"lich.json: camera.{k} must be three numbers")
        if _vec3(cam.get("pos")) and _vec3(cam.get("target")):
            if np.linalg.norm(np.subtract(cam["pos"], cam["target"])) < 1e-6:
                rep.fail("lich.json: camera.pos and camera.target coincide")
    for k, v in meta.items():
        if v is None:
            rep.fail(f"lich.json: {k} is null (omit optional fields instead)")


def check_glb(data: bytes, rep: Report, max_bytes: int = MAX_BYTES, max_tris: int = MAX_TRIS) -> None:
    rep.facts["bytes"] = len(data)
    if len(data) > max_bytes:
        rep.fail(f"lich.glb: {len(data)} bytes, over the {max_bytes} byte budget")
    try:
        g = read_glb(data)
    except (GlbError, ValueError, json.JSONDecodeError) as e:
        rep.fail(f"lich.glb: {e}")
        return
    nodes = g.json.get("nodes", [])
    names = [n.get("name") for n in nodes]
    scene = g.json.get("scenes", [{}])[g.json.get("scene", 0)].get("nodes", [])
    for want in NODE_NAMES:
        if names.count(want) != 1:
            rep.fail(f"lich.glb: node {want!r} appears {names.count(want)} times, expected once")
    extra = sorted({str(n) for n in names} - set(NODE_NAMES))
    if extra:
        rep.fail(f"lich.glb: unexpected nodes {extra}")
    if rep.errors:
        return
    by = g.nodes_by_name()
    for name in NODE_NAMES:
        if names.index(name) not in scene:
            rep.fail(f"lich.glb: node {name!r} is not a root node of the scene")
        n = by[name]
        if any(k in n for k in ("rotation", "scale", "matrix", "children")):
            rep.fail(f"lich.glb: node {name!r} must carry a translation only")
    for name in EMPTY_NODES:
        if "mesh" in by[name]:
            rep.fail(f"lich.glb: {name!r} must be an empty, not a mesh")
    for name in MESH_NODES:
        if "mesh" not in by[name]:
            rep.fail(f"lich.glb: {name!r} has no mesh")
    if rep.errors:
        return

    t = {n: np.array(by[n].get("translation", [0.0, 0.0, 0.0]), float) for n in NODE_NAMES}
    skull_v, skull_f = g.mesh_arrays(by["skull"]["mesh"])
    jaw_v, jaw_f = g.mesh_arrays(by["jaw"]["mesh"])
    tris = int(skull_f.shape[0] + jaw_f.shape[0])
    rep.facts.update({"tris": tris, "skull_tris": int(skull_f.shape[0]), "jaw_tris": int(jaw_f.shape[0])})
    if tris > max_tris:
        rep.fail(f"lich.glb: {tris} triangles, over the {max_tris} budget")
    for name, v, f in (("skull", skull_v, skull_f), ("jaw", jaw_v, jaw_f)):
        if f.size == 0:
            rep.fail(f"lich.glb: {name!r} mesh is empty")
        elif f.max() >= v.shape[0] or f.min() < 0:
            rep.fail(f"lich.glb: {name!r} indexes past its vertices")
        if not np.isfinite(v).all():
            rep.fail(f"lich.glb: {name!r} has non-finite positions")
    if rep.errors:
        return

    world = np.concatenate([skull_v + t["skull"], jaw_v + t["jaw"]])
    lo, hi = world.min(axis=0), world.max(axis=0)
    size = hi - lo
    rep.facts["height"] = round(float(size[1]), 4)
    if abs(size[1] - 1.0) > 0.02:
        rep.fail(f"lich.glb: skull and jaw are {size[1]:.3f} tall, expected 1 unit")

    # The jaw pivot: the hinge is at the top and back of the jaw, centred between the condyles.
    jlo, jhi = jaw_v.min(axis=0), jaw_v.max(axis=0)
    jsize = np.maximum(jhi - jlo, 1e-9)
    rep.facts["jaw_pivot"] = [round(float(c), 4) for c in t["jaw"]]
    if jhi[1] > 0.2 * jsize[1]:
        rep.fail("lich.glb: jaw pivot is not at the top of the jaw (the jaw must hang below its hinge)")
    if jlo[2] < -0.25 * jsize[2]:
        rep.fail("lich.glb: jaw pivot is not at the back of the jaw (the chin must lie toward +Z)")
    if jhi[2] <= 0.5 * jsize[2]:
        rep.fail("lich.glb: jaw does not reach forward (+Z) from its pivot")
    if abs(jlo[0] + jhi[0]) / 2 > 0.1 * jsize[0]:
        rep.fail("lich.glb: jaw pivot is not centred between the condyles (X)")
    if not (np.all(t["jaw"] >= lo - 1e-6) and np.all(t["jaw"] <= hi + 1e-6)):
        rep.fail("lich.glb: jaw pivot lies outside the head")

    # Anchors: eyes left and right of centre, the monocle on the image's right eye, the neck low.
    if not t["eye_l"][0] < 0 < t["eye_r"][0]:
        rep.fail("lich.glb: eye_l must have negative X and eye_r positive X")
    if np.linalg.norm(t["monocle"] - t["eye_r"]) > np.linalg.norm(t["monocle"] - t["eye_l"]):
        rep.fail("lich.glb: monocle is nearer eye_l than eye_r")
    if t["neck"][1] >= min(t["eye_l"][1], t["eye_r"][1]):
        rep.fail("lich.glb: neck is not below the eyes")
    for name in EMPTY_NODES:
        p = t[name]
        if not (np.all(p >= lo - 0.1 * size) and np.all(p <= hi + 0.1 * size)):
            rep.fail(f"lich.glb: {name!r} lies outside the head")


def check(glb_path: Path, meta_path: Path | None = None, max_bytes: int = MAX_BYTES,
          max_tris: int = MAX_TRIS) -> Report:
    """Validate a GLB and its lich.json (default: ``lich.json`` beside the GLB)."""
    rep = Report()
    glb_path = Path(glb_path)
    meta_path = Path(meta_path) if meta_path else glb_path.with_name("lich.json")
    try:
        data = glb_path.read_bytes()
    except OSError as e:
        rep.fail(f"lich.glb: {e}")
        return rep
    check_glb(data, rep, max_bytes, max_tris)
    try:
        meta = json.loads(meta_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as e:
        rep.fail(f"lich.json: {e}")
        return rep
    check_meta(meta, rep)
    if "tris" in rep.facts and meta.get("tris") != rep.facts["tris"]:
        rep.fail(f"lich.json: tris {meta.get('tris')} but the GLB has {rep.facts['tris']}")
    return rep
