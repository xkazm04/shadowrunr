// segments/tables/<SEG>: the owner's directions for this segment, per character
// (segments.directions.get / .set). The character is the one the daemon runs (ranks.table)
// until another is picked. The daemon writes segments/<SEG-ID>.md in that character's package
// and the brain reads it into the segment's turn while it is live; the owner commits it.
import { useState } from "react";
import type { SegmentId } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { SegmentDirectionsEditor, segmentDirectionsFile } from "../../kit/Directions";
import { PillGroup } from "../../ui/PillGroup";
import { Section } from "../../ui/Section";
import { StatePanel } from "../../ui/States";
import { useRanks } from "./hooks";
import styles from "./Layer2.module.css";

export function SegmentDirections({ segment, title }: { segment: SegmentId; title: string }) {
  const list = useRpc("character.list", {});
  const ranks = useRanks();
  const [picked, setPicked] = useState<string | undefined>(undefined);
  const rows = list.data ?? [];
  const running = ranks.data?.character;
  const character = picked ?? (rows.some((c) => c.id === running) ? running : rows[0]?.id);
  const name = rows.find((c) => c.id === character)?.name ?? character;
  return (
    <Section title="Directions" right={segmentDirectionsFile(segment)}>
      <div data-role="segments-directions">
        {list.data === undefined && list.errorView ? (
          <StatePanel state={list.errorView.state} title={list.errorView.title} detail={`${list.errorView.code}: ${list.errorView.message}`} action={{ label: "Retry", onClick: list.reload }} />
        ) : list.data === undefined ? (
          <StatePanel state="loading" title="Reading the cast" />
        ) : character === undefined ? (
          <StatePanel state="empty" title="No characters yet" hint="Segment directions belong to a character package: add one under characters/." />
        ) : (
          <>
            {rows.length > 1 && <PillGroup label="Character" options={rows.map((c) => ({ id: c.id, label: c.name }))} value={character} onChange={setPicked} />}
            <p className={styles.note}>
              How {name} runs {title}: read into this segment&apos;s turn while it is live, never in the others. The daemon writes <code>{segmentDirectionsFile(segment)}</code> in the package; it stays uncommitted until you commit it.
            </p>
            <SegmentDirectionsEditor key={`${character}:${segment}`} character={character} segment={segment} role="segments-directions" />
          </>
        )}
      </div>
    </Section>
  );
}
