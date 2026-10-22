// The player's "Direct this segment" and "Direct behaviour": a side panel (ui/Drawer) with the
// directions editor of the line's segment (segments/<SEG-ID>.md) or of the character's
// behaviour (directions.md), the line quoted at the end of the draft as `> L123 "text"` for the
// owner to turn into a direction. Nothing is written until Save; closing drops the draft.
import { useState } from "react";
import type { ReviewSession, SegmentId } from "@shadowrunr/contract";
import { appendOf, BehaviourDirectionsEditor, isSegmentId, quoteLine, SegmentDirectionsEditor, segmentDirectionsFile, type Append } from "../../kit/Directions";
import { Button } from "../../ui/Button";
import { Drawer } from "../../ui/Drawer";
import type { SayLine } from "./derive";
import { segmentAt } from "./derive";
import { SEGMENT_NAME } from "./names";
import styles from "./Player.module.css";
import kit from "./tabkit.module.css";

type Target = { readonly kind: "segment"; readonly segment: SegmentId } | { readonly kind: "behaviour" };

interface Open {
  readonly target: Target;
  readonly append: Append;
  readonly line: number;
}

export function DirectActions({ rs, say }: { rs: ReviewSession; say: SayLine }) {
  const [open, setOpen] = useState<Open | undefined>(undefined);
  const [shown, setShown] = useState(false);
  const start = rs.events.find((e) => e.type === "stream_start");
  if (start?.type !== "stream_start") return null;
  const character = start.character;
  const seg = segmentAt(rs, say.t);
  const segment = isSegmentId(seg) ? seg : undefined;
  const direct = (target: Target) => {
    setOpen({ target, append: appendOf(quoteLine(say.line, say.text)), line: say.line });
    setShown(true);
  };
  const segName = (s: SegmentId) => SEGMENT_NAME[s] ?? "this segment";
  const t = open?.target;
  return (
    <>
      <div className={styles.actions} data-role="review-direct">
        {segment && (
          <Button size="compact" onClick={() => direct({ kind: "segment", segment })} data-role="review-direct-segment">
            Direct this segment
          </Button>
        )}
        <Button size="compact" onClick={() => direct({ kind: "behaviour" })} data-role="review-direct-behaviour">
          Direct behaviour
        </Button>
      </div>
      <Drawer
        open={shown && open !== undefined}
        onClose={() => setShown(false)}
        page="review"
        closeLabel="Close directions"
        data-role="review-direct-drawer"
        kicker={open ? `Line ${open.line} · ${t?.kind === "segment" ? segmentDirectionsFile(t.segment) : "directions.md"}` : undefined}
        title={t?.kind === "segment" ? `Direct ${segName(t.segment)}` : "Direct behaviour"}
      >
        {open && (
          <div key={open.append.key} className={kit.stack}>
            <p className={kit.note}>
              {t?.kind === "segment"
                ? "Read into this segment's turn while it is live, never in the others."
                : "The do and don't list, read with the tone on every stream."}{" "}
              The line is quoted at the end: turn it into a direction, then save. The file stays uncommitted until you commit it.
            </p>
            {t?.kind === "segment" ? (
              <SegmentDirectionsEditor character={character} segment={t.segment} role="review-direct" append={open.append} />
            ) : (
              <BehaviourDirectionsEditor character={character} role="review-direct" append={open.append} />
            )}
          </div>
        )}
      </Drawer>
    </>
  );
}
