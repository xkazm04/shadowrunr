# SPDX-License-Identifier: Apache-2.0
"""``python -m shadowrunr_rig``: split a portrait into the stage's face rig.

    python -m shadowrunr_rig --portrait <portrait.png> --spec <spec.json> --out <rig dir>
        [--lama <big-lama.pt>] [--device cuda|cpu] [--debug <dir>] [--id <portrait id>]

Publishes ``layers.json`` and one PNG per layer into ``--out`` through :mod:`.rigdir` (only the
files the old ``layers.json`` named are cleared; a depth map made from another rig is removed,
``depth.png`` and ``depth.json`` together) and prints how closely the rest frame recomposes the
portrait. ``--lama`` (or ``SHADOWRUNR_LAMA``)
names the big-lama TorchScript file; it is needed only when an element is filled by inpainting.
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path

import numpy as np

from . import formspec as fsm
from . import rigdir
from .split import assemble, debug_overlay, split


def _save(arr: np.ndarray, path: Path) -> None:
    from PIL import Image

    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr).save(path, compress_level=6)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="shadowrunr_rig", description=__doc__.split("\n\n")[0])
    ap.add_argument("--portrait", required=True, type=Path)
    ap.add_argument("--spec", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
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
    try:
        report = rigdir.publish(a.out, fs, pictures)
    except fsm.SpecError as e:
        print(f"publish: {e}", file=sys.stderr)
        return 1
    rest = rigdir.compose_rest(fs, pictures)
    err = np.abs(rest - img.astype(np.float32)).max(axis=2)
    print(f"wrote {report.layers} ({len(fs['layers'])} layers) in {time.time() - t0:.1f}s")
    for note in report.notes:
        print(note)
    print(
        f"rest frame vs portrait: mean {err.mean():.2f}, p99 {np.percentile(err, 99):.1f}, max {err.max():.0f} (0..255)"
    )
    if a.debug:
        _save(debug_overlay(result), a.debug / "regions.png")
        _save(
            np.clip(np.rint(result.background), 0, 255).astype(np.uint8), a.debug / "background.png"
        )
        _save(np.rint(rest).astype(np.uint8), a.debug / "rest.png")
        _save(np.clip(err * 4, 0, 255).astype(np.uint8), a.debug / "rest-error-x4.png")
    return 0


if __name__ == "__main__":
    sys.exit(main())
