// Line-kind identity colours, kept apart from names.ts so the poster face (in the entry chunk)
// does not pull every label in with it. Literal references, so the token check sees each one.
import type { ShownKind } from "./names";

/** Each kind's identity colour (literal references, so the token check can see every one). */
export const KIND_INK: Readonly<Record<ShownKind, string>> = {
  roast: "var(--sr-review-kind-roast)",
  tease: "var(--sr-review-kind-tease)",
  joke: "var(--sr-review-kind-joke)",
  title: "var(--sr-review-kind-title)",
  welcome: "var(--sr-review-kind-welcome)",
  reply: "var(--sr-review-kind-reply)",
  callout: "var(--sr-review-kind-callout)",
  show: "var(--sr-review-kind-show)",
  refusal: "var(--sr-review-kind-refusal)",
  care: "var(--sr-review-kind-care)",
  disclosure: "var(--sr-review-kind-disclosure)",
  signoff: "var(--sr-review-kind-signoff)",
  stall: "var(--sr-review-kind-stall)",
  other: "var(--sr-review-kind-other)",
  untagged: "var(--sr-review-kind-untagged)",
};
/** The ring on a line the operator has marked. */
export const VOTED_RING = "var(--sr-review-voted-ring)";
