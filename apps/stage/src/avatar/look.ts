// Where Malakor looks on the broadcast (`Look`, variants.ts): straight at the viewer in the host
// room, toward the content when he stands in the corner of a content screen, and toward the chat
// column while the line on screen answers chatters. A face hears a change as an event (`subscribe`)
// and reads the value on its own frames; it never polls, so a still face costs nothing.

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

/** A look the stage sets on render and the face reads on its own frames, woken by `subscribe`. */
export interface LookBox extends LookSource {
  /** Set the look; subscribers hear it only when it differs from the current one. */
  set(look: Look): void;
  subscribe(fn: (look: Look) => void): () => void;
}

export function lookBox(initial: Look = LOOK_HOST): LookBox {
  let current = initial;
  const subs = new Set<(look: Look) => void>();
  return {
    get: () => current,
    set: (look) => {
      if (look.x === current.x && look.y === current.y) return;
      current = look;
      for (const fn of [...subs]) fn(look);
    },
    subscribe(fn) {
      subs.add(fn);
      return () => void subs.delete(fn);
    },
  };
}
