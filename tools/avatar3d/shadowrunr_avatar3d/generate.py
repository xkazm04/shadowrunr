# SPDX-License-Identifier: Apache-2.0
"""Run one image-to-3D model on the prepped skull, once per seed, and keep each raw candidate.

Each candidate lands in ``<work>/cand/<model>-<input>-s<seed>/`` as ``mesh.npz`` (vertices,
triangles and per-vertex colour in the model's own frame) with ``meta.json``: the model, seed,
wall time and peak VRAM. Orientation, scale and everything else is :mod:`.build`'s job, so every
backend only has to hand back a coloured triangle soup.

The models are the operator's install (see the README): their code is found through
``SHADOWRUNR_TRELLIS`` / ``SHADOWRUNR_TRIPOSR`` / ``SHADOWRUNR_SF3D`` (a source checkout) and
their weights through ``--weights`` or ``SHADOWRUNR_MODELS``. Nothing here downloads a model.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

import numpy as np

HOME = Path(os.environ.get("SHADOWRUNR_HOME", Path.home() / ".shadowrunr"))
MODELS = Path(os.environ.get("SHADOWRUNR_MODELS", HOME / "models"))
SRC = HOME / "src"


class VramPeak:
    """The GPU's used memory sampled with nvidia-smi, minus what was in use before the run.

    torch's own counter misses memory a model allocates outside its caching allocator, so the
    whole-device sample is the honest number; it includes other processes' growth during a run."""

    def __init__(self, every: float = 0.25) -> None:
        self.every = every
        self.base = self._used()
        self.peak = self.base
        self._stop = threading.Event()
        self._t = threading.Thread(target=self._run, daemon=True)

    @staticmethod
    def _used() -> int:
        try:
            out = subprocess.run(
                ["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
                capture_output=True, text=True, timeout=5,
            ).stdout
            return int(out.split()[0])
        except (OSError, ValueError, IndexError, subprocess.TimeoutExpired):
            return 0

    def _run(self) -> None:
        while not self._stop.wait(self.every):
            self.peak = max(self.peak, self._used())

    def __enter__(self) -> "VramPeak":
        self._t.start()
        return self

    def __exit__(self, *exc) -> None:
        self._stop.set()
        self._t.join()

    @property
    def mib(self) -> int:
        return self.peak - self.base


def _src(env: str, default: str) -> Path:
    p = Path(os.environ.get(env, SRC / default))
    if not p.is_dir():
        raise SystemExit(f"{env}: no checkout at {p} (see tools/avatar3d/README.md)")
    if str(p) not in sys.path:
        sys.path.insert(0, str(p))
    return p


# --- TRELLIS (microsoft/TRELLIS, MIT; weights microsoft/TRELLIS-image-large, MIT) ---------------


def trellis_runner(weights: Path | None):
    os.environ.setdefault("ATTN_BACKEND", "xformers")
    os.environ.setdefault("SPCONV_ALGO", "native")
    _src("SHADOWRUNR_TRELLIS", "TRELLIS")
    from PIL import Image
    from trellis.pipelines import TrellisImageTo3DPipeline

    pipe = TrellisImageTo3DPipeline.from_pretrained(str(weights or MODELS / "TRELLIS-image-large"))
    pipe.cuda()

    def run(image_path: Path, seed: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        out = pipe.run(
            Image.open(image_path),
            seed=seed,
            formats=["mesh"],
            sparse_structure_sampler_params={"steps": 25, "cfg_strength": 7.5},
            slat_sampler_params={"steps": 25, "cfg_strength": 3.0},
        )
        m = out["mesh"][0]
        v = m.vertices.detach().float().cpu().numpy()
        f = m.faces.detach().long().cpu().numpy()
        c = m.vertex_attrs[:, :3].detach().float().clamp(0, 1).cpu().numpy()
        return v, f, c

    return run


# --- TripoSR (VAST-AI-Research/TripoSR, MIT; weights stabilityai/TripoSR, MIT) ------------------


def _torchmcubes_shim() -> None:
    """TripoSR imports ``torchmcubes``, whose CMake build does not configure on Windows. Its one
    function is marching cubes, which scikit-image (BSD) has: same contract, (x, y, z) vertex
    order with x along the volume's last axis."""
    import types

    import torch

    try:
        import torchmcubes  # noqa: F401

        return
    except ImportError:
        pass
    from skimage.measure import marching_cubes as mc

    def marching_cubes(vol, level):
        verts, faces, _, _ = mc(vol.detach().float().cpu().numpy(), level)
        v = torch.from_numpy(verts[:, [2, 1, 0]].copy()).float().to(vol.device)
        return v, torch.from_numpy(faces.astype("int64")).to(vol.device)

    sys.modules["torchmcubes"] = types.SimpleNamespace(marching_cubes=marching_cubes)


def triposr_runner(weights: Path | None):
    import torch

    _torchmcubes_shim()
    _src("SHADOWRUNR_TRIPOSR", "TripoSR")
    from PIL import Image
    from tsr.system import TSR

    root = weights or MODELS / "TripoSR"
    model = TSR.from_pretrained(str(root), config_name="config.yaml", weight_name="model.ckpt")
    model.renderer.set_chunk_size(8192)
    model.to("cuda")

    def run(image_path: Path, seed: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        torch.manual_seed(seed)  # TripoSR is deterministic; the seed is recorded, not used
        rgba = np.asarray(Image.open(image_path).convert("RGBA")).astype(np.float32) / 255.0
        # TripoSR wants the object on 50% grey, filling ~85% of the frame.
        rgb = rgba[..., :3] * rgba[..., 3:] + 0.5 * (1 - rgba[..., 3:])
        img = Image.fromarray((rgb * 255).astype(np.uint8))
        with torch.no_grad():
            codes = model([img], device="cuda")
            mesh = model.extract_mesh(codes, True, resolution=320)[0]
        c = np.asarray(mesh.visual.vertex_colors[:, :3], np.float32) / 255.0
        return np.asarray(mesh.vertices, np.float32), np.asarray(mesh.faces), c

    return run


# --- Stable Fast 3D (Stability-AI/stable-fast-3d; Stability AI Community Licence) ---------------


def sf3d_runner(weights: Path | None):
    import torch

    _src("SHADOWRUNR_SF3D", "stable-fast-3d")
    from PIL import Image
    from sf3d.system import SF3D

    root = weights or MODELS / "stable-fast-3d"
    model = SF3D.from_pretrained(str(root), config_name="config.yaml", weight_name="model.safetensors")
    model.to("cuda").eval()

    def run(image_path: Path, seed: int) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
        torch.manual_seed(seed)
        img = Image.open(image_path).convert("RGBA")
        with torch.no_grad(), torch.autocast(device_type="cuda", dtype=torch.bfloat16):
            mesh, _ = model.run_image(img, bake_resolution=1024, remesh="none")
        # SF3D bakes a UV texture; sample it at the vertices so every backend hands back the same.
        uv = np.asarray(mesh.visual.uv, np.float32)
        tex = np.asarray(mesh.visual.material.baseColorTexture.convert("RGB"), np.float32) / 255.0
        h, w = tex.shape[:2]
        px = np.clip((uv[:, 0] * (w - 1)).round().astype(int), 0, w - 1)
        py = np.clip(((1 - uv[:, 1]) * (h - 1)).round().astype(int), 0, h - 1)
        return np.asarray(mesh.vertices, np.float32), np.asarray(mesh.faces), tex[py, px]

    return run


RUNNERS = {"trellis": trellis_runner, "triposr": triposr_runner, "sf3d": sf3d_runner}


def generate(model: str, work: Path, which: str, seeds: list[int], weights: Path | None) -> int:
    import torch

    image = work / f"input_{which}.png"
    if not image.is_file():
        raise SystemExit(f"{image} is missing: run prep first")
    t0 = time.time()
    with VramPeak() as load_peak:
        run = RUNNERS[model](weights)
    load_s = time.time() - t0
    print(f"{model}: loaded in {load_s:.1f}s ({load_peak.mib} MiB)")
    for seed in seeds:
        out = work / "cand" / f"{model}-{which}-s{seed}"
        out.mkdir(parents=True, exist_ok=True)
        torch.cuda.empty_cache()  # so each seed's device peak is its own, not the cache's growth
        torch.cuda.reset_peak_memory_stats()
        t = time.time()
        with VramPeak() as peak:
            v, f, c = run(image, seed)
            torch.cuda.synchronize()
        wall = time.time() - t
        np.savez_compressed(out / "mesh.npz", v=v.astype(np.float32), f=f.astype(np.int32), c=c.astype(np.float32))
        meta = {
            "model": model,
            "input": which,
            "seed": seed,
            "load_s": round(load_s, 1),
            "wall_s": round(wall, 1),
            # The device's peak during the run, and what other processes held before the load.
            "peak_vram_mib": peak.peak,
            "device_base_mib": load_peak.base,
            "torch_peak_mib": int(torch.cuda.max_memory_reserved() / 2**20),
            "verts": int(v.shape[0]),
            "tris": int(f.shape[0]),
        }
        (out / "meta.json").write_text(json.dumps(meta, indent=1), encoding="utf-8")
        print(f"{out.name}: {meta['tris']} tris in {wall:.1f}s, device peak {peak.peak} MiB "
              f"(torch {meta['torch_peak_mib']} MiB)")
    return 0
