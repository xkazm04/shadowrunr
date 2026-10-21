// Go to anything (Ctrl+K), the overlay. The shell lazy-loads it and mounts it only while open, so
// it costs the first paint nothing and the closed shell no DOM. It owns its keys: arrows, Enter
// and Escape are handled here and preventDefault'ed, which the one keymap listener skips
// (keys/listener.ts), so Escape closes the palette and does not also go up a level. It lands
// through router.navigate, the one write door, and hands focus on: to the page it landed on
// (the spread, or the poster), or back to whatever had it when Escape closes it.
import { useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { encodeRoute, layerOf, useRouter, type LabelOf, type Route } from "../router";
import styles from "./GoToPalette.module.css";
import { placesOf, resolveQuery } from "./goto";

export interface GoToPaletteProps {
  /** The places visited this session, newest first (the current one included). */
  readonly recents: readonly Route[];
  readonly current: Route;
  readonly labelOf: LabelOf;
  readonly onClose: () => void;
}

const optionId = (i: number) => `goto-option-${i}`;

export default function GoToPalette(props: GoToPaletteProps) {
  const { recents, current, labelOf, onClose } = props;
  const router = useRouter();
  const [query, setQuery] = useState("");
  const [at, setAt] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const opener = useRef<Element | null>(null);
  useLayoutEffect(() => {
    opener.current = document.activeElement;
    input.current?.focus();
  }, []);

  const close = (to: Element | null) => {
    onClose();
    if (to instanceof HTMLElement) to.focus({ preventScroll: true });
  };
  const onGo = (to: Route) => {
    router.navigate(to);
    // A page route lands on its spread (layer 2) or its poster's hit area (layer 1); the spread
    // focuses itself when it opens, so focusing it early is harmless.
    const page = to.view === "page" ? `[data-role='poster'][data-page='${to.page}'] > button` : null;
    close(document.querySelector(layerOf(to) === "l2" ? "[data-role='spread']" : (page ?? "[data-role='wall']")));
  };

  const places = useMemo(() => {
    const here = encodeRoute(current);
    return placesOf(
      recents.filter((r) => encodeRoute(r) !== here),
      labelOf,
    );
  }, [recents, current, labelOf]);
  const list = useMemo(() => resolveQuery(query, places, labelOf), [query, places, labelOf]);
  const sel = Math.min(at, list.length - 1);
  const chosen = list[sel];

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (step !== 0) {
      e.preventDefault();
      if (list.length > 0) setAt((sel + step + list.length) % list.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (chosen) onGo(chosen.route);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close(opener.current);
    }
  };

  return (
    <div
      className={styles.scrim}
      data-role="goto"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close(opener.current);
      }}
    >
      <div className={styles.box} role="dialog" aria-modal="true" aria-label="Go to">
        <input
          ref={input}
          className={styles.field}
          type="text"
          role="combobox"
          aria-label="Go to a page, a recent place, a run#line ref or a console link"
          aria-expanded={list.length > 0}
          aria-controls="goto-list"
          aria-autocomplete="list"
          aria-activedescendant={chosen ? optionId(sel) : undefined}
          placeholder="Paste run#line, a link or SEG-02A, or type a page"
          spellCheck={false}
          autoComplete="off"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setAt(0);
          }}
          onKeyDown={onKeyDown}
        />
        <ul id="goto-list" className={styles.list} role="listbox" aria-label="Places">
          {list.map((c, i) => (
            <li
              key={`${c.kind}:${encodeRoute(c.route)}`}
              id={optionId(i)}
              className={styles.option}
              role="option"
              aria-selected={i === sel}
              data-kind={c.kind}
              onMouseMove={() => setAt(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                onGo(c.route);
              }}
            >
              <span className={styles.label}>{c.label}</span>
              <span className={styles.hint}>{c.hint}</span>
            </li>
          ))}
        </ul>
        {list.length === 0 && <p className={styles.empty}>Nothing goes by that name. Try a page, a run#line ref or a #/ link.</p>}
      </div>
    </div>
  );
}
