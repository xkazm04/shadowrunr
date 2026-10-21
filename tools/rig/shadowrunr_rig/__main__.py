# SPDX-License-Identifier: Apache-2.0
"""``python -m shadowrunr_rig``: split a portrait into the stage's face rig.

    python -m shadowrunr_rig --portrait <portrait.png> --spec <spec.json> --out <rig dir>
        [--candidate] [--lama <big-lama.pt>] [--device cuda|cpu] [--debug <dir>] [--id <id>]
    python -m shadowrunr_rig promote --out <rig dir> [--accept] [--tol 4] | [--revert]

Publishes ``layers.json`` and one PNG per layer into ``--out`` through :mod:`.rigdir` (only the
files the old ``layers.json`` named are cleared; a depth map made from another rig is removed,
``depth.png`` and ``depth.json`` together) and prints how closely the rest frame recomposes the
portrait. ``--lama`` (or ``SHADOWRUNR_LAMA``)
names the big-lama TorchScript file; it is needed only when an element is filled by inpainting.

``--candidate`` stages the new rig in ``<out>.next`` instead and leaves ``--out`` alone; every
run writes ``report.json`` (per element) and ``extremes.png`` (rest | jaw open | floats) beside
its rig. ``promote`` moves the candidate live, refusing an element whose p99 rose by more than
``--tol`` unless ``--accept``; the replaced rig is kept in ``<out>.prev`` and ``promote
--revert`` puts it back (see :mod:`.review`).
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path

import numpy as np

from . import formspec as fsm
from . import review, rigdir
from .split import assemble, split


def _save(arr: np.ndarray, path: Path) -> None:
    from PIL import Image

    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr).save(path, compress_level=6)


def promote_main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(
        prog="shadowrunr_rig promote", description="move the staged candidate rig live"
    )
    ap.add_argument("--out", required=True, type=Path, help="the live rig folder")
    ap.add_argument("--accept", action="store_true", help="promote even if an element got worse")
    ap.add_argument("--tol", type=float, default=review.TOL, help="p99 rise that counts as worse")
    ap.add_argument("--revert", action="store_true", help="put back the rig the last one replaced")
    a = ap.parse_args(argv)
    code, lines = review.revert(a.out) if a.revert else review.promote(a.out, a.accept, a.tol)
    for line in lines:
        print(line)
    return code


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else list(argv)
    if argv[:1] == ["promote"]:
        return promote_main(argv[1:])
    ap = argparse.ArgumentParser(prog="shadowrunr_rig", description=__doc__.split("\n\n")[0])
    ap.add_argument("--portrait", required=True, type=Path)
    ap.add_argument("--spec", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument(
        "--candidate",
        action="store_true",
        help="stage the rig in <out>.next and leave --out alone (then: promote --out <out>)",
    )
    ap.add_argument("--lama", type=Path, default=os.environ.get("SHADOWRUNR_LAMA") or None)
    ap.add_argument("--device", default="cuda")
    ap.add_argument(
        "--debug", type=Path, help="also write the region overlay, background and rest frame here"
    )
    ap.add_argument(
        "--id", help="the portrait id written into layers.json (default: the spec's portrait)"
    )
    a = ap.parse_args(argv)
    t0 = time.time()
    try:
        spec = fsm.load_spec(a.spec)
    except fsm.SpecError as e:
        print(f"spec: {e}", file=sys.stderr)
        return 2
    from PIL import Image

    img = np.array(Image.open(a.portrait).convert("RGB"))
    needs_model = any(
        el.get("mode") != "none" and el.get("fill", "inpaint") == "inpaint"
        for el in spec["elements"]
    )
    if needs_model:
        if a.lama is None:
            print(
                "an element is filled by inpainting: pass --lama <big-lama.pt> or set SHADOWRUNR_LAMA",
                file=sys.stderr,
            )
            return 2
        from .lama import Lama

        fill = Lama(a.lama, a.device).fill
    else:

        def fill(rgb, mask):  # never called
            raise RuntimeError("no inpainter")

    try:
        result = split(img, spec, fill)
        fs, pictures = assemble(spec, result, a.id or spec.get("portrait", a.portrait.parent.name))
    except fsm.SpecError as e:
        print(f"split: {e}", file=sys.stderr)
        return 1
    target = review.staged(a.out) if a.candidate else a.out
    try:
        report = rigdir.publish(target, fs, pictures)
    except fsm.SpecError as e:
        print(f"publish: {e}", file=sys.stderr)
        return 1
    judged = review.element_report(result, fs, pictures, img)
    review.write_review(target, judged, review.extremes(fs, pictures, spec))
    rest = rigdir.compose_rest(fs, pictures)
    err = np.abs(rest - img.astype(np.float32)).max(axis=2)
    print(f"wrote {report.layers} ({len(fs['layers'])} layers) in {time.time() - t0:.1f}s")
    for note in report.notes:
        print(note)
    print(
        f"rest frame vs portrait: mean {err.mean():.2f}, p99 {np.percentile(err, 99):.1f}, "
        f"max {err.max():.0f} (0..255)"
    )
    for line in review.report_lines(judged):
        print(line)
    if a.candidate:
        live = review.read_report(a.out)
        if live is not None:
            print(f"against the live rig in {a.out}:")
            for line in review.compare_lines(review.compare(live, judged)):
                print(line)
        print(f"staged in {target}; look at {target / review.EXTREMES}, then:")
        print(f"  python -m shadowrunr_rig promote --out {a.out}")
    if a.debug:
        _save(review.regions(result), a.debug / "regions.png")
        _save(
            np.clip(np.rint(result.background), 0, 255).astype(np.uint8), a.debug / "background.png"
        )
        _save(np.rint(rest).astype(np.uint8), a.debug / "rest.png")
        _save(np.clip(err * 4, 0, 255).astype(np.uint8), a.debug / "rest-error-x4.png")
    return 0


if __name__ == "__main__":
    sys.exit(main())
