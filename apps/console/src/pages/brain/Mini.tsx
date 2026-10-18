// The vision-mixer mini-render (A/1 app.js:52 `MINI.brain`). Loaded with the shell, so it stays
// a few bytes: no CSS module and no palette import (the bus sizes it). A/1's colours, exact.
const MINI = { ground: "#0b100f", rule: "#24564b", trace: "#ff6a2b" } as const;

export function BrainMini() {
  const m = MINI;
  return (
    <svg viewBox="0 0 80 50" width="100%" height="100%" data-role="brain-mini" aria-hidden="true">
      <rect width="80" height="50" fill={m.ground} />
      <path d="M0 12H80M0 25H80M0 38H80" stroke={m.rule} strokeWidth=".6" />
      <path d="M0 28 L18 28 L22 14 L26 40 L30 28 L48 28 L52 10 L56 42 L60 28 L80 28" fill="none" stroke={m.trace} strokeWidth="1.6" strokeDasharray="120" strokeDashoffset="120">
        <animate attributeName="stroke-dashoffset" values="120;0" dur="2.2s" repeatCount="indefinite" />
      </path>
    </svg>
  );
}
