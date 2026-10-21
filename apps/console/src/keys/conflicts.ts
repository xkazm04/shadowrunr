import { MODES, type Binding, type Mode, type TypingScope } from "./keymap";
import type { KeyScope } from "./scopes";

export interface Conflict {
  /** Set when the clash is inside one scope; absent for the global bindings. */
  readonly scope?: KeyScope;
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

/**
 * A scoped binding that takes a chord from a global one while focus is inside its scope: a
 * declared, owner-visible fact (the player's 1-7 are marks, not page jumps), never a conflict.
 */
export interface Shadow {
  readonly scope: KeyScope;
  readonly mode: Mode;
  readonly chord: string;
  readonly typing: TypingScope;
  /** The global binding's label. */
  readonly global: string;
  /** The scoped binding's label. */
  readonly scoped: string;
}

export interface ConflictReport {
  readonly bindings: number;
  readonly conflicts: readonly Conflict[];
  readonly meaningChanges: readonly MeaningChange[];
  readonly shadows: readonly Shadow[];
}

const typingOf = (b: Binding): TypingScope => b.typing ?? "never";

/** Clashes among one layer of bindings (the global ones, or one scope's). */
function clashes(layer: readonly Binding[], scope: KeyScope | undefined): Conflict[] {
  const conflicts: Conflict[] = [];
  for (const mode of MODES) {
    for (const typing of ["never", "only"] as const) {
      const live = layer.filter((b) => b.modes.includes(mode) && typingOf(b) === typing);
      const wildcard = live.filter((b) => b.chord === "*");
      const byChord = new Map<string, Binding[]>();
      for (const b of live) byChord.set(b.chord, [...(byChord.get(b.chord) ?? []), b]);
      for (const [chord, bs] of byChord) {
        const clash = chord === "*" ? (live.length > 1 ? live : []) : bs.length > 1 ? bs : wildcard.length > 0 ? [...bs, ...wildcard] : [];
        if (clash.length > 1) conflicts.push({ ...(scope ? { scope } : {}), mode, chord, typing, labels: clash.map((b) => b.label) });
      }
    }
  }
  return conflicts;
}

export function conflictReport(keymap: readonly Binding[]): ConflictReport {
  const global = keymap.filter((b) => b.scope === undefined);
  const scopes = [...new Set(keymap.flatMap((b) => (b.scope ? [b.scope] : [])))];
  const conflicts = [...clashes(global, undefined), ...scopes.flatMap((s) => clashes(keymap.filter((b) => b.scope === s), s))];

  const meanings = new Map<string, Partial<Record<Mode, string>>>();
  for (const b of global) {
    if (typingOf(b) !== "never") continue;
    const m = meanings.get(b.chord) ?? {};
    for (const mode of b.modes) m[mode] = b.label;
    meanings.set(b.chord, m);
  }
  const meaningChanges: MeaningChange[] = [];
  for (const [chord, m] of meanings) {
    if (new Set(Object.values(m)).size > 1) meaningChanges.push({ chord, meanings: m });
  }

  const shadows: Shadow[] = [];
  for (const b of keymap) {
    if (!b.scope) continue;
    for (const mode of b.modes) {
      for (const g of global) {
        if (g.modes.includes(mode) && typingOf(g) === typingOf(b) && (g.chord === b.chord || g.chord === "*")) {
          shadows.push({ scope: b.scope, mode, chord: b.chord, typing: typingOf(b), global: g.label, scoped: b.label });
        }
      }
    }
  }
  return { bindings: keymap.length, conflicts, meaningChanges, shadows };
}
