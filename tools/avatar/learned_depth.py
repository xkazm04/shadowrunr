# SPDX-License-Identifier: Apache-2.0
"""The model step of the `learned-v1` depth map: Depth-Anything-V2-Small over the rig's portrait.

    <python with onnxruntime> tools/avatar/learned_depth.py <rig dir> [--model <model.onnx>]

`depth_map.py --method learned-v1` writes the model's input beside the rig (`learned.in.npy`,
the composited portrait resized and normalised, 1x3x518x518 float32). This script runs the model
on it and writes, beside it:

- `learned.out.npy`: the model's relative inverse depth (518x518 float32, nearer is larger);
- `learned.json`: the model's id, revision, file and sha256, and the sha256 of the input it read.

It needs numpy and onnxruntime only (no Pillow), so it runs in the voice venv; `depth_map.py`
needs numpy and Pillow only. The split keeps either from needing a new package.

**The model.** `onnx-community/depth-anything-v2-small` (the Small model's ONNX export,
Apache-2.0; the Base, Large and Giant models are CC-BY-NC and are not used), pinned below by
revision and sha256. Nothing downloads it: put `onnx/model.onnx` at
`~/.shadowrunr/models/depth-anything-v2-small/model.onnx`, or pass `--model` (or set
`SHADOWRUNR_DEPTH_MODEL`). A file with another sha256 is refused.

**Reproducible.** The CPU provider with the threads pinned, sequential execution and a fixed
optimisation level: the same model file and input give the same bytes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
from pathlib import Path

import numpy as np

MODEL_ID = "onnx-community/depth-anything-v2-small"
MODEL_REVISION = "4472b7362082ad9968fee890ca0f1e5aca36b93d"
MODEL_FILE = "onnx/model.onnx"
MODEL_SHA256 = "afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c"
MODEL_LICENSE = "apache-2.0"
THREADS = 4

IN_FILE = "learned.in.npy"
OUT_FILE = "learned.out.npy"
META_FILE = "learned.json"


def default_model() -> Path:
    env = os.environ.get("SHADOWRUNR_DEPTH_MODEL")
    if env:
        return Path(env)
    return Path.home() / ".shadowrunr" / "models" / "depth-anything-v2-small" / "model.onnx"


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_model(path: Path) -> str:
    """The model file's sha256, or ValueError when it is not the pinned file."""
    if not path.is_file():
        raise ValueError(f"no model at {path} (see the module docstring for where it goes)")
    digest = sha256_file(path)
    if digest != MODEL_SHA256:
        raise ValueError(f"{path} has sha256 {digest}, not the pinned {MODEL_ID}@{MODEL_REVISION[:7]} {MODEL_FILE}")
    return digest


def infer(model: Path, pixels: np.ndarray, threads: int = THREADS) -> np.ndarray:
    """Run the model on `pixels` (1x3xHxW float32); returns its HxW output as float32."""
    import onnxruntime as ort  # only here, so the rest of the module loads without it

    opts = ort.SessionOptions()
    opts.intra_op_num_threads = threads
    opts.inter_op_num_threads = 1
    opts.execution_mode = ort.ExecutionMode.ORT_SEQUENTIAL
    opts.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    opts.log_severity_level = 3
    session = ort.InferenceSession(str(model), sess_options=opts, providers=["CPUExecutionProvider"])
    name = session.get_inputs()[0].name
    out = session.run(None, {name: np.ascontiguousarray(pixels, dtype=np.float32)})[0]
    return np.ascontiguousarray(np.asarray(out, dtype=np.float32).reshape(out.shape[-2:]))


def run(folder: Path, model: Path, infer_fn=infer, check_fn=check_model) -> dict:
    """Read `learned.in.npy`, run the model, write `learned.out.npy` and `learned.json`. (Tests
    pass a fake `infer_fn` and `check_fn`; nothing else does.)"""
    src = folder / IN_FILE
    if not src.is_file():
        raise ValueError(f"no {IN_FILE} in {folder}: run depth_map.py --method learned-v1 on it first")
    raw = src.read_bytes()
    pixels = np.load(src, allow_pickle=False)
    if pixels.dtype != np.float32 or pixels.ndim != 4 or pixels.shape[:2] != (1, 3):
        raise ValueError(f"{IN_FILE} is {pixels.dtype} {pixels.shape}, not 1x3xHxW float32")
    digest = check_fn(model)
    depth = infer_fn(model, pixels)
    if depth.shape != pixels.shape[2:] or not np.isfinite(depth).all():
        raise ValueError(f"the model returned {depth.shape}, not a finite {pixels.shape[2:]} map")
    np.save(folder / OUT_FILE, depth.astype(np.float32), allow_pickle=False)
    meta = {
        "model": {
            "id": MODEL_ID,
            "revision": MODEL_REVISION,
            "file": MODEL_FILE,
            "sha256": digest,
            "license": MODEL_LICENSE,
        },
        "input_sha256": hashlib.sha256(raw).hexdigest(),
        "output_sha256": hashlib.sha256((folder / OUT_FILE).read_bytes()).hexdigest(),
    }
    (folder / META_FILE).write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return meta


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Run Depth-Anything-V2-Small on a rig's learned.in.npy.")
    ap.add_argument("rig", type=Path, help="the rig folder (holds learned.in.npy)")
    ap.add_argument("--model", type=Path, default=None, help="the model file (default: ~/.shadowrunr/models/...)")
    args = ap.parse_args(argv)
    started = time.perf_counter()
    try:
        run(args.rig, args.model or default_model())
    except (OSError, ValueError) as error:
        print(f"learned_depth: {error}", file=sys.stderr)
        return 1
    print(f"learned_depth: {args.rig / OUT_FILE} in {time.perf_counter() - started:.2f} s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
