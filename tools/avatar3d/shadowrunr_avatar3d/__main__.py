# SPDX-License-Identifier: Apache-2.0
"""``python -m shadowrunr_avatar3d``: Malakor's 3D head, from portrait to ``lich.glb``.

    python -m shadowrunr_avatar3d --check <lich.glb> [--meta <lich.json>] [--max-bytes N] [--max-tris N]
    python -m shadowrunr_avatar3d prep --rig <rig dir> --portrait <portrait.png> --work <work dir>
    python -m shadowrunr_avatar3d gen --model trellis|triposr|sf3d --work <work dir>
        [--input clean|portrait] [--seed N ...] [--weights <dir>]
    python -m shadowrunr_avatar3d turntable --work <work dir> --cand <name> --runs <dir>
    python -m shadowrunr_avatar3d sheet --glb <lich.glb> ... --out <png> [--label <name> ...]
    python -m shadowrunr_avatar3d build --work <work dir> --cand <name> --rig <rig dir> [--out <dir>]
        --spec <m8.spec.json> --portrait <portrait.png> [--runs <dir>] [--budget 80000] [--tex 4096] [--collar auto --bust <cand>]

``--check`` validates a GLB and its ``lich.json`` against the contract in :mod:`.contract` and
exits 0 when it holds, 1 when it does not. It needs numpy and nothing else.

``prep`` composes the skull alone from the rig's ``head`` and ``jaw`` layers (crystals, collar and
cloak are not in them) and pads it square for the models. ``gen`` runs one image-to-3D model per
seed and keeps each candidate's raw mesh under ``<work>/cand/``. ``turntable`` renders a
candidate from eight sides. ``build`` turns the chosen candidate into ``lich.glb`` and
``lich.json``: orient, fit the portrait's camera, cut the jaw, place the anchors, decimate, bake
the texture, then check the result.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from . import contract


def check_main(a: argparse.Namespace) -> int:
    rep = contract.check(Path(a.check), a.meta, a.max_bytes, a.max_tris)
    facts = ", ".join(f"{k}={v}" for k, v in rep.facts.items())
    if rep.ok:
        print(f"ok: {a.check} ({facts})")
        return 0
    for e in rep.errors:
        print(f"error: {e}")
    if facts:
        print(f"facts: {facts}")
    return 1


def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else list(argv)
    ap = argparse.ArgumentParser(prog="shadowrunr_avatar3d", description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", metavar="GLB", help="validate a GLB and its lich.json, then exit")
    ap.add_argument("--meta", type=Path, help="the lich.json to check (default: beside the GLB)")
    ap.add_argument("--max-bytes", type=int, default=contract.MAX_BYTES)
    ap.add_argument("--max-tris", type=int, default=contract.MAX_TRIS)
    sub = ap.add_subparsers(dest="cmd")

    p = sub.add_parser("prep", help="compose and pad the skull image the models see")
    p.add_argument("--rig", required=True, type=Path)
    p.add_argument("--portrait", required=True, type=Path)
    p.add_argument("--work", required=True, type=Path)

    p = sub.add_parser("gen", help="run an image-to-3D model on the prepped skull")
    p.add_argument("--model", required=True, choices=["trellis", "triposr", "sf3d"])
    p.add_argument("--work", required=True, type=Path)
    p.add_argument("--input", default="clean", choices=["clean", "clean2", "portrait", "bust"])
    p.add_argument("--seed", type=int, nargs="+", default=[1])
    p.add_argument("--weights", type=Path, help="the model's weights folder")
    p.add_argument("--steps", type=int, help="trellis: both samplers' step count (default 25)")

    p = sub.add_parser("turntable", help="render a candidate from eight sides")
    p.add_argument("--work", required=True, type=Path)
    p.add_argument("--cand", required=True)
    p.add_argument("--runs", required=True, type=Path)

    p = sub.add_parser("sheet", help="one row of ten views per GLB, side by side")
    p.add_argument("--glb", required=True, nargs="+", type=Path)
    p.add_argument("--label", nargs="+", help="a label per GLB (default: the folder name)")
    p.add_argument("--out", required=True, type=Path)

    p = sub.add_parser("build", help="make lich.glb and lich.json from a candidate")
    p.add_argument("--work", required=True, type=Path)
    p.add_argument("--cand", required=True)
    p.add_argument("--rig", required=True, type=Path, help="the 2D rig (its head and jaw layers)")
    p.add_argument("--out", type=Path, help="where lich.glb and lich.json go (default: --rig)")
    p.add_argument("--spec", required=True, type=Path)
    p.add_argument("--portrait", required=True, type=Path)
    p.add_argument("--runs", type=Path, help="where the review renders go")
    p.add_argument("--budget", type=int, default=80000, help="triangles, skull and jaw together")
    p.add_argument("--tex", type=int, default=2048, choices=[1024, 2048, 4096])
    p.add_argument("--tex-format", default="jpeg", choices=["jpeg", "png"],
                   help="the atlas and the normal map (jpeg keeps 4096 under the size budget)")
    p.add_argument("--jpeg-quality", type=int, default=92)
    p.add_argument("--unseen", default="synth", choices=["synth", "model"],
                   help="bone where the portrait's camera saw nothing: cut from the front, or the model's own")
    p.add_argument("--ao", type=float, default=0.5, help="cavity darkening multiplied into the colour (0 = off)")
    p.add_argument("--normal", action=argparse.BooleanOptionalAction, default=True,
                   help="bake a tangent-space normal map from the dense mesh")
    p.add_argument("--collar", default="none", choices=["none", "generated"],
                   help="add the collar node, cut from the generated bust named by --bust")
    p.add_argument("--bust", help="the bust candidate (name under <work>/cand) the generated collar is cut from")
    p.add_argument("--collar-budget", type=int, default=20000, help="triangles of the collar")
    p.add_argument("--jaw-open", type=float, default=0.35, help="the jaw's largest opening, rad")

    a = ap.parse_args(argv)
    if a.check:
        return check_main(a)
    if a.cmd == "prep":
        from .prep import prep

        prep(a.rig, a.portrait, a.work)
        return 0
    if a.cmd == "gen":
        from .generate import generate

        return generate(a.model, a.work, a.input, a.seed, a.weights, a.steps)
    if a.cmd == "turntable":
        from .review import turntable

        turntable(a.work / "cand" / a.cand, a.runs)
        return 0
    if a.cmd == "sheet":
        from .review import candidate_sheet

        labels = a.label or [g.parent.name for g in a.glb]
        print(candidate_sheet(list(zip(labels, a.glb)), a.out))
        return 0
    if a.cmd == "build":
        from .build import build

        return build(a)
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
