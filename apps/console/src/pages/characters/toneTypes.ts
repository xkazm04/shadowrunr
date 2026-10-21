// What the Tone Lab's panels share: the tab's props, the seed of the variants panel and the
// text appended to the directions draft.
import type { LineSituation } from "@shadowrunr/contract";

/** What the variants panel is asked to start from; a new `key` re-seeds it. */
export interface Seed {
  readonly key: number;
  readonly line?: string;
  readonly note?: string;
  readonly situation?: LineSituation;
  /** Where the line came from, as words. */
  readonly source?: string;
}

/** Text to append to the directions draft; a new `key` appends again. */
export interface Append {
  readonly key: number;
  readonly text: string;
}

export interface LabProps {
  readonly character: string;
  readonly tone: string;
  /** A write landed in this package file. */
  readonly onWrote: (file: string) => void;
}

let seq = 0;
export const nextKey = (): number => ++seq;
