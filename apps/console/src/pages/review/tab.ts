// The contract between the Review page's layer 2 and its tabs: every tab gets the loaded
// session, its derived view, the feedback state and a way to open a moment on the timeline.
import type { ReviewSession } from "@shadowrunr/contract";
import type { Derived } from "./derive";
import type { Feedback } from "./hooks";
import type { ShownKind } from "./names";

/** `compare` spans two sessions (`#/review/<A>/compare/<B>`); it is not one of the session tabs' bodies. */
export type TabId = "timeline" | "types" | "player" | "judge" | "moderation" | "calls" | "compare";

export interface TabProps {
  readonly rs: ReviewSession;
  readonly d: Derived;
  readonly feedback: Feedback;
  /** Open a line in the timeline's drawer (switches to the timeline tab). */
  readonly openLine: (line: number) => void;
  /** Open the player on one kind. */
  readonly openPlayer: (kind: ShownKind) => void;
  /** The player's kind, from the route (`#/review/<session>/player/<kind>`). */
  readonly kind?: ShownKind;
  /** The line to open in the timeline drawer, from the route (`#/review/<session>/timeline/<line>`). */
  readonly line?: number;
}
