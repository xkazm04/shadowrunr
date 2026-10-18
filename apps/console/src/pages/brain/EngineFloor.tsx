// The engine floor (A/1 engineFloor, app.js:580-591): the routing table and the fleet accounts
// as the engine applies them (`engine.status`: configured routes, overrides, account cooldowns
// by label, the fleet's mode and fallbacks), with what the ledger shows each one did. Without
// `engine.status` (an older daemon, or it failed) the tables fall back to the ledger alone.
import type { EngineStatus } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { int, msText } from "../../kit/events";
import { Chip } from "../../ui/Chip";
import { Section } from "../../ui/Section";
import { roleColor } from "./draw";
import type { EngineSummary } from "./engineModel";
import styles from "./Layer2.module.css";

const MODE_TONE = { primary: "ok", degraded: "caution", down: "fail", scripted: "neutral" } as const;

function Role({ role }: { role: string }) {
  return (
    <span className={styles.role}>
      <i style={{ background: roleColor(role) }} />
      {role}
    </span>
  );
}

const until = (ms: number) => new Date(ms).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });

function LiveRoutes({ st, s }: { st: EngineStatus; s: EngineSummary }) {
  const seen = new Map(s.routes.map((r) => [r.role, r]));
  return (
    <table className={styles.table} data-role="brain-routes" data-source="engine.status">
      <thead>
        <tr>
          <th>Role</th>
          <th>Tier</th>
          <th>Effort</th>
          <th className={styles.num}>Timeout</th>
          <th className={styles.num}>Calls</th>
          <th className={styles.num}>Errors</th>
          <th className={styles.num}>p50</th>
          <th className={styles.num}>p95</th>
          <th className={styles.num}>First text p50</th>
        </tr>
      </thead>
      <tbody>
        {st.routes.map((r) => {
          const o = seen.get(r.role);
          return (
            <tr key={r.role}>
              <td>
                <Role role={r.role} /> {r.overridden && <Chip tone="caution">overridden</Chip>}
              </td>
              <td>{r.tier}</td>
              <td>{r.effort}</td>
              <td className={styles.num}>{msText(r.timeout_ms)}</td>
              <td className={styles.num}>{o ? int(o.calls) : "–"}</td>
              <td className={styles.num}>{o ? o.errors : "–"}</td>
              <td className={styles.num}>{msText(o?.durationP50)}</td>
              <td className={styles.num}>{msText(o?.durationP95)}</td>
              <td className={styles.num}>{msText(o?.firstTextP50)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function ObservedRoutes({ s }: { s: EngineSummary }) {
  return (
    <table className={styles.table} data-role="brain-routes" data-source="ledger">
      <thead>
        <tr>
          <th>Role</th>
          <th>Tier</th>
          <th>Effort</th>
          <th>Engine</th>
          <th>Models</th>
          <th className={styles.num}>Calls</th>
          <th className={styles.num}>Errors</th>
          <th className={styles.num}>p50</th>
          <th className={styles.num}>p95</th>
          <th className={styles.num}>First text p50</th>
        </tr>
      </thead>
      <tbody>
        {s.routes.map((r) => (
          <tr key={r.role}>
            <td>
              <Role role={r.role} />
            </td>
            <td>{r.tiers.join(", ") || "–"}</td>
            <td>{r.efforts.join(", ") || "–"}</td>
            <td>{r.engines.join(", ") || "–"}</td>
            <td className={styles.mono}>{r.models.join(", ") || "–"}</td>
            <td className={styles.num}>{int(r.calls)}</td>
            <td className={styles.num}>{r.errors}</td>
            <td className={styles.num}>{msText(r.durationP50)}</td>
            <td className={styles.num}>{msText(r.durationP95)}</td>
            <td className={styles.num}>{msText(r.firstTextP50)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Accounts: the fleet's rotation (labels and cooldowns) when known, with the ledger's calls. */
function Accounts({ st, s }: { st: EngineStatus | undefined; s: EngineSummary }) {
  const byLabel = new Map<string, EngineSummary["accounts"][number]>();
  for (const a of s.accounts) if (!byLabel.has(a.account)) byLabel.set(a.account, a);
  const live = st?.accounts ?? [];
  const extra = s.accounts.filter((a) => !live.some((l) => l.label === a.account));
  return (
    <table className={styles.table} data-role="brain-accounts">
      <thead>
        <tr>
          <th>Engine</th>
          <th>Account</th>
          <th>State</th>
          <th className={styles.num}>Calls</th>
          <th className={styles.num}>Errors</th>
          <th>Models</th>
        </tr>
      </thead>
      <tbody>
        {live.map((l) => {
          const o = byLabel.get(l.label);
          return (
            <tr key={`live:${l.label}`} data-account={l.label}>
              <td className={styles.mono}>{o?.engine ?? "claude_cli"}</td>
              <td>{l.label}</td>
              <td>{l.cooling_until_ms === undefined ? <Chip tone="ok">in rotation</Chip> : <Chip tone="caution">cooling until {until(l.cooling_until_ms)}</Chip>}</td>
              <td className={styles.num}>{o ? int(o.calls) : "–"}</td>
              <td className={styles.num}>{o ? o.errors : "–"}</td>
              <td className={styles.mono}>{o ? o.models.map((m) => `${m.model} ${m.calls}`).join(" · ") : ""}</td>
            </tr>
          );
        })}
        {extra.map((a) => (
          <tr key={a.key}>
            <td className={styles.mono}>{a.engine}</td>
            <td>{a.account}</td>
            <td>{a.lastError === undefined ? <Chip tone="ok">ok</Chip> : <Chip tone="fail">{a.lastError}</Chip>}</td>
            <td className={styles.num}>{int(a.calls)}</td>
            <td className={styles.num}>{a.errors}</td>
            <td className={styles.mono}>{a.models.map((m) => `${m.model} ${m.calls}`).join(" · ")}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function EngineFloor({ s }: { s: EngineSummary }) {
  const status = useRpc("engine.status", {});
  const st = status.data;
  return (
    <>
      <Section title="Routing" right={st ? "live from the engine" : "as the ledger shows it"}>
        {st ? (
          <p className={styles.note} data-role="brain-fleet">
            Fleet <Chip tone={MODE_TONE[st.fleet.mode]}>{st.fleet.mode}</Chip>
            {st.fleet.since_ms > 0 && ` since ${until(st.fleet.since_ms)}`} · fallbacks: {[...(st.api_fallback ? ["API key"] : []), ...st.fallbacks].join(", ") || "none"}
          </p>
        ) : (
          <p className={styles.note}>{status.errorView ? `The engine's live table is unavailable (${status.errorView.title}); this is what the ledger shows the engine did.` : "Reading the engine's routing table…"}</p>
        )}
        {st ? <LiveRoutes st={st} s={s} /> : <ObservedRoutes s={s} />}
      </Section>
      <Section title="Fleet accounts" right={st ? `${st.accounts.length} in rotation` : `${s.accounts.length} seen`}>
        <Accounts st={st} s={s} />
        {s.unrouted > 0 && <p className={styles.note}>{int(s.unrouted)} ledger rows carry no engine or account: they errored before routing.</p>}
      </Section>
    </>
  );
}
