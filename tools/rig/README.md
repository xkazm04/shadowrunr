# shadowrunr-rig: Malakor's face rig, split offline

Splits the owner's portrait into the layers the stage's face renderer draws: a `layers.json`
(the glow rig's `FormSpec`, checked as the stage's `checkSpec` checks it) and one PNG per layer.
The daemon serves them from `<data>/avatar/rig/` at `GET /avatar/{file}`.

The split is driven by a hand-measured **region spec** per portrait. For Malakor's M8 portrait
it is `characters/malakor/rig/m8.spec.json`. The spec holds polygons and points in source
pixels, never image data.

**The portrait and every layer cut from it are the owner's files. Never commit them.** Both live
under `data/`, which is gitignored. The LaMa weights are the operator's install: nothing here
downloads them, and the repo ships none.

## Run it

You need a Python with numpy and Pillow, plus torch for inpainting:
`pip install ./tools/rig[lama]`. You also need big-lama as TorchScript (`big-lama.pt`,
Apache-2.0). From `tools/rig`:

```
python -m shadowrunr_rig \
  --portrait <data>/avatar/portraits/m8/portrait.png \
  --spec ../../characters/malakor/rig/m8.spec.json \
  --out <data>/avatar/rig \
  --lama <path to big-lama.pt>
```

- `--lama` can also come from `SHADOWRUNR_LAMA`. It is needed only when an element is filled by
  inpainting.
- `--device cpu` runs without a GPU. The default is `cuda`, which falls back to the CPU when
  there is none.
- `--debug <dir>` also writes the region overlay, the background, the recomposed rest frame and
  its error against the portrait. Point it outside the repo.

The run publishes the new rig through `shadowrunr_rig.rigdir`, the rig folder's one owner. It
clears only the files the old `layers.json` named (anything else in `--out`, such as notes or the
depth prep's `learned.*`, is left alone), writes the PNGs, and writes `layers.json` last. The depth
prep (`tools/avatar/depth_map.py`) writes `depth.png` and `depth.json` into the same folder and
records the sha256 of every rig file it read. When the new rig is not byte for byte the one the
depth map was made from, the run removes `depth.png` and `depth.json` together and prints
`depth map is stale: run tools/avatar/depth_map.py <dir>`; until then the stage draws the glow
face. `python tools/avatar/depth_map.py <dir> --check` says whether a depth map is fresh (exit 0),
stale (3) or missing (2).

The run prints how closely the rest frame recomposes the portrait (mean, p99 and max error,
0..255), composed by `rigdir.compose_rest`: the frame the stage's glow shader draws, and the one
the depth prep shows its depth model. M8 recomposes at mean 0.14 and
p99 2.8. Reload the stage to see the new rig: `/avatar/` responses are not cached.

## Judge a rig before the stage shows it

The folder you pass as `--out` is the live rig. While tuning the spec, stage the new rig beside it
instead, judge it, then move it in:

```
python -m shadowrunr_rig --portrait ... --spec ... --out <data>/avatar/rig --candidate
python -m shadowrunr_rig promote --out <data>/avatar/rig            # refuses a regression
python -m shadowrunr_rig promote --out <data>/avatar/rig --accept   # takes it anyway
python -m shadowrunr_rig promote --out <data>/avatar/rig --revert   # the previous rig, back
```

- `--candidate` writes the rig into `<out>.next` and leaves `<out>` untouched.
- Every run writes two files beside its rig. The stage ignores them: it reads only what
  `layers.json` names.
  - `report.json` has one row per element: its bbox, `coverage_px`, `filled_px` (pixels its fill
    invented) and the rest-frame error on the pixels it owns.
  - `extremes.png` shows the rest frame, the jaw dropped by its `uncover`, and the floats at
    `+amp`, side by side. These are the pixels a viewer sees when the rig moves.
- A pixel's owner is the frontmost solid drawn there. When what that solid shows was painted in
  by another element's fill, the owner is that element instead. So a polygon pushed off its
  feature shows up as that element's p99, not the head's.
- The run prints the table, worst first. A candidate run also prints each element against the
  live `report.json`.
- The rest frame cannot see everything. A misplaced light, or a cut whose fill leaks nothing,
  still recomposes the portrait exactly. For those, look at `extremes.png`.
- `promote` refuses, and changes nothing, when an element's p99 rose by more than `--tol`
  (default 4) against the live report. It names each such element as `worse`, with before -> after.
- When `promote` goes through:
  - the live rig, its depth pair and its report are kept in `<out>.prev`, written whole before the
    live folder is touched;
  - the candidate's files move in byte for byte through `rigdir.publish_files`, `layers.json` last.
- `promote --revert` makes the live folder byte-identical to what it was before the promote. It
  refuses if the live rig has changed since then, because reverting would lose that rig.
- `--debug`'s `regions.png` colours each element by its name, so the same name gets the same
  colour in every run. A legend strip under the overlay names every colour.

## How it splits

The spec lists the portrait's elements front to back. Each element is lifted off a working copy
in that order, and the copy is filled in behind it:

| `mode` | The layer | What stays behind (`fill`) |
| --- | --- | --- |
| `light` | The light alone, drawn additively: the copy minus an inpainted base (`inpaint`), or the part that leans to a hue over a dark base (`key`). | The copy without the light. |
| `cut` | The copy's own pixels, with the region's soft coverage as alpha. | LaMa (`inpaint`), only inside `inpaint_within` if given; the ground colour (`ground`); or the pixels as they are (`keep`). |
| `none` | No pixels: the renderer draws it (the mouth light). | Nothing. |

What is left of the copy becomes the static `background` layer.

**Regions.** A region is one of:
- `poly`: a polygon;
- `ring` (`[cx, cy, r]` with `width`): refined to the brightest circle nearby;
- `rest`: whatever is lit and not yet taken.

Optional refinements:
- `lit` keeps only pixels brighter than a threshold, after closing small gaps;
- `holes` fills enclosed holes;
- `also` and `minus` add or remove polygons after that;
- `grow` and `feather` soften the edge.

**A hinged jaw.**
- `interior` paints the mouth's inside where the jaw will open.
- `uncover: N` keeps, on the layers behind, only the strip the jaw uncovers when it drops by up
  to N px. What the jaw always covers is left out of the skull and the background.

**`depth: "surface"`** gives a layer the head sphere's depth at its anchor, or at `depth_at`, so
it turns with the bone under it. The other renderer fields (`gain`, `bloom`, `blink`, `float`,
`pulse`, `follow`, `occl`, ...) pass through to `layers.json` unchanged.

## Tests

```
python -m unittest
```

Run this from `tools/rig`. The spec and `layers.json` checks need only the standard library. The
masks and the split need numpy and use a stand-in inpainter, so no model and no torch.
