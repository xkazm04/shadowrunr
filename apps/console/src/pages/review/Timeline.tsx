// The session's vertical timeline: one-line rows of 10 s to 2 min, a kind filter over the dots
// and the drawer, quiet stretches folded, virtualised past VIRTUAL_AFTER rows. A row opens the
// drawer on its lines; a line (or the route's `timeline/<line>`) opens the moment, and the row
// a deep link lands on flashes once in the page accent.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { pageRoute, useRouter } from "../../router";
import { Button } from "../../ui/Button";
import { Pill, PillGroup, pillRow } from "../../ui/PillGroup";
import { Section } from "../../ui/Section";
import { VirtualList } from "../../ui/VirtualList";
import { buckets, fold, rowOfLine, type Item } from "./buckets";
import { segmentAt } from "./derive";
import { Moment } from "./Moment";
import { HUMOUR, KIND_INK, KINDS, ROW_SIZES, type ShownKind } from "./names";
import type { TabProps } from "./tab";
import styles from "./Timeline.module.css";
import { TimelineFold, TimelineHead, TimelineRow } from "./TimelineRows";

/** Past this many items the rows are virtualised (a 30-minute session at 10 s rows is 180). */
export const VIRTUAL_AFTER = 120;
const ALL: ReadonlySet<ShownKind> = new Set(KINDS.map(([k]) => k));
const SIZE_OPTIONS = ROW_SIZES.map(([s, label]) => ({ id: String(s), label }));

export function Timeline(props: TabProps) {
  const { rs, d, feedback } = props;
  const router = useRouter();
  const rootRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState(30);
  const [shown, setShown] = useState<ReadonlySet<ShownKind>>(ALL);
  const B = useMemo(() => buckets(rs, d, size), [rs, d, size]);
  const items = useMemo(() => fold(B, shown), [B, shown]);
  const maxChat = useMemo(() => Math.max(1, ...B.map((b) => b.chat)), [B]);
  const voted = useMemo(() => new Set(feedback.byLine.keys()), [feedback.byLine]);
  const present = KINDS.filter(([k]) => (d.mix[k] ?? 0) > 0);

  // The drawer: a selected row and/or an open moment. The route's line opens its moment, and
  // its row flashes once (cleared when the flash ends, so a remounted row does not replay it).
  const [row, setRow] = useState<number | undefined>(() => (props.line === undefined ? undefined : rowOfLine(rs, props.line, size, B.length)));
  const [line, setLine] = useState<number | undefined>(() => (props.line !== undefined && rs.events[props.line - 1] ? props.line : undefined));
  const [scrollTo, setScrollTo] = useState<number | undefined>(row);
  const [flash, setFlash] = useState<number | undefined>(row);
  const [routeLine, setRouteLine] = useState(props.line);
  if (props.line !== routeLine) {
    setRouteLine(props.line);
    if (props.line !== undefined && rs.events[props.line - 1]) {
      const r = rowOfLine(rs, props.line, size, B.length);
      setLine(props.line);
      setRow(r);
      setScrollTo(r);
      setFlash(r);
    }
  }
  const open = row !== undefined || line !== undefined;

  // Focus returns to the row the drawer came from when it closes.
  const returnTo = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (open || returnTo.current === undefined) return;
    rootRef.current?.querySelector<HTMLElement>(`[data-row='${returnTo.current}']`)?.focus();
    returnTo.current = undefined;
  }, [open]);

  // Leaving a deep-linked moment drops the line from the route, so the same link opens it again.
  const leaveRoute = () => {
    if (props.line !== undefined) router.navigate(pageRoute("review", rs.row.session_id, "timeline"), { replace: true });
  };
  const close = () => {
    returnTo.current = row;
    setRow(undefined);
    setLine(undefined);
    leaveRoute();
  };
  const back = () => {
    setLine(undefined);
    leaveRoute();
  };
  // Stable, so a memoised row re-renders only when its own data changes.
  const openRow = useCallback(
    (i: number) => {
      setRow(i);
      setLine(undefined);
    },
    [setRow, setLine],
  );

  const resize = (s: number) => {
    setSize(s);
    setRow(undefined);
    setLine(undefined);
    setScrollTo(undefined);
    setFlash(undefined);
    leaveRoute();
  };
  const toggle = (k: ShownKind) =>
    setShown((cur) => {
      const next = new Set(cur);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });

  const renderItem = (it: Item) =>
    it.kind === "fold" ? (
      <TimelineFold key={`f${it.from}`} from={it.from} to={it.to} />
    ) : (
      <TimelineRow
        key={it.b.i}
        b={it.b}
        shown={shown}
        voted={voted}
        maxChat={maxChat}
        segment={segmentAt(rs, (it.b.from + it.b.to) / 2)}
        selected={row === it.b.i}
        flash={flash === it.b.i}
        onOpen={openRow}
      />
    );
  const scrollIndex = scrollTo === undefined ? undefined : items.findIndex((it) => it.kind === "row" && it.b.i === scrollTo);

  return (
    <div
      ref={rootRef}
      className={styles.root}
      data-role="review-timeline"
      data-rise
      onAnimationEnd={(e) => {
        if (e.target instanceof HTMLElement && e.target.dataset["flash"]) setFlash(undefined);
      }}
    >
      <div className={styles.ctl}>
        <span className={styles.label} aria-hidden="true">
          One row is
        </span>
        <PillGroup options={SIZE_OPTIONS} value={String(size)} onChange={(s) => resize(Number(s))} label="One row is" />
        <span className={styles.key}>
          <span className={styles.label}>Band</span> <i className={`${styles.sw} ${styles.segImprov}`} /> Improv room <i className={`${styles.sw} ${styles.segInvestigation}`} /> Investigation ·{" "}
          <i className={`${styles.dot} ${styles.voted} ${styles.keyDot}`} /> you gave feedback
        </span>
      </div>
      <div className={styles.legend} role="group" aria-label="Show these types" data-role="review-tl-filter">
        <span className={styles.label}>Show</span>
        <span className={pillRow}>
          {present.map(([k, name]) => (
            <Pill key={k} pressed={shown.has(k)} onToggle={() => toggle(k)}>
              <i className={styles.sw} style={{ background: KIND_INK[k] }} />
              {name}
            </Pill>
          ))}
        </span>
        <Button size="compact" onClick={() => setShown(ALL)}>
          All types
        </Button>
        <Button size="compact" onClick={() => setShown(new Set(HUMOUR))}>
          Humour only
        </Button>
      </div>
      <Section title="Malakor’s lines, row by row" right={`${B.length} rows`}>
        <div className={styles.tl}>
          <TimelineHead />
          {items.length > VIRTUAL_AFTER ? (
            <VirtualList
              className={styles.viewport}
              count={items.length}
              estimateRem={2}
              overscan={6}
              label="Timeline rows"
              role="review-tl-rows"
              getKey={(i) => {
                const it = items[i];
                return it?.kind === "row" ? it.b.i : `f${it?.from ?? i}`;
              }}
              scrollToIndex={scrollIndex !== undefined && scrollIndex >= 0 ? scrollIndex : undefined}
              renderRow={(i) => {
                const it = items[i];
                return it ? renderItem(it) : null;
              }}
            />
          ) : items.length > 0 ? (
            <div className={styles.rows} data-role="review-tl-rows">
              {items.map(renderItem)}
            </div>
          ) : null}
        </div>
      </Section>
      <Moment rs={rs} d={d} feedback={feedback} open={open} bucket={row === undefined ? undefined : B[row]} line={line} shown={shown} onOpenLine={setLine} onBack={back} onClose={close} />
    </div>
  );
}
