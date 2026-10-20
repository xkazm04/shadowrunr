# SPDX-License-Identifier: Apache-2.0
"""Split a portrait into the glow rig's layers, from a hand-measured region spec.

Ported from athena's ``split_glow.py``. The portrait is read front to back: each element of the
spec is lifted off a working copy, and what lay behind it is filled in, so the next element (and
in the end the static background) continues under it:

- ``mode: "light"``: the element is light added over what is behind it. Its layer is the
  difference between the working copy and the fill (``fill: "inpaint"``), or the part of the
  working copy that leans to a hue over a dark base (``fill: "key"``); the working copy keeps the
  rest. Drawn additively, the two sum back to the portrait.
- ``mode: "cut"``: the element is a solid. Its layer is the working copy's own pixels with its
  soft coverage as alpha; behind it the working copy is inpainted (``"inpaint"``), set to the
  ground colour (``"ground"``) or left as it is (``"keep"``), and an ``interior`` (a mouth) may be
  painted in where the element will open.
- ``mode: "none"``: drawn procedurally by the renderer; no pixels.

What is left of the working copy is the ``background`` layer. :func:`split` is numpy only; the
inpainter is passed in, so tests run it with a stand-in and no model.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any

import numpy as np

from . import formspec as fsm
from . import masks as mk
from .lama import fill_crop

Fill = Callable[[np.ndarray, np.ndarray], np.ndarray]

#: A light layer fades out over the outer 12 % of its box in the renderer: pad it past that.
LIGHT_PAD = 0.17
MIN_ALPHA = 3


@dataclass
class Lifted:
    """One element as lifted: its spec, its full-canvas RGBA (0..255 float) or None, and meta."""

    el: dict
    rgba: np.ndarray | None
    meta: dict = field(default_factory=dict)


@dataclass
class Split:
    background: np.ndarray
    #: front to back, as the spec lists them
    lifted: list[Lifted]
    #: per element name, the coverage it was lifted with (for the debug overlay)
    coverage: dict[str, np.ndarray]


def _polys(shape, reg: dict, key: str) -> np.ndarray:
    m = np.zeros(shape, bool)
    for p in reg.get(key, []):
        m |= mk.poly_mask(shape, p)
    return m


def region_coverage(
    reg: dict, work: np.ndarray, orig: np.ndarray, state: dict
) -> tuple[np.ndarray, dict]:
    """An element's soft coverage (0..1) on the canvas, and any meta the renderer wants."""
    h, w = work.shape[:2]
    meta: dict[str, Any] = {}
    if "poly" in reg:
        m = mk.poly_mask((h, w), reg["poly"])
    elif "ring" in reg:
        cx, cy, r = reg["ring"]
        if reg.get("fit", True):
            cx, cy, r = mk.ring_fit(mk.lum(work), cx, cy, r)
        m = mk.ring_mask((h, w), cx, cy, r, float(reg.get("width", 8)))
        meta = {"center": [round(cx, 1), round(cy, 1)], "radius": round(r, 1)}
    else:  # rest: whatever is lit in the portrait and not yet claimed
        lit = (mk.lum(orig) > float(reg.get("lit", 4))) & ~state["claimed"]
        m = mk.fill_holes(mk.close(lit, int(reg.get("close", 12))))
        m &= ~state["taken"]
    if "lit" in reg and "rest" not in reg:
        lit = mk.close(mk.lum(work) > float(reg["lit"]), int(reg.get("close", 3)))
        m &= lit
        if reg.get("holes"):
            m = mk.fill_holes(m)
    # `also` is taken whatever its brightness (a dark mouth inside a bright skull), `minus` never
    m |= _polys((h, w), reg, "also")
    m &= ~_polys((h, w), reg, "minus")
    if "y_min" in reg:
        m &= (np.arange(h) >= reg["y_min"])[:, None]
    if not reg.get("under_cover"):
        m &= ~state["covered"]
    return mk.soften(m, int(reg.get("grow", 0)), float(reg.get("feather", 0))), meta


def _interior(shape, it: dict) -> tuple[np.ndarray, np.ndarray]:
    """A mouth's inside: its coverage, and its colour (a base plus an optional soft glow)."""
    h, w = shape
    cov = mk.soften(mk.poly_mask(shape, it["poly"]), 0, float(it.get("feather", 0)))
    col = np.empty((h, w, 3), np.float32)
    col[:] = np.asarray(it["color"], np.float32)
    g = it.get("glow")
    if g:
        yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
        (cx, cy), (rx, ry) = g["center"], g["radius"]
        fall = np.exp(-(((xx + 0.5 - cx) / rx) ** 2 + ((yy + 0.5 - cy) / ry) ** 2))
        col += fall[..., None] * np.asarray(g["color"], np.float32)
    return cov, col


def _shift_down(m: np.ndarray, d: int) -> np.ndarray:
    out = np.zeros_like(m)
    if d < m.shape[0]:
        out[d:] = m[: m.shape[0] - d]
    return out


def split(portrait: np.ndarray, spec: dict, fill: Fill) -> Split:
    """Lift every element of `spec` off `portrait` (H, W, 3), front to back."""
    fsm.check_spec(spec)
    orig = np.asarray(portrait, np.float32)[..., :3]
    h, w = orig.shape[:2]
    if (w, h) != (int(spec["width"]), int(spec["height"])):
        raise fsm.SpecError(
            f"the portrait is {w}x{h}, the spec is for {spec['width']}x{spec['height']}"
        )
    ground = np.asarray(spec.get("ground_rgb", [0, 0, 0]), np.float32)
    work = orig.copy()
    state = {
        "taken": np.zeros((h, w), bool),  # solids already lifted
        "claimed": np.zeros((h, w), bool),  # lights already lifted
        "covered": np.zeros((h, w), bool),  # always hidden behind a solid in front
    }
    lifted: list[Lifted] = []
    coverage: dict[str, np.ndarray] = {}
    for el in spec["elements"]:
        mode = el["mode"]
        if mode == "none":
            lifted.append(Lifted(el, None))
            continue
        cov, meta = region_coverage(el["region"], work, orig, state)
        coverage[el["name"]] = cov
        hard = cov > 0.02
        how = el.get("fill", "inpaint")
        if mode == "light":
            c = cov[..., None]
            if how == "key":
                k = el["key"]
                lean = mk.channel_excess(work, k.get("over", [1, 2]), k.get("under", [0]))
                wgt = (
                    mk.smoothstep(float(k.get("lo", 6)), float(k.get("hi", 30)), lean)[..., None]
                    * c
                )
                delta = np.maximum(0.0, work - np.asarray(k["base"], np.float32)) * wgt
                work = work - delta
            else:
                # light only adds: what lies behind it is never brighter than what is seen
                filled = np.minimum(fill_crop(fill, work, mk.dilate(hard, 2), margin=1.0), work)
                delta = np.maximum(0.0, work - filled) * c
                work = filled * c + work * (1 - c)
            rgba = np.dstack([delta, cov * 255.0])
            state["claimed"] |= hard
        else:
            rgba = np.dstack([work, cov * 255.0])
            fm = mk.dilate(hard, 3)
            if how == "inpaint":
                filled = fill_crop(fill, work, fm, margin=0.5)
                within = el.get("inpaint_within")
                if within:
                    # inpaint only where something lies behind; elsewhere the ground is behind it
                    keep = _polys((h, w), {"p": within}, "p")
                    filled = np.where(keep[..., None], filled, ground)
                work = np.where(fm[..., None], filled, work)
            elif how == "ground":
                s = mk.soften(hard, 3, 1.5)[..., None]
                work = work * (1 - s) + ground * s
            if "interior" in el:
                icov, icol = _interior((h, w), el["interior"])
                work = work * (1 - icov[..., None]) + icol * icov[..., None]
            solid = cov > 0.5
            state["taken"] |= solid
            if "uncover" in el:
                # what this solid uncovers when it moves (by up to `uncover` px down) stays behind
                # it; the rest of what it covers is never seen, so later layers leave it out
                state["covered"] |= solid & _shift_down(solid, int(el["uncover"]))
            elif el.get("covers"):
                state["covered"] |= solid
        lifted.append(Lifted(el, rgba, meta))
    # what a solid always covers is never seen, so the background holds only the ground there
    s = mk.soften(state["covered"], 2, 1.0)[..., None]
    work = work * (1 - s) + ground * s
    return Split(background=work, lifted=lifted, coverage=coverage)


def _crop(rgba: np.ndarray, light: bool) -> tuple[np.ndarray, tuple[int, int, int, int]] | None:
    a = rgba[..., 3]
    tight = mk.bbox(a, MIN_ALPHA)
    if tight is None:
        return None
    pad = 2 + (int(LIGHT_PAD * max(tight[2], tight[3])) if light else 0)
    x, y, bw, bh = mk.bbox(a, MIN_ALPHA, pad)  # type: ignore[misc]
    crop = np.clip(np.rint(rgba[y : y + bh, x : x + bw]), 0, 255).astype(np.uint8)
    crop[crop[..., 3] < MIN_ALPHA] = 0
    return crop, (x, y, bw, bh)


def assemble(spec: dict, result: Split, portrait_id: str) -> tuple[dict, dict[str, np.ndarray]]:
    """The FormSpec and its pictures (file name -> uint8 RGBA crop), layers back to front."""
    h, w = result.background.shape[:2]
    pictures: dict[str, np.ndarray] = {}
    entries: list[dict] = []
    bg = np.dstack([np.clip(np.rint(result.background), 0, 255), np.full((h, w), 255.0)]).astype(
        np.uint8
    )
    pictures["background.png"] = bg
    entries.append(
        fsm.layer_entry(
            {"name": "background", "kind": "static"},
            1,
            (0, 0, w, h),
            spec["head"],
            "background.png",
        )
    )
    for item in reversed(result.lifted):
        el = item.el
        z = len(entries) + 1
        if item.rgba is None:
            entries.append(fsm.layer_entry(el, z, None, spec["head"], None))
            continue
        cropped = _crop(item.rgba, el["mode"] == "light")
        if cropped is None:
            raise fsm.SpecError(f"{el['name']}: its region is empty")
        pic, box = cropped
        file = f"{el['name']}.png"
        pictures[file] = pic
        entry = fsm.layer_entry(el, z, box, spec["head"], file)
        if item.meta:
            entry["meta"] = item.meta
        entries.append(entry)
    fs = fsm.formspec(spec, portrait_id, entries)
    fsm.check_formspec(fs)
    return fs, pictures


def compose_rest(fs: dict, pictures: dict[str, np.ndarray]) -> np.ndarray:
    """The rig's rest frame as the renderer draws it (no bloom, no motion): solids cover what is
    behind them by their alpha, lights add. (H, W, 3) float 0..255, to compare with the portrait."""
    h, w = int(fs["height"]), int(fs["width"])
    out = np.zeros((h, w, 3), np.float32)
    for layer in fs["layers"]:
        file = layer.get("file")
        if not file:
            continue
        x, y, bw, bh = layer["bbox"]
        pic = pictures[file].astype(np.float32)
        rgb, a = pic[..., :3], pic[..., 3:] / 255.0
        gain = float(layer.get("gain", 1.0))
        dst = out[y : y + bh, x : x + bw]
        if layer.get("mode") == "light" or layer["kind"] == "static":
            dst += rgb * gain
        else:
            occl = a * float(layer.get("occl", 1.0))
            dst *= 1 - occl
            dst += rgb * gain * a
    return np.clip(out, 0, 255)


def debug_overlay(result: Split) -> np.ndarray:
    """Each element's coverage in its own colour over the dimmed portrait's background: uint8 RGB."""
    rng = np.random.default_rng(3)
    acc = result.background.astype(np.float32) * 0.25
    for cov in result.coverage.values():
        acc += rng.uniform(40, 200, 3).astype(np.float32)[None, None, :] * cov[..., None] * 0.6
    return np.clip(acc, 0, 255).astype(np.uint8)
