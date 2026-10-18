import { MODES, type Binding, type Mode, type TypingScope } from "./keymap";

export interface Conflict {
  readonly mode: Mode;
  readonly chord: string;
  readonly typing: TypingScope;
  readonly labels: readonly string[];
}

/** A chord whose meaning differs between modes: reported to the owner, never silently "fixed". */
export interface MeaningChange {
  readonly chord: string;
  readonly meanings: Readonly<Partial<Record<Mode, string>>>;
}

export interface ConflictReport {
  readonly bindings: number;
  readonly conflicts: readonly Conflict[];
  readonly meaningChanges: readonly MeaningChange[];
}

export function conflictReport(keymap: readonly Binding[]): ConflictReport {
  const conflicts: Conflict[] = [];
  for (const mode of MODES) {
    for (const typing of ["never", "only"] as const) {
      const live = keymap.filter((b) => b.modes.includes(mode) && (b.typing ?? "never") === typing);
      const wildcard = live.filter((b) => b.chord === "*");
      const byChord = new Map<string, Binding[]>();
      for (const b of live) byChord.set(b.chord, [...(byChord.get(b.chord) ?? []), b]);
      for (const [chord, bs] of byChord) {
        const clash = chord === "*" ? (live.length > 1 ? live : []) : bs.length > 1 ? bs : wildcard.length > 0 ? [...bs, ...wildcard] : [];
        if (clash.length > 1) conflicts.push({ mode, chord, typing, labels: clash.map((b) => b.label) });
      }
    }
  }

  const meanings = new Map<string, Partial<Record<Mode, string>>>();
  for (const b of keymap) {
    if ((b.typing ?? "never") !== "never") continue;
    const m = meanings.get(b.chord) ?? {};
    for (const mode of b.modes) m[mode] = b.label;
    meanings.set(b.chord, m);
  }
  const meaningChanges: MeaningChange[] = [];
  for (const [chord, m] of meanings) {
    if (new Set(Object.values(m)).size > 1) meaningChanges.push({ chord, meanings: m });
  }
  return { bindings: keymap.length, conflicts, meaningChanges };
}
