"""The print texture's probe: what style-contract.py cannot see.

    python texture.py capture <winner index url> <roles.json> <contract.texture.json> [--width 1440x1000]
    python texture.py check   <harness url>      <roles.json> <contract.texture.json> [--width 1440x1000]

style-contract.py measures elements, and records a background image only as present or absent.
The poster's print texture is two halftone screens (the dot ink is the page's overprint colours,
so the gradient string carries the palette) and a grain laid over everything as the poster's
`::after` pseudo-element, whose blend mode and opacity are the point. This probe reads those on
the active poster of every page in `texture.pages`, on the winner (capture) and on the harness
(check), and holds the harness to the winner with the same rules as style-contract.py: a length
may move 1px, everything else must match exactly. It exits non-zero on drift.
"""

import argparse
import json
import re
import sys
from pathlib import Path

PROPS = ["opacity", "mixBlendMode", "backgroundImage", "backgroundSize", "maskImage", "inset"]

MEASURE_JS = """(args) => {
  const out = {};
  for (const [layer, spec] of Object.entries(args.layers)) {
    const el = document.querySelector(spec.sel);
    if (!el) { out[layer] = null; continue; }
    const cs = getComputedStyle(el, spec.pseudo || null);
    const v = {};
    for (const p of args.props) v[p] = cs[p] ?? null;
    out[layer] = v;
  }
  return out;
}"""

NUM = re.compile(r"-?\\d+(?:\\.\\d+)?px")


def differs(a, b):
    if a == b:
        return False
    if a is None or b is None:
        return True
    # Lengths may move 1px (sub-pixel rem rounding); the text around them must match exactly.
    if NUM.sub("#", str(a)) != NUM.sub("#", str(b)):
        return True
    na = [float(x[:-2]) for x in NUM.findall(str(a))]
    nb = [float(x[:-2]) for x in NUM.findall(str(b))]
    return len(na) != len(nb) or any(abs(x - y) > 1.0 for x, y in zip(na, nb))


def layers_for(spec, side, page):
    out = {}
    for name in ("halftoneA", "halftoneB", "grain"):
        s = spec[name]
        out[name] = {"sel": s[side].replace("{page}", page), "pseudo": s.get("pseudo")}
    return out


def measure(base, spec, side, width, pages):
    from playwright.sync_api import sync_playwright

    w, h = (int(x) for x in width.split("x"))
    data, errs = {}, []
    with sync_playwright() as p:
        b = p.chromium.launch()
        for page in pages:
            pg = b.new_page(viewport={"width": w, "height": h})
            pg.on("pageerror", lambda e: errs.append(str(e)[:200]))
            pg.goto(f"{base}#/{page}", wait_until="networkidle")
            # 3.5 s: past the winner's intro (2.75 s), which plays on a fresh load of #/onair.
            pg.wait_for_timeout(3500)
            data[page] = pg.evaluate(MEASURE_JS, {"layers": layers_for(spec, side, page), "props": PROPS})
            pg.close()
        b.close()
    return data, errs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["capture", "check"])
    ap.add_argument("target")
    ap.add_argument("roles")
    ap.add_argument("contract")
    ap.add_argument("--width", default="1440x1000")
    a = ap.parse_args()

    spec = json.loads(Path(a.roles).read_text(encoding="utf-8"))["texture"]
    side = "winner" if a.mode == "capture" else "port"
    data, errs = measure(a.target, spec, side, a.width, spec["pages"])
    if errs:
        print("PAGE ERRORS:", errs)

    if a.mode == "capture":
        Path(a.contract).write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        missing = [f"{pg}.{k}" for pg, layers in data.items() for k, v in layers.items() if v is None]
        print(f"captured the texture of {len(data)} page(s) -> {a.contract}")
        if missing:
            print("  NOT FOUND on the winner (fix the selector):", ", ".join(missing))
            sys.exit(1)
        return

    contract = json.loads(Path(a.contract).read_text(encoding="utf-8"))
    rows, checked = [], 0
    for page, layers in contract.items():
        for layer, want in layers.items():
            if want is None:
                continue
            checked += 1
            got = (data.get(page) or {}).get(layer)
            if got is None:
                rows.append((f"{page}.{layer}", "(missing)", "present", "absent"))
                continue
            for prop in PROPS:
                if differs(want.get(prop), got.get(prop)):
                    rows.append((f"{page}.{layer}", prop, want.get(prop), got.get(prop)))
    if rows:
        print(f"{'layer':<22} {'property':<16} {'winner':<40} port")
        for layer, prop, w, g in rows:
            print(f"{layer:<22} {prop:<16} {str(w)[:39]:<40} {str(g)[:60]}")
    print(f"\n{len(rows)} deviation(s) from the winner's texture across {checked} layer(s)")
    sys.exit(1 if rows else 0)


if __name__ == "__main__":
    main()
