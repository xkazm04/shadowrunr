# shadowrunr-avatar3d: Malakor's 3D head, built offline

Turns the owner's M8 portrait into `lich.glb` + `lich.json`: a textured skull with a separate,
hinged jaw and four anchors (eyes, monocle, neck). The stage's `lich3d` face draws it with
three.js. **This is offline work.** An image-to-3D model runs once on the GPU; the stream never
runs it.

**The portrait and everything made from it belong to the owner. Never commit them.** That covers
the prepped inputs, the candidate meshes, the GLB and the renders. They live under `data/` and
`runs/`, which are gitignored. The repo ships no model code and no weights, and nothing in this
package downloads them.

## Install (the operator's machine)

The models live outside the repo, in the same place as the LaMa and Depth-Anything models:

```
uv venv --python 3.11 ~/.shadowrunr/venvs/avatar3d
PY=~/.shadowrunr/venvs/avatar3d/Scripts/python.exe        # bin/python off Windows
uv pip install --python $PY torch==2.4.0 torchvision==0.19.0 --index-url https://download.pytorch.org/whl/cu121
uv pip install --python $PY xformers==0.0.27.post2 --index-url https://download.pytorch.org/whl/cu121
uv pip install --python $PY spconv-cu120
uv pip install --python $PY kaolin==0.17.0 -f https://nvidia-kaolin.s3.us-east-2.amazonaws.com/torch-2.4.0_cu121.html
uv pip install --python $PY "numpy<2" "transformers<4.47" open3d imageio tqdm easydict opencv-python-headless \
    scipy ninja trimesh xatlas safetensors huggingface_hub moderngl fast-simplification rembg onnxruntime \
    omegaconf einops scikit-image \
    "git+https://github.com/EasternJournalist/utils3d.git@9a4eb15e4021b67b12c460c7057d642626897ec8"
git clone --recurse-submodules https://github.com/microsoft/TRELLIS ~/.shadowrunr/src/TRELLIS
huggingface-cli download microsoft/TRELLIS-image-large --local-dir ~/.shadowrunr/models/TRELLIS-image-large
# optional comparison model
git clone https://github.com/VAST-AI-Research/TripoSR ~/.shadowrunr/src/TripoSR
huggingface-cli download stabilityai/TripoSR --local-dir ~/.shadowrunr/models/TripoSR
```

On the first run, TRELLIS fetches DINOv2 (`facebookresearch/dinov2`, Apache-2.0) through
`torch.hub`. Set `TORCH_HOME=~/.shadowrunr/models/torch-hub` to keep that download with the other
models. Other overrides:

- `SHADOWRUNR_TRELLIS`, `SHADOWRUNR_TRIPOSR`, `SHADOWRUNR_SF3D`: where the source checkouts are.
- `SHADOWRUNR_MODELS`: where the weights are.

This setup works on Windows with an RTX 4090, Python 3.11 and CUDA 12.1 wheels. It compiles
nothing:

- **Attention:** xformers (`ATTN_BACKEND=xformers`, set by the runner), not flash-attn.
- **Sparse convolution:** spconv's prebuilt wheel.
- **kaolin:** a prebuilt Windows wheel. TRELLIS imports it only for one tensor check in
  FlexiCubes.

**On purpose, the setup does not install nvdiffrast or diff-gaussian-rasterization.** TRELLIS
uses them only in its own GLB export, and both carry non-commercial licences (see below). This
package exports and bakes the mesh itself, with permissive tools.

### TRELLIS.2 (the newer model): not installed, and why

`microsoft/TRELLIS.2-4B` and its code are MIT, but the pipeline pulls two weights that cannot be
fetched without an account:

- `facebook/dinov3-vitl16-pretrain-lvd1689m` (the image encoder): gated on Hugging Face with
  manual approval, under the DINOv3 licence. Anonymous download answers HTTP 401.
- `briaai/RMBG-2.0` (background removal): gated, and BRIA's licence is non-commercial. The MIT
  `ZhengPeng7/BiRefNet` is the same architecture and could replace it in `pipeline.json`.

Its CUDA parts (cumesh, flexgemm, o-voxel, flash-attn or an SDPA backend) also need a Visual
Studio build, and its own rendering uses nvdiffrast, which this package avoids. The attempt of
Oct 2026 stopped at the gate, before any install; see `runs/lich3d-quality/g/trellis2-install.md`.
To take the route: accept the DINOv3 licence with the account that will hold the token, download
the two weights, point `pipeline.json` at the MIT BiRefNet, build the three extensions from the
x64 Native Tools prompt, and add a `trellis2` runner to `generate.py` (a runner is a function
from an image path and a seed to vertices, faces and colours).

The shipped asset uses TRELLIS v1 with both samplers' steps raised to 40 (`gen --steps 40`).

## Licences (audited Oct 2026)

| Component | Code | Weights | Training data | Used here |
| --- | --- | --- | --- | --- |
| TRELLIS (`microsoft/TRELLIS`) | MIT | `TRELLIS-image-large` MIT | TRELLIS-500K: Objaverse(XL) (Sketchfab and GitHub objects, per-object licences, mostly CC), ABO (CC BY-NC 4.0), 3D-FUTURE (its own non-commercial terms), HSSD (CC BY-NC 4.0), Toys4k | **yes**: the shipped asset |
| DINOv2 (image encoder inside TRELLIS) | Apache-2.0 | Apache-2.0 | LVD-142M | yes |
| FlexiCubes (TRELLIS submodule) | Apache-2.0 | - | - | yes |
| kaolin, spconv, xformers | Apache-2.0, Apache-2.0, BSD-3 | - | - | yes |
| TripoSR | MIT | MIT | curated Objaverse subset (CC-BY) | comparison only |
| Stable Fast 3D | Stability AI Community Licence: free under US$1M annual revenue; commercial use requires registering with Stability; "Powered by Stability AI" attribution and a Notice file when distributed | gated on Hugging Face | Objaverse | not run (gated) |
| TRELLIS.2 (`microsoft/TRELLIS.2-4B`) | MIT | MIT | not stated on the card | **not used**: needs DINOv3 (gated, manual approval, DINOv3 licence) and RMBG-2.0 (gated, non-commercial) |
| DINOv3 (`facebook/dinov3-vitl16-pretrain-lvd1689m`) | DINOv3 licence | gated, manual approval | LVD-1689M | not used |
| BRIA RMBG-2.0 | - | BRIA non-commercial licence, gated | licensed data | not used |
| scipy, Pillow, moderngl, xatlas, fast-simplification | BSD-3, HPND, MIT, MIT, MIT | - | - | yes (offline build only; nothing ships in the GLB) |
| Hunyuan3D-2 | - | - | - | excluded: its licence carves out territories |
| nvdiffrast / diff-gaussian-rasterization | NVIDIA Source Code Licence (non-commercial) / Inria (non-commercial research) | - | - | **avoided** |

**The training-data flag.** TRELLIS's weights are MIT, but some of its training subsets (ABO,
HSSD, 3D-FUTURE) are non-commercial. Whether a model's licence covers its outputs when its
training data was NC is unsettled. Our use is narrow: the asset comes from the owner's original
portrait, and the model only lifts it to 3D. The owner should still know about it.

## Run it

From `tools/avatar3d`, with `$PY` the venv above and `D=<repo>/data/avatar`:

```
python -m shadowrunr_avatar3d prep --rig $D/rig --portrait $D/portraits/m8/portrait.png --work $D/lich3d-work
$PY -m shadowrunr_avatar3d gen --model trellis --work $D/lich3d-work --input clean2 --seed 1 2 3 4 5 6 7 8 --steps 40
$PY -m shadowrunr_avatar3d gen --model trellis --work $D/lich3d-work --input bust --seed 1 2 3 4 --steps 40
$PY -m shadowrunr_avatar3d turntable --work $D/lich3d-work --cand trellis-clean2-s4-st40 --runs <repo>/runs/lich3d-quality/g/turn
$PY -m shadowrunr_avatar3d build --work $D/lich3d-work --cand trellis-clean2-s4-st40 --rig $D/rig \
    --spec ../../characters/malakor/rig/m8.spec.json --portrait $D/portraits/m8/portrait.png \
    --tex 4096 --collar generated --bust trellis-bust-s3-st40 \
    --out $D/lich3d-out/A --runs <repo>/runs/lich3d-quality/g/cand-A
$PY -m shadowrunr_avatar3d sheet --glb $D/rig/lich.glb $D/lich3d-out/A/lich.glb --label control A --out candidates.png
python -m shadowrunr_avatar3d --check $D/rig/lich.glb
```

What each step does:

- **`prep`** composes the skull from the 2D rig's `head` and `jaw` layers. Those layers already
  have the crystal inpainted away, the eye fire keyed out and the monocle ring lifted, and their
  alpha is the matte. `prep` pads the skull square and writes these inputs:
  - `input_clean.png`: the rig's head and jaw layers.
  - `input_clean2.png`: the same with the monocle ring's orange smear taken out of the bone
    around the image-right socket (`prep.desmear`: pixels whose tint leans far from the bone's
    are recoloured to bone, keeping their strokes and the shadows). **Use this one.** The rig's
    own files are left as they are; `build` applies the same repair to the layers it paints from.
  - `input_bust.png` and `bust.json`: the clean skull over the rig's `torso` layer (collar,
    cloak, shoulders), for the collar.
  - `input_portrait.png`: the portrait's own pixels under the same matte.

  `clean` gives the better skull. With `portrait`, the eye fire and the ring get baked into the
  geometry, and the crystal's tip cuts a slit in the brow.
- **`gen`** runs one model per seed and writes `cand/<model>-<input>-s<seed>[-st<steps>]/` holding
  `mesh.npz` and `meta.json`. The meta records wall time, the device's VRAM peak (nvidia-smi
  sampled every 0.25 s) and torch's reserved peak.
- **`turntable`** first fits the candidate's orientation. It tries the 24 axis rotations, scores
  each one's silhouette and luminance against the input, then refines by a few degrees. It
  renders eight views 45 degrees apart.
- **`build`** makes the asset. See `shadowrunr_avatar3d/build.py` for each step:
  - fits a 20-degree camera to the portrait's skull matte;
  - cuts the jaw along the rig's jaw polygon, plus the mandible's rami up to the condyles,
    keeping only faces in front of the hinge plane;
  - caps both pieces;
  - places the anchors;
  - decimates to `--budget` triangles (80k by default, skull and jaw together);
  - unwraps one xatlas atlas and bakes a `--tex` texture (1024, 2048 or 4096; 2048 by default),
    stored as JPEG q92 (`--tex-format png` for lossless). The bake takes the portrait's clean
    layers wherever the camera saw the surface and near-black on the caps. **Where the camera saw
    nothing** (the cranium's sides and back) it paints bone cut from the front (`--unseen synth`,
    `tex.py`): two swatches of lit cranium become a multiplicative detail map (cracks, pits, grain
    at the front's scale), looked up in object space by a triplanar projection with a
    variance-preserving mix of decorrelated copies, times the portrait's mean bone colour. The
    model's own colour stays only where it reads as a dark cavity (the sockets' walls).
    `--unseen model` is the old behaviour;
  - bakes a **tangent-space normal map** from the dense mesh (about a million triangles) onto the
    decimated one (`--no-normal` to skip), in glTF's frame: +X along `u`, +Y toward image-up, the
    frame a renderer rebuilds from the UVs when a mesh has no tangents. The material gets a
    standard `normalTexture`. It also multiplies a **cavity** term from the dense mesh's curvature
    into the base colour (`--ao 0.5`, 0 = off; half strength where the portrait painted);
  - with `--collar generated --bust <cand>`, adds the **collar** node (below);
  - exports, checks, and writes `front_vs_portrait.png`, `jaw_closed_open.png`,
    `jaw_split.png`, `anchors.png`, `final_turntable.png` and `atlas.png`.

  `--out` writes somewhere other than `--rig`, for trying a candidate without replacing the
  live asset.

- **`sheet`** renders ten views (yaw -35, +35, 0, 45 ... 315) of each GLB given, one row per
  asset, from the GLBs themselves with the normal maps applied: the control and the candidates
  side by side.

### The collar

`gen --input bust` makes a mesh of skull, collar and cloak from `input_bust.png`. `build --collar
generated --bust <cand>` orients it, fits the portrait's camera to its matte, and maps it into
the skull's frame (same field of view, so a uniform scale about the camera targets). It then drops
the bust's own skull, by colour (pale or grey faces) and by place (within 0.045 of the final
skull's surface), keeps the large connected pieces, closes the rims with dark caps, decimates to
`--collar-budget` (20k), unwraps a 2048 atlas and paints it from the portrait wherever the skull's
camera sees the surface (not hidden by the skull), the bust's own colour elsewhere. The shipped
collars are generated: the two flared wings with their gold trim and the cloak came out clean. A
modeled collar (wings and a slab extruded from the portrait's silhouette) was the planned
fallback; it was not needed, so it is not in the code.

The `--check` command needs numpy and nothing else. It exits 0 when the GLB and `lich.json`
hold the contract, 1 when they do not.

## The GLB contract

The authoritative text is the docstring of `shadowrunr_avatar3d/contract.py`. In short:

- Scene 0 has six **root** nodes, named exactly `skull`, `jaw`, `eye_l`, `eye_r`, `monocle` and
  `neck`. Each carries a translation only: no rotation, scale or children.
- The model faces **+Z**, with **+Y** up. Skull plus closed jaw is **1 unit tall**, centred on its
  bounding box.
- `skull` is a mesh at the origin.
- `jaw` is a mesh whose node translation is the **hinge**. Its vertices are relative to the
  hinge. A rotation of **+θ about the node's local +X opens the mouth**, up to
  `lich.json.jaw_open_rad`. Both pieces are closed, so the open mouth shows dark caps, not holes.
- `eye_l` and `eye_r` sit at the socket centres, slightly inside the bone. `_l` and `_r` are
  the **image's** left and right, as in the 2D rig, so `eye_l` has the smaller X.
- `monocle` sits at the ring's centre on `eye_r`, at the rim.
- `neck` is the head's pivot, under the skull.
- `skull` and `jaw` share one atlas (PNG or JPEG) with an optional tangent-space `normalTexture`
  (a `build` without `--no-normal` writes one); metallic 0, roughness 1. The collar has its own
  small atlas and no normal map.
- `collar` (optional): the high collar and shoulders as a mesh whose node translation is `neck` and
  whose vertices are relative to it. Absent means no node at all.
- `lich.json` (version 1):

  ```
  {"version": 1, "source_model": "trellis:clean:seed1", "portrait_sha256": "<hex>", "tris": <n>,
   "jaw_open_rad": 0.35, "eye_radius": <socket's equivalent radius, scene units>,
   "camera": {"fov": <vertical deg>, "pos": [x, y, z], "target": [x, y, z]}}
  ```

  `camera` is a square view (aspect 1). Rendered through it, the head lands where the portrait
  has it in a 1024 frame.

## Tests

```
python -m unittest
```

Run this from `tools/avatar3d`. The tests need numpy and the standard library, and nothing
else: no model, no GPU, nothing from `data/`. They cover:

- the GLB writer and reader on drawn boxes;
- the contract check: missing or extra nodes, a pivot at the chin or off centre, swapped eyes,
  the size and triangle budgets, `lich.json` fields and nulls, and the one-unit height;
- the jaw cut on a drawn sphere: both pieces closed and outward-facing, and their volumes adding
  up to the whole.
