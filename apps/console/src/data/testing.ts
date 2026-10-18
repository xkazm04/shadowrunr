// Test doubles for the data layer: a fake WebSocket, manual timers and a scripted fetch.
// Imported only by tests (and page tests later); nothing here reaches the production bundle.
import type { FetchLike } from "./rpc";
import type { SocketFactory, SocketLike, Timers } from "./socket";

export class FakeSocket implements SocketLike {
  readonly url: string;
  readonly sent: string[] = [];
  closed: { code?: number; reason?: string } | null = null;
  onopen: ((ev: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;
  onclose: ((ev: CloseEvent) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;

  constructor(url: string) {
    this.url = url;
  }
  send(data: string) {
    this.sent.push(data);
  }
  close(code?: number, reason?: string) {
    this.closed = { ...(code === undefined ? {} : { code }), ...(reason === undefined ? {} : { reason }) };
  }
  /** Server side: the connection opened. */
  open() {
    this.onopen?.(new Event("open"));
  }
  /** Server side: push one frame. */
  push(frame: unknown) {
    this.onmessage?.(new MessageEvent("message", { data: typeof frame === "string" ? frame : JSON.stringify(frame) }));
  }
  /** Server side: the connection dropped. */
  drop(code = 1006) {
    this.onerror?.(new Event("error"));
    this.onclose?.(new CloseEvent("close", { code }));
  }
}

export function fakeSockets(): { factory: SocketFactory; all: FakeSocket[]; last: () => FakeSocket } {
  const all: FakeSocket[] = [];
  return {
    factory: (url) => {
      const s = new FakeSocket(url);
      all.push(s);
      return s;
    },
    all,
    last: () => {
      const s = all[all.length - 1];
      if (!s) throw new Error("no socket opened yet");
      return s;
    },
  };
}

/** Timers advanced by hand: `advance(ms)` runs everything due, in order. */
export function manualTimers(): Timers & { advance(ms: number): void; pending(): number[] } {
  let now = 0;
  let next = 1;
  const queue = new Map<number, { at: number; fn: () => void }>();
  return {
    setTimeout(fn, ms) {
      const id = next++;
      queue.set(id, { at: now + ms, fn });
      return id;
    },
    clearTimeout(h) {
      queue.delete(h);
    },
    advance(ms) {
      const until = now + ms;
      for (;;) {
        const due = [...queue].filter(([, t]) => t.at <= until).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        queue.delete(due[0]);
        now = due[1].at;
        due[1].fn();
      }
      now = until;
    },
    pending: () => [...queue.values()].map((t) => t.at - now),
  };
}

export type FetchReply = { status?: number; body: unknown } | { network: true } | { hang: true };

/** A fetch that answers from a script, keyed by RPC method. */
export function scriptedFetch(script: (method: string, params: unknown) => FetchReply): FetchLike & { calls: string[] } {
  const calls: string[] = [];
  const f: FetchLike = (_url, init) => {
    const sent = typeof init.body === "string" ? init.body : "";
    const method = /"method":"([^"]+)"/.exec(sent)?.[1] ?? "?";
    calls.push(method);
    const reply = script(method, sent);
    if ("network" in reply) return Promise.reject(new TypeError("Failed to fetch"));
    if ("hang" in reply) {
      return new Promise((_resolve, reject) => {
        init.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
      });
    }
    const text = typeof reply.body === "string" ? reply.body : JSON.stringify(reply.body);
    const status = reply.status ?? 200;
    return Promise.resolve({ ok: status < 400, status, text: () => Promise.resolve(text) });
  };
  return Object.assign(f, { calls });
}
