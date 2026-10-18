// `GET /events` WebSocket client (docs/spec/contract.md § Daemon protocol). The server pushes
// one StreamEvent JSON per message; the client chooses the source with
// `{"subscribe": "<run_id>|live", "speed": 1.0}`.

import type { StreamEvent, SubscribeMessage } from "../contract";

export type EventStreamStatus = "connecting" | "open" | "closed" | "error";

export interface EventSource {
  subscribe(
    target: string,
    speed: number,
    onEvent: (ev: StreamEvent) => void,
    onStatus: (s: EventStreamStatus, detail?: string) => void,
  ): () => void;
}

export function wsUrl(baseUrl: string): string {
  return `${baseUrl.replace(/\/+$/, "").replace(/^http/, "ws")}/events`;
}

export function daemonEvents(baseUrl: string): EventSource {
  return {
    subscribe(target, speed, onEvent, onStatus) {
      let closedByUs = false;
      onStatus("connecting");
      const ws = new WebSocket(wsUrl(baseUrl));
      ws.onopen = () => {
        const msg: SubscribeMessage = { subscribe: target, speed };
        ws.send(JSON.stringify(msg));
        onStatus("open");
      };
      ws.onmessage = (m) => {
        try {
          const ev = JSON.parse(String(m.data)) as StreamEvent;
          if (ev && typeof ev === "object" && typeof ev.type === "string") onEvent(ev);
        } catch {
          onStatus("error", "Unparseable event frame");
        }
      };
      ws.onerror = () => onStatus("error", `Cannot open ${wsUrl(baseUrl)}`);
      ws.onclose = () => {
        if (!closedByUs) onStatus("closed");
      };
      return () => {
        closedByUs = true;
        ws.close();
      };
    },
  };
}
