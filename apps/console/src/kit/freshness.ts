// How far the live feed vouches for a reply on screen. Freshness rides on `/events`
// invalidation: while the stream is down, or after it reopened and before the revalidation
// lands, a reply may have missed the event that would have refreshed it. Pure; no clock.
import type { ConnectionSnapshot, DataError } from "../data";

/**
 * - `none`: no reply yet.
 * - `live`: the open feed vouches for it (any change would have invalidated it).
 * - `unconfirmed`: the feed is down, or reopened after the reply landed.
 * - `failed`: the last refresh failed; the reply on screen is the one before it.
 */
export type Freshness = "none" | "live" | "unconfirmed" | "failed";

export function freshnessOf(
  reply: { readonly updatedAt: number | undefined; readonly error: DataError | undefined },
  connection: ConnectionSnapshot,
  reopenedAt: number | undefined,
): Freshness {
  if (reply.updatedAt === undefined) return "none";
  if (reply.error !== undefined) return "failed";
  if (feedDown(connection)) return "unconfirmed";
  if (reopenedAt !== undefined && reply.updatedAt < reopenedAt) return "unconfirmed";
  return "live";
}

/**
 * The feed cannot vouch: it dropped (reconnecting or closed) or reports a gap. Before the first
 * open (`idle`, `connecting`) the reply was just fetched, so it counts as live.
 */
export function feedDown(c: ConnectionSnapshot): boolean {
  return c.stream === "reconnecting" || c.stream === "closed" || c.issue !== undefined || c.rpc === "offline";
}

/** A wall-clock time `HH:MM:SS`. Absolute, so nothing ticks. */
export function timeOf(ms: number): string {
  const d = new Date(ms);
  return [d.getHours(), d.getMinutes(), d.getSeconds()].map((n) => String(n).padStart(2, "0")).join(":");
}
