// Where Malakor looks on the broadcast (`Look`, variants.ts): straight at the viewer in the host
// room, toward the content when he stands in the corner of a content screen, and toward the chat
// column while the line on screen answers chatters. A renderer reads it every frame and may ignore it.

import { isContentScreen, type LayoutId } from "@shadowrunr/stage-kit";
import type { Look, LookSource } from "./variants";

/** The host room: at the viewer. */
export const LOOK_HOST: Look = Object.freeze({ x: 0, y: 0 });
/** In the bottom-left corner of a content screen: up and right, at the content. */
export const LOOK_CONTENT: Look = Object.freeze({ x: 0.6, y: -0.2 });
/** Answering chat: right, at the chat column. */
export const LOOK_CHAT: Look = Object.freeze({ x: 1, y: -0.1 });

/** The look for a layout and the caption on screen (its `replyingTo`, empty when it answers nobody). */
export function lookFor(layout: LayoutId, replyingTo: readonly string[] | null): Look {
  if (replyingTo && replyingTo.length > 0) return LOOK_CHAT;
  return isContentScreen(layout) ? LOOK_CONTENT : LOOK_HOST;
}

/** A look the stage sets on render and the face reads on its own frames. */
export interface LookBox extends LookSource {
  set(look: Look): void;
}

export function lookBox(initial: Look = LOOK_HOST): LookBox {
  let current = initial;
  return {
    get: () => current,
    set: (look) => {
      current = look;
    },
  };
}
