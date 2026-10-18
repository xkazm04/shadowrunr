// LEGACY (deleted at cutover C2): the pre-port operator desk, reachable at `#/legacy/<view>`.
// The shell's router owns the view and the keymap owns the keys (Ctrl+1..5 in legacy mode), so
// this file no longer reads the hash or listens to the keyboard itself.
import { ApiProvider, readConfig, useApi } from "../api/context";
import { DirectivesView } from "../views/DirectivesView";
import { LordChatView } from "../views/LordChatView";
import { PreflightView } from "../views/PreflightView";
import { ReconcileView } from "../views/ReconcileView";
import { SimulationsView } from "../views/SimulationsView";
import { DeskProvider, VIEWS, type ViewId } from "./state";
import { cx } from "./ui/primitives";
import "./styles/tokens.css";
import "./styles/app.css";

export const LEGACY_VIEWS: readonly ViewId[] = VIEWS.map((v) => v.id);

const ICONS: Record<ViewId, string> = {
  preflight: "◉",
  simulations: "▦",
  reconcile: "⇄",
  lord: "✉",
  directives: "☰",
};

function ConnectionPill() {
  const { connection, config } = useApi();
  const label =
    connection === "mock"
      ? "Mock data · fictional"
      : connection === "online"
        ? "Daemon online"
        : connection === "offline"
          ? "Daemon offline"
          : "Daemon";
  return (
    <div className={cx("conn", `conn-${connection}`)} title={config.mock ? "?mock=1: fixture JSON, no daemon" : config.daemonUrl}>
      <span className="conn-dot" />
      <span className="conn-label">{label}</span>
      {!config.mock && <span className="conn-url mono">{config.daemonUrl.replace(/^https?:\/\//, "")}</span>}
    </div>
  );
}

function Desk({ view, go }: { view: ViewId; go: (v: ViewId) => void }) {
  const { config } = useApi();
  return (
    <div className="window">
      <nav className="sidebar" aria-label="Views">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true" />
          <span>Shadowrunr · legacy desk</span>
          {config.mock && <span className="mock-badge">MOCK</span>}
        </div>
        <div className="side-group">Desk</div>
        {VIEWS.map((v) => (
          <button
            key={v.id}
            type="button"
            className={cx("side-item", v.id === view && "on")}
            onClick={() => go(v.id)}
            aria-current={v.id === view ? "page" : undefined}
            title={`${v.label} (Ctrl/⌘+${v.key})`}
          >
            <span className="side-icon" aria-hidden="true">
              {ICONS[v.id]}
            </span>
            <span className="side-label">{v.label}</span>
            <kbd>{v.key}</kbd>
          </button>
        ))}
        <div className="side-spacer" />
        <ConnectionPill />
      </nav>
      {view === "preflight" && <PreflightView />}
      {view === "simulations" && <SimulationsView />}
      {view === "reconcile" && <ReconcileView />}
      {view === "lord" && <LordChatView />}
      {view === "directives" && <DirectivesView />}
    </div>
  );
}

export default function LegacyDesk(props: { view: string | undefined; onNavigate: (view: string) => void }) {
  const view: ViewId = LEGACY_VIEWS.find((v) => v === props.view) ?? "preflight";
  return (
    <ApiProvider config={readConfig()}>
      <DeskProvider view={view} go={props.onNavigate}>
        <Desk view={view} go={props.onNavigate} />
      </DeskProvider>
    </ApiProvider>
  );
}
