import { useEffect } from "react";
import { useApi } from "./api/context";
import { VIEWS, useDesk, type ViewId } from "./state";
import { cx } from "./ui/primitives";
import { DirectivesView } from "./views/DirectivesView";
import { LordChatView } from "./views/LordChatView";
import { PreflightView } from "./views/PreflightView";
import { ReconcileView } from "./views/ReconcileView";
import { SimulationsView } from "./views/SimulationsView";

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

export function App() {
  const { view, go } = useDesk();
  const { config } = useApi();

  // Keep the view in the hash so a reload (or a shared link) lands on the same view.
  useEffect(() => {
    window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}#${view}`);
  }, [view]);
  useEffect(() => {
    const onHash = () => {
      const v = VIEWS.find((x) => x.id === window.location.hash.slice(1));
      if (v) go(v.id);
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, [go]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return;
      const v = VIEWS.find((x) => x.key === e.key);
      if (v) {
        e.preventDefault();
        go(v.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go]);

  return (
    <div className="window">
      <nav className="sidebar" aria-label="Views">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true" />
          <span>Shadowrunr</span>
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
