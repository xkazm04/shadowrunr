// What the broadcast canvas shows when the feed itself is in trouble: the signal-lost slate, the
// stalled mark, the stream clock and the fallback slate behind the error boundary. They sit on
// the 1920x1080 canvas, so they show with `?broadcast=1` too.

import { Component, type ErrorInfo, type ReactNode } from "react";

import { DisclosureBadge, Silhouette } from "@shadowrunr/stage-kit/ui";
import type { SceneState } from "@shadowrunr/stage-kit";

/** mm:ss (minutes keep counting past an hour). */
export function fmtClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** The stream's elapsed time at `now`: from its `stream_start` (or first event), frozen at its end. */
export function streamElapsed(scene: SceneState, now: number): number | null {
  const t0 = scene.meta?.at ?? scene.t0;
  if (t0 === null) return null;
  const t = scene.ended ? Math.min(now, scene.ended.at) : now;
  return Math.max(0, t - t0);
}

export function SignalLostSlate() {
  return (
    <div className="signal-slate" role="status">
      <div className="signal-slate-card">
        <div className="signal-slate-sigil">
          <Silhouette speaking={false} small />
        </div>
        <div className="signal-slate-title">SIGNAL LOST · reconnecting</div>
        <div className="signal-slate-sub">The Spire's glass has clouded. Hold your place, minions.</div>
      </div>
    </div>
  );
}

export function StalledMark({ ms }: { ms: number }) {
  return (
    <div className="stalled-mark" role="status">
      <span className="stalled-dot" />
      signal stalled · {Math.floor(ms / 1000)} s
    </div>
  );
}

/** The calm slate a render error leaves on screen instead of a blank page. */
export function FallbackSlate({ voice }: { voice: boolean }) {
  return (
    <div className="canvas">
      <div className="backdrop">
        <div className="backdrop-grid" />
        <div className="backdrop-glow" />
      </div>
      <div className="signal-slate calm">
        <div className="signal-slate-card">
          <div className="signal-slate-title">THE SPIRE IS RECOMPOSING</div>
          <div className="signal-slate-sub">One moment. The broadcast resumes shortly.</div>
        </div>
      </div>
      <DisclosureBadge voice={voice} />
    </div>
  );
}

/** Retry the canvas this long after a render error. */
export const BOUNDARY_RETRY_MS = 5000;

/**
 * Catches a render throw in the stage canvas: logs it, shows the fallback slate, and tries the
 * canvas again after BOUNDARY_RETRY_MS (a live stream moves on; a bad frame should not end it).
 */
export class StageErrorBoundary extends Component<{ voice: boolean; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  private retry: ReturnType<typeof setTimeout> | null = null;

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error("stage canvas failed to render", error, info.componentStack);
    if (this.retry) clearTimeout(this.retry);
    this.retry = setTimeout(() => {
      this.retry = null;
      this.setState({ failed: false });
    }, BOUNDARY_RETRY_MS);
  }

  componentWillUnmount(): void {
    if (this.retry) clearTimeout(this.retry);
  }

  render(): ReactNode {
    return this.state.failed ? <FallbackSlate voice={this.props.voice} /> : this.props.children;
  }
}
