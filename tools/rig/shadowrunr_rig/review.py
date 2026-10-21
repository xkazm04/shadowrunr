# SPDX-License-Identifier: Apache-2.0
"""Judge a split before the live stage sees it: stage a candidate, read it per element and at
its extremes, promote it, and take it back.

The live rig (``--out``) is the folder the daemon serves. A candidate run publishes into the
sibling ``<out>.next`` instead, and beside its rig writes:

- ``report.json`` (:func:`element_report`): one row per element with its bbox, the pixels it
  covers, the pixels its fill invented (``filled_px``) and the rest-frame error on the pixels it
  owns. A pixel is owned by the frontmost solid drawn there, or, when what that solid shows was
  painted in by an element's fill (an inpaint, the ground, a mouth's inside), by that element:
  the error is charged to whoever invented the pixel. Every pixel has exactly one owner; what no
  solid covers is the background's.
- ``extremes.png`` (:func:`extremes`): the rest frame, the jaw dropped by its ``uncover`` and the
  floats at the top of their swing, side by side: what a viewer sees when the rig moves.

:func:`promote` compares the candidate's report with the live one and refuses an element whose
p99 rose by more than ``tol`` unless accepted, keeps the live rig in ``<out>.prev`` and moves the
candidate in; :func:`revert` puts the kept rig back. The rig files go through
:func:`rigdir.publish_files` both ways (byte for byte, ``layers.json`` last): the rig folder keeps
one writer. Files the rig does not name (``notes.png``, ``learned.*``) are never touched.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
from io import BytesIO
from pathlib import Path

import numpy as np

from . import formspec as fsm
from . import rigdir
from .split import Split, debug_overlay, element_colour

REPORT = "report.json"
EXTREMES = "extremes.png"
#: the review's own files beside a rig: not the rig's, but they travel with it
REVIEW_FILES = (REPORT, EXTREMES)
#: in ``<out>.prev``: which live rig replaced the kept one (a revert puts it back only over that)
KEPT = "kept.json"
#: the default for :func:`promote`: an element whose p99 rises by more than this is worse
TOL = 4.0
#: the legend strip under the region overlay: one row this tall per element
LEGEND_ROW = 12

__all__ = [
    "element_colour",
    "element_report",
    "extremes",
    "compare",
    "regions",
    "promote",
    "revert",
]


def staged(out: Path) -> Path:
    """Where a candidate for the rig in `out` is staged."""
    out = Path(out)
    return out.with_name(out.name + ".next")


def kept(out: Path) -> Path:
    """Where the rig a promotion replaced is kept."""
    out = Path(out)
    return out.with_name(out.name + ".prev")


# ---------------------------------------------------------------- judging a split


def _stats(err: np.ndarray) -> dict:
    if err.size == 0:
        return {"mean": 0.0, "p99": 0.0, "max": 0.0}
    return {
        "mean": round(float(err.mean()), 2),
        "p99": round(float(np.percentile(err, 99)), 1),
        "max": round(float(err.max()), 1),
    }


def owners(result: Split) -> np.ndarray:
    """Which row owns each pixel: an index into ``result.lifted``, or ``len(result.lifted)``
    for the background (see the module doc)."""
    bg = len(result.lifted)
    src = result.source if result.source is not None else np.full((1, 1), -1, np.int16)
    own = np.where(src >= 0, src, bg).astype(np.int32)
    own = np.broadcast_to(own, result.background.shape[:2]).copy()
    front = np.zeros(own.shape, bool)
    for i, item in enumerate(result.lifted):  # front to back
        if item.rgba is None or item.el.get("mode") != "cut":
            continue
        solid = (item.rgba[..., 3] > 127) & ~front
        lifted_src = item.source if item.source is not None else np.full(own.shape, -1)
        own[solid] = np.where(lifted_src[solid] >= 0, lifted_src[solid], i)
        front |= solid
    return own


def element_report(
    result: Split, fs: dict, pictures: dict[str, np.ndarray], portrait: np.ndarray
) -> dict:
    """The per-element report of a split (``report.json``), its rows worst p99 first."""
    rest = rigdir.compose_rest(fs, pictures)
    err = np.abs(rest - np.asarray(portrait, np.float32)[..., :3]).max(axis=2)
    own = owners(result)
    boxes = {layer["name"]: layer.get("bbox") for layer in fs["layers"]}
    kinds = {layer["name"]: layer["kind"] for layer in fs["layers"]}
    h, w = err.shape
    rows = []
    entries = [(i, item.el["name"], item.el.get("mode")) for i, item in enumerate(result.lifted)]
    entries.append((len(result.lifted), "background", "static"))
    for i, name, mode in entries:
        mine = own == i
        cov = result.coverage.get(name)
        filled = result.filled.get(name)
        covered = int((cov > 0.5).sum()) if cov is not None else 0
        if name == "background":
            covered = h * w
        rows.append(
            {
                "name": name,
                "kind": kinds.get(name, "static"),
                "mode": mode,
                "bbox": boxes.get(name),
                "coverage_px": covered,
                "filled_px": int(filled.sum()) if filled is not None else 0,
                "owned_px": int(mine.sum()),
                "error": _stats(err[mine]),
            }
        )
    rows.sort(key=lambda r: (-r["error"]["p99"], -r["error"]["mean"], r["name"]))
    return {"version": 1, "width": w, "height": h, "rest": _stats(err), "elements": rows}


def _moved(fs: dict, by: dict[str, tuple[int, int]]) -> dict:
    out = json.loads(json.dumps(fs))
    for layer in out["layers"]:
        d = by.get(layer["name"])
        if d and layer.get("bbox"):
            layer["bbox"] = [layer["bbox"][0] + d[0], layer["bbox"][1] + d[1], *layer["bbox"][2:]]
    return out


def extremes(fs: dict, pictures: dict[str, np.ndarray], spec: dict) -> np.ndarray:
    """Three W x H panels side by side, uint8 RGB: the rest frame | every element with an
    ``uncover`` (the jaw) dropped by it | every float at ``+amp``. Each is
    :func:`rigdir.compose_rest` with those layers translated: the pixels a moving rig shows."""
    drops = {
        el["name"]: (0, int(el["uncover"])) for el in spec.get("elements", []) if "uncover" in el
    }
    lifts = {
        layer["name"]: tuple(int(round(float(v))) for v in layer["float"]["amp"])
        for layer in fs["layers"]
        if isinstance(layer.get("float"), dict) and "amp" in layer["float"]
    }
    panels = [fs, _moved(fs, drops), _moved(fs, lifts)]
    frames = [rigdir.compose_rest(p, pictures) for p in panels]
    return np.clip(np.rint(np.concatenate(frames, axis=1)), 0, 255).astype(np.uint8)


def compare(live: dict | None, candidate: dict, tol: float = TOL) -> list[dict]:
    """Each candidate element against the live report: ``worse`` when its p99 rose by more
    than `tol`, ``better`` when it fell by more, else ``same``; ``new`` when live has no such
    element. Worst first."""
    before = {r["name"]: r["error"]["p99"] for r in (live or {}).get("elements", [])}
    rows = []
    for r in candidate.get("elements", []):
        after = r["error"]["p99"]
        b = before.get(r["name"])
        if b is None:
            verdict = "new"
        elif after - b > tol:
            verdict = "worse"
        elif b - after > tol:
            verdict = "better"
        else:
            verdict = "same"
        rows.append({"name": r["name"], "before": b, "after": after, "verdict": verdict})
    rank = {"worse": 0, "new": 1, "same": 2, "better": 3}
    rows.sort(key=lambda r: (rank[r["verdict"]], -(r["after"] - (r["before"] or 0))))
    return rows


def compare_lines(rows: list[dict]) -> list[str]:
    out = []
    for r in rows:
        b = "-" if r["before"] is None else f"{r['before']:.1f}"
        out.append(f"{r['name']:<14} {r['verdict']:<6} p99 {b} -> {r['after']:.1f}")
    return out


def report_lines(report: dict) -> list[str]:
    """The per-element table a run prints, worst first."""
    lines = [f"{'element':<14} {'p99':>6} {'mean':>6} {'owned':>7} {'filled':>7}"]
    for r in report["elements"]:
        e = r["error"]
        lines.append(
            f"{r['name']:<14} {e['p99']:>6.1f} {e['mean']:>6.2f} {r['owned_px']:>7} "
            f"{r['filled_px']:>7}"
        )
    return lines


# ---------------------------------------------------------------- the region overlay


def regions(result: Split) -> np.ndarray:
    """The region overlay with a legend strip under it: one :data:`LEGEND_ROW` row per element,
    its colour swatch and its name. uint8 RGB, the overlay's width."""
    from PIL import Image, ImageDraw

    top = debug_overlay(result)
    w = top.shape[1]
    names = list(result.coverage)
    strip = Image.new("RGB", (w, LEGEND_ROW * len(names)), (0, 0, 0))
    draw = ImageDraw.Draw(strip)
    for i, name in enumerate(names):
        y = i * LEGEND_ROW
        draw.rectangle([2, y + 2, 10, y + LEGEND_ROW - 3], fill=element_colour(name))
        draw.text((14, y), name, fill=(235, 235, 235))
    return np.concatenate([top, np.asarray(strip, np.uint8)], axis=0)


# ---------------------------------------------------------------- files beside a rig


def _write(path: Path, data: bytes) -> None:
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_bytes(data)
    os.replace(tmp, path)


def png_bytes(rgb: np.ndarray) -> bytes:
    from PIL import Image

    buf = BytesIO()
    Image.fromarray(np.asarray(rgb, np.uint8)).save(buf, format="PNG", compress_level=6)
    return buf.getvalue()


def write_review(folder: Path, report: dict, sheet: np.ndarray) -> None:
    """``report.json`` and ``extremes.png`` beside the rig in `folder`."""
    folder = Path(folder)
    _write(folder / REPORT, (json.dumps(report, indent=1) + "\n").encode("utf-8"))
    _write(folder / EXTREMES, png_bytes(sheet))


def read_report(folder: Path) -> dict | None:
    try:
        return json.loads((Path(folder) / REPORT).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def _depth_pair(folder: Path) -> tuple[bytes, bytes] | None:
    png, meta = Path(folder) / rigdir.DEPTH_PNG, Path(folder) / rigdir.DEPTH_JSON
    if png.is_file() and meta.is_file():
        return png.read_bytes(), meta.read_bytes()
    return None


def _carry_review(src: Path, dst: Path) -> None:
    """Make `dst`'s review files what `src` holds (removing those `src` has not)."""
    for name in REVIEW_FILES:
        if (src / name).is_file():
            _write(dst / name, (src / name).read_bytes())
        elif (dst / name).is_file():
            (dst / name).unlink()


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


# ---------------------------------------------------------------- promote and revert


def promote(out: Path, accept: bool = False, tol: float = TOL) -> tuple[int, list[str]]:
    """Move the candidate in ``<out>.next`` into `out`; ``(exit code, lines to print)``.

    Refuses (1, nothing touched) when there is no candidate, it does not load, or an element's
    p99 rose by more than `tol` against ``<out>/report.json`` and `accept` is false. Otherwise
    the live rig, its depth pair and its review files are kept in ``<out>.prev`` (written whole
    before the live folder is touched), and the candidate replaces the live rig."""
    out = Path(out)
    nxt, prev = staged(out), kept(out)
    if not (nxt / rigdir.LAYERS).is_file():
        return 1, [f"no candidate in {nxt}: run the split with --candidate first"]
    try:
        cand = rigdir.load(nxt)
    except (OSError, fsm.SpecError) as e:
        return 1, [f"the candidate in {nxt} does not load: {e}"]
    lines: list[str] = []
    cand_report = read_report(nxt)
    live_report = read_report(out)
    if cand_report is not None and live_report is not None:
        rows = compare(live_report, cand_report, tol)
        lines += compare_lines(rows)
        worse = [r["name"] for r in rows if r["verdict"] == "worse"]
        if worse and not accept:
            lines.append(
                f"refused: p99 rose by more than {tol:g} on {', '.join(worse)}; the live rig is "
                "unchanged. --accept promotes anyway."
            )
            return 1, lines

    if (out / rigdir.LAYERS).is_file():
        try:
            live = rigdir.load(out)
        except (OSError, fsm.SpecError) as e:
            return 1, lines + [
                f"the live rig in {out} does not load ({e}): it cannot be kept for a revert, so "
                "nothing was promoted. Move it aside by hand."
            ]
        tmp = out.with_name(out.name + ".prev.tmp")
        if tmp.exists():
            shutil.rmtree(tmp)
        rigdir.publish_files(tmp, live.files, depth=_depth_pair(out))
        _carry_review(out, tmp)
        marker = {"replaced_by": _sha(cand.files[rigdir.LAYERS])}
        _write(tmp / KEPT, (json.dumps(marker) + "\n").encode("utf-8"))
        if prev.exists():
            shutil.rmtree(prev)
        os.replace(tmp, prev)
        lines.append(f"kept the live rig in {prev}")
    else:
        lines.append(f"no live rig in {out}: nothing to keep")

    depth = _depth_pair(nxt) if rigdir.depth_status(nxt).state == "fresh" else None
    report = rigdir.publish_files(out, cand.files, depth=depth)
    _carry_review(nxt, out)
    shutil.rmtree(nxt)
    lines.append(f"promoted {nxt} -> {out}")
    lines += report.notes
    if prev.is_dir() and (prev / KEPT).is_file():
        lines.append(f"undo: python -m shadowrunr_rig promote --out {out} --revert")
    return 0, lines


def revert(out: Path) -> tuple[int, list[str]]:
    """Put the rig kept by the last :func:`promote` back into `out`; ``(exit code, lines)``.

    Refuses (1, nothing touched) when nothing is kept, or the live rig is no longer the one that
    promotion put there (reverting would lose it)."""
    out = Path(out)
    prev = kept(out)
    if not (prev / rigdir.LAYERS).is_file() or not (prev / KEPT).is_file():
        return 1, [f"nothing to revert to: no kept rig in {prev}"]
    try:
        marker = json.loads((prev / KEPT).read_text(encoding="utf-8"))
        live_sha = _sha((out / rigdir.LAYERS).read_bytes())
    except (OSError, ValueError):
        marker, live_sha = {}, None
    if marker.get("replaced_by") != live_sha:
        return 1, [
            f"the live rig in {out} is not the one the last promotion put there: reverting would "
            f"lose it. Nothing changed; the kept rig is still in {prev}."
        ]
    try:
        rig = rigdir.load(prev)
    except (OSError, fsm.SpecError) as e:
        return 1, [f"the kept rig in {prev} does not load: {e}"]
    report = rigdir.publish_files(out, rig.files, depth=_depth_pair(prev))
    _carry_review(prev, out)
    shutil.rmtree(prev)
    return 0, [f"reverted {out} to the rig kept in {prev}", *report.notes]
