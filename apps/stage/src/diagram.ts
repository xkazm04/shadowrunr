// Picks an animated diagram for a `show_card` from its text, so "what Malakor is talking about"
// gets a picture without the action grammar needing a diagram field.
//
//   flow     "A -> B -> C" or "A → B → C"                    animated chain of steps
//   figures  "Promised: 4%; Delivered: 0%" (2+ label:number)  animated comparison bars
//   gauge    exactly one percentage in the body              ring gauge
//   runes    anything else                                    keyword constellation

export type Diagram =
  | { kind: "flow"; steps: string[] }
  | { kind: "figures"; items: { label: string; value: number; unit: string }[] }
  | { kind: "gauge"; value: number; label: string }
  | { kind: "runes"; words: string[] };

const STOP = new Set(
  "the a an and or but of to in on at for with by from is are was were be been it its this that these those as into than then so not no yes his her their our your my we you they he she i me us them which who whom whose what when where why how all any each every some such only own same too very can will just do does did have has had".split(
    " ",
  ),
);

function clean(s: string): string {
  return s.replace(/\s+/g, " ").trim();
}

export function diagramFor(title: string, body: string): Diagram {
  const flowParts = body.split(/\s*(?:->|→|=>)\s*/).map(clean).filter(Boolean);
  if (flowParts.length >= 2) {
    return { kind: "flow", steps: flowParts.slice(0, 5).map((p) => p.slice(0, 28)) };
  }

  const items: { label: string; value: number; unit: string }[] = [];
  for (const part of body.split(/[;\n]|,\s(?=[^\d])/)) {
    const m = /^\s*([^:=]{1,40}?)\s*[:=]\s*([$€£]?)\s*(-?\d+(?:\.\d+)?)\s*(%|x|k|m|bn)?/i.exec(part);
    if (m) {
      items.push({
        label: clean(m[1]!),
        value: Number(m[3]),
        unit: (m[2] ?? "") + (m[4] ?? ""),
      });
    }
  }
  if (items.length >= 2) return { kind: "figures", items: items.slice(0, 6) };

  const pcts = [...body.matchAll(/(-?\d+(?:\.\d+)?)\s*%/g)];
  if (pcts.length === 1) {
    const value = Math.max(0, Math.min(100, Number(pcts[0]![1])));
    return { kind: "gauge", value, label: clean(title).slice(0, 32) };
  }

  const seen = new Set<string>();
  const words: string[] = [];
  for (const w of `${title} ${body}`.split(/[^\p{L}\p{N}'-]+/u)) {
    const k = w.toLowerCase();
    if (w.length < 4 || STOP.has(k) || seen.has(k)) continue;
    seen.add(k);
    words.push(w);
    if (words.length === 6) break;
  }
  return { kind: "runes", words };
}
