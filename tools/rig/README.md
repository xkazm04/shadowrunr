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

The run removes the old PNGs in `--out`, then writes the new rig. It prints how closely the rest
frame recomposes the portrait (mean, p99 and max error, 0..255). M8 recomposes at mean 0.14 and
p99 2.8. Reload the stage to see the new rig: `/avatar/` responses are not cached.

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
