# SPDX-License-Identifier: Apache-2.0
"""The rig folder (``<data>/avatar/rig``) and its one owner: numpy and Pillow, no torch.

Two tools write the folder: the splitter (``python -m shadowrunr_rig``) writes ``layers.json``
and its PNGs, and the depth prep (``tools/avatar/depth_map.py``) writes ``depth.png`` and
``depth.json`` beside them, recording in ``depth.json``'s ``inputs`` the sha256 of every rig file
it read. Both go through this module:

- :func:`load` reads the rig back with the stage's checks (:func:`formspec.check_formspec`: a
  layer's file is one path segment) and refuses a picture that is not its bbox's size;
- :func:`compose_rest` is the rest frame as the stage's glow shader draws it, the one compositor
  (the splitter judges a split by it, the depth prep shows it to the depth model);
- :func:`publish` replaces a rig: it clears only the files the old ``layers.json`` named, writes
  ``layers.json`` last, and removes the depth pair (both files, never one) when the new rig is not
  the one the depth map was made from;
- :func:`depth_status` says whether the depth map beside the rig was made from it.

Files the rig does not name (``notes.png``, ``learned.*``) are never touched.
"""

from __future__ import annotations

import hashlib
import json
import os
from dataclasses import dataclass, field
from io import BytesIO
from pathlib import Path

import numpy as np

from . import formspec as fsm

LAYERS = "layers.json"
#: The depth prep's pair. They are written together and are only ever removed together.
DEPTH_PNG = "depth.png"
DEPTH_JSON = "depth.json"
#: What to run when the depth map no longer matches the rig.
DEPTH_TOOL = "tools/avatar/depth_map.py"


@dataclass
class LoadedRig:
    """A rig read back from its folder: the checked FormSpec, each picture (file -> uint8 RGBA,
    its bbox's size) and the bytes of every file read (``layers.json`` included)."""

    fs: dict
    pictures: dict[str, np.ndarray] = field(default_factory=dict)
    files: dict[str, bytes] = field(default_factory=dict)


@dataclass
class DepthStatus:
    """``fresh``: depth.json's inputs are the rig's files as they are; ``stale``: some are not
    (``changed`` names them); ``missing``: no usable depth pair."""

    state: str
    changed: list[str] = field(default_factory=list)
    reason: str = ""


@dataclass
class PublishReport:
    layers: Path
    written: list[str]
    removed: list[str]
    depth: str
    notes: list[str] = field(default_factory=list)


def _box(layer: dict, fs: dict) -> tuple[int, int, int, int]:
    x, y, w, h = layer.get("bbox", [0, 0, fs["width"], fs["height"]])
    return int(round(x)), int(round(y)), int(round(w)), int(round(h))


def _decode(data: bytes) -> np.ndarray:
    from PIL import Image

    with Image.open(BytesIO(data)) as im:
        return np.asarray(im.convert("RGBA"), dtype=np.uint8).copy()


def _encode(pic: np.ndarray) -> bytes:
    from PIL import Image

    buf = BytesIO()
    Image.fromarray(np.asarray(pic, np.uint8)).save(buf, format="PNG", compress_level=6)
    return buf.getvalue()


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def check_pictures(fs: dict, pictures: dict[str, np.ndarray]) -> None:
    """Every file the FormSpec names is a picture of its bbox's size."""
    for layer in fs["layers"]:
        file = layer.get("file")
        if not file:
            continue
        pic = pictures.get(file)
        if pic is None:
            raise fsm.SpecError(f"layer {layer['name']}: no picture {file}")
        _, _, w, h = _box(layer, fs)
        if pic.shape[:2] != (h, w):
            raise fsm.SpecError(
                f"layer {layer['name']}: {file} is {pic.shape[1]}x{pic.shape[0]}, its bbox {w}x{h}"
            )


def owned_files(fs: dict) -> set[str]:
    """The files a rig owns in its folder: ``layers.json`` and every file it names."""
    return {LAYERS} | {layer["file"] for layer in fs["layers"] if layer.get("file")}


def load(folder: Path) -> LoadedRig:
    """The rig in `folder`, checked as the stage checks it. Raises :class:`formspec.SpecError`
    (a ``ValueError``) on a malformed ``layers.json`` or a picture that is not its bbox."""
    folder = Path(folder)
    raw = (folder / LAYERS).read_bytes()
    try:
        fs = json.loads(raw)
    except json.JSONDecodeError as e:
        raise fsm.SpecError(f"{LAYERS}: {e}") from e
    fsm.check_formspec(fs)
    rig = LoadedRig(fs=fs, files={LAYERS: raw})
    for layer in fs["layers"]:
        file = layer.get("file")
        if not file or file in rig.pictures:
            continue
        data = (folder / file).read_bytes()
        rig.files[file] = data
        rig.pictures[file] = _decode(data)
    check_pictures(fs, rig.pictures)
    return rig


def compose_rest(fs: dict, pictures: dict[str, np.ndarray]) -> np.ndarray:
    """The rig's rest frame as the renderer draws it (no bloom, no motion): solids cover what is
    behind them by alpha x occl and add their light at gain x alpha; lights (and the backplate)
    add at gain, whatever their alpha (the glow shader: ``cov = u_occl > 0 ? c.a : 1``).
    (H, W, 3) float 0..255, to compare with the portrait."""
    h, w = int(fs["height"]), int(fs["width"])
    out = np.zeros((h, w, 3), np.float32)
    for layer in fs["layers"]:
        file = layer.get("file")
        if not file:
            continue
        x, y, bw, bh = _box(layer, fs)
        x0, y0, x1, y1 = max(0, x), max(0, y), min(w, x + bw), min(h, y + bh)
        if x1 <= x0 or y1 <= y0:
            continue
        pic = pictures[file][y0 - y : y1 - y, x0 - x : x1 - x].astype(np.float32)
        rgb, a = pic[..., :3], pic[..., 3:] / 255.0
        gain = float(layer.get("gain", 1.0))
        dst = out[y0:y1, x0:x1]
        if layer.get("mode") == "light" or layer["kind"] == "static":
            dst += rgb * gain
        else:
            occl = a * float(layer.get("occl", 1.0))
            dst *= 1 - occl
            dst += rgb * gain * a
    return np.clip(out, 0, 255)


# ---------------------------------------------------------------- the depth pair


def _depth_inputs(folder: Path) -> dict[str, str] | None:
    """depth.json's recorded inputs, or None when there is no usable depth pair."""
    if not (folder / DEPTH_JSON).is_file() or not (folder / DEPTH_PNG).is_file():
        return None
    try:
        meta = json.loads((folder / DEPTH_JSON).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    inputs = meta.get("inputs") if isinstance(meta, dict) else None
    if not isinstance(inputs, dict) or LAYERS not in inputs:
        return None
    return inputs


def _changed(inputs: dict[str, str], current: dict[str, bytes], folder: Path) -> list[str]:
    """The recorded inputs whose bytes are not what `current` (else the folder) holds now."""
    changed = []
    for name, sha in sorted(inputs.items()):
        data = current.get(name)
        if data is None and (name == LAYERS or fsm.FILE_RE.match(str(name))):
            p = folder / name
            data = p.read_bytes() if p.is_file() else None
        if data is None or _sha(data) != sha:
            changed.append(name)
    return changed


def depth_status(folder: Path) -> DepthStatus:
    """Whether the depth map beside the rig in `folder` was made from the rig as it is now."""
    folder = Path(folder)
    inputs = _depth_inputs(folder)
    if inputs is None:
        return DepthStatus("missing", reason=f"no usable {DEPTH_PNG} + {DEPTH_JSON} pair")
    changed = _changed(inputs, {}, folder)
    if changed:
        return DepthStatus("stale", changed, f"changed since the depth map: {', '.join(changed)}")
    return DepthStatus("fresh")


def remove_depth(folder: Path) -> list[str]:
    """Remove the depth pair, both files (the stage draws the glow face without them)."""
    gone = []
    for name in (DEPTH_PNG, DEPTH_JSON):
        p = Path(folder) / name
        if p.exists():
            p.unlink()
            gone.append(name)
    return gone


# ---------------------------------------------------------------- publishing a rig


def _old_owned(out: Path) -> set[str]:
    """What the rig now in `out` owns, read from its layers.json (nothing if it has none or it
    cannot be read: then no file is cleared but the ones the new rig overwrites)."""
    try:
        fs = json.loads((out / LAYERS).read_text(encoding="utf-8"))
        layers = fs["layers"]
    except (OSError, ValueError, KeyError, TypeError):
        return set()
    files = {layer.get("file") for layer in layers if isinstance(layer, dict) and layer.get("file")}
    return {f for f in files if isinstance(f, str) and fsm.FILE_RE.match(f)}


def _write(path: Path, data: bytes) -> None:
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_bytes(data)
    os.replace(tmp, path)


def publish(out: Path, fs: dict, pictures: dict[str, np.ndarray]) -> PublishReport:
    """Replace the rig in `out` with `fs` and its `pictures`.

    Checked first (nothing is touched if the rig would not load back); the depth pair is removed,
    both files, when the new rig is not byte for byte the one it was made from; the files only the
    old ``layers.json`` named are cleared; the pictures are written, and ``layers.json`` last."""
    fsm.check_formspec(fs)
    check_pictures(fs, pictures)
    named = owned_files(fs) - {LAYERS}
    new: dict[str, bytes] = {file: _encode(pictures[file]) for file in sorted(named)}
    new[LAYERS] = (json.dumps(fs, indent=1) + "\n").encode("utf-8")
    return _publish(Path(out), new)


def publish_files(
    out: Path, files: dict[str, bytes], depth: tuple[bytes, bytes] | None = None
) -> PublishReport:
    """:func:`publish` from a rig's files as they are, byte for byte (a staged candidate moved
    live, or a kept rig put back): `files` is ``layers.json`` and every file it names, as
    :attr:`LoadedRig.files` holds them. `depth`, when given, is the ``(depth.png, depth.json)``
    pair that belongs with this rig: it is written back after ``layers.json``, both files."""
    if LAYERS not in files:
        raise fsm.SpecError(f"no {LAYERS}")
    try:
        fs = json.loads(files[LAYERS])
    except ValueError as e:
        raise fsm.SpecError(f"{LAYERS}: {e}") from e
    fsm.check_formspec(fs)
    named = owned_files(fs)
    missing = sorted(named - set(files))
    if missing:
        raise fsm.SpecError(f"{LAYERS} names {', '.join(missing)}, which is not given")
    check_pictures(fs, {f: _decode(files[f]) for f in named - {LAYERS}})
    report = _publish(Path(out), {f: files[f] for f in named})
    if depth is not None:
        png, meta = depth
        _write(Path(out) / DEPTH_PNG, png)
        _write(Path(out) / DEPTH_JSON, meta)
        status = depth_status(out)
        report.depth = status.state
        fresh = status.state == "fresh"
        report.notes = [] if fresh else [f"depth map is stale: run {DEPTH_TOOL} {out}"]
    return report


def _publish(out: Path, new: dict[str, bytes]) -> PublishReport:
    """Write a checked rig's encoded files into `out` (see :func:`publish`)."""
    named = set(new) - {LAYERS}
    out.mkdir(parents=True, exist_ok=True)
    notes: list[str] = []
    removed: list[str] = []
    inputs = _depth_inputs(out)
    if inputs is None:
        depth = "missing"
        removed += remove_depth(out)  # never one of the pair without the other
    else:
        # inputs the new rig still holds compare with its bytes; the rest with the folder
        changed = _changed(inputs, new, out)
        if changed:
            depth = "stale"
            removed += remove_depth(out)
            notes.append(f"depth map was made from another rig ({', '.join(changed)}): removed")
        else:
            depth = "fresh"
    if depth == "stale":
        notes.append(f"depth map is stale: run {DEPTH_TOOL} {out}")
    elif depth == "missing":
        notes.append(f"no depth map beside the rig: run {DEPTH_TOOL} {out}")

    for old in sorted(_old_owned(out) - named):
        p = out / old
        if p.is_file():
            p.unlink()
            removed.append(old)
    for file in sorted(named):
        _write(out / file, new[file])
    _write(out / LAYERS, new[LAYERS])
    return PublishReport(
        layers=out / LAYERS,
        written=sorted(named) + [LAYERS],
        removed=removed,
        depth=depth,
        notes=notes,
    )
