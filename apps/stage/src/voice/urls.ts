// Where the voice and the face rig live: on the same daemon as `/events` (see source.ts).

/** `ws://host:port/events` → `ws://host:port/voice` (the same daemon base). */
export function voiceUrl(eventsUrl: string): string {
  const u = new URL(eventsUrl);
  u.pathname = u.pathname.replace(/\/events\/?$/, "") + "/voice";
  u.search = "";
  return u.toString();
}

/** `ws://host:port/events` → `http://host:port/avatar/` (where the daemon serves the face rig). */
export function rigBaseUrl(eventsUrl: string): string {
  const u = new URL(eventsUrl);
  u.protocol = u.protocol === "wss:" ? "https:" : "http:";
  u.pathname = u.pathname.replace(/\/events\/?$/, "") + "/avatar/";
  u.search = "";
  return u.toString();
}
