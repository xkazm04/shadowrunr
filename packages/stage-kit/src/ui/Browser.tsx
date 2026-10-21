// The browser window (appendix (screens)): a staged case record shown as a web page inside a
// browser frame. Nothing here touches the network; the URL bar only prints the record's source.
// Tabs are the records opened this stream, most recent first; the page is the active one.

import type { ShownRecord } from "@shadowrunr/contract";
import type { MapModel } from "../hexgrid";
import { GRADE, isAlleged } from "../lore";
import type { SceneState } from "../sceneModel";

/** A tab's title: the record's title without the fictional marker (the ribbon says it), cut short. */
export function tabTitle(title: string, max = 24): string {
  const t = title.replace(/\s*\(fictional\)\s*/gi, " ").trim();
  return t.length > max ? `${t.slice(0, max - 1).trimEnd()}…` : t;
}

/** The URL bar's two parts: the host, and the path with its query. An unparsable source stays whole. */
export function sourceParts(source: string): { host: string; path: string } {
  try {
    const u = new URL(source);
    const path = `${u.pathname}${u.search}`;
    return { host: u.host, path: path === "/" ? "" : path };
  } catch {
    return { host: source, path: "" };
  }
}

/** The provenance chip: the retrieved date, or the Wayback capture it came through. */
export function provenance(r: ShownRecord): string | null {
  if (r.via !== undefined) return "via Wayback";
  if (r.retrieved !== undefined) return `retrieved ${r.retrieved}`;
  return null;
}

function hexChip(map: MapModel, hex: string): string {
  const label = map.hexes.find((h) => h.id === hex)?.label.replace(/\s*\(fictional\)\s*/gi, " ").trim();
  return label ? `${hex} · ${label}` : hex;
}

export function BrowserWindow({ scene, map, onAir = false }: { scene: SceneState; map: MapModel; onAir?: boolean }) {
  const id = scene.activeRecord;
  const record = id === null ? undefined : scene.records[id];
  if (!record) {
    return (
      <div className="browser-window">
        <div className="browser-chrome">
          <span className="browser-dots" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
        </div>
        <div className="browser-page browser-empty">No record is open.</div>
      </div>
    );
  }
  const { host, path } = sourceParts(record.source);
  const chip = provenance(record);
  const g = GRADE[record.grade];
  const tabs = scene.tabs.flatMap((t) => (scene.records[t] ? [scene.records[t]] : []));
  return (
    <div className="browser-window" data-record={record.id}>
      <div className="browser-chrome">
        <span className="browser-dots" aria-hidden="true">
          <i />
          <i />
          <i />
        </span>
        <div className="browser-tabs">
          {tabs.map((t) => (
            <div key={t.id} className={`browser-tab${t.id === record.id ? " active" : ""}`}>
              <span className="browser-tab-id">{t.id}</span>
              <span className="browser-tab-title">{tabTitle(t.title)}</span>
            </div>
          ))}
        </div>
      </div>
      <div className="browser-bar">
        <span className="browser-lock" aria-hidden="true" />
        <span className="browser-url">
          <span className="browser-host">{host}</span>
          <span className="browser-path">{path}</span>
        </span>
        {chip && <span className="browser-chip">{chip}</span>}
        {!onAir && <span className="browser-note">staged copy · no live network</span>}
      </div>
      <div className="browser-page" data-grade={record.grade} key={record.id}>
        {record.fictional && (
          <div className="browser-ribbon">
            <b>FICTIONAL</b> a staged record, invented for the demo case
          </div>
        )}
        <div className="browser-doc">
          <div className="browser-head">
            <span className="browser-grade">{g.code}</span>
            <span className="browser-grade-name">{g.name}</span>
            {isAlleged(record.grade) && <span className="browser-alleged">ALLEGED</span>}
          </div>
          <h2 className="browser-title">{record.title}</h2>
          <p className="browser-summary">{record.summary}</p>
          <span className="browser-hex">{hexChip(map, record.hex)}</span>
          <div className="browser-skeleton" aria-hidden="true">
            <i />
            <i />
            <i />
            <i />
          </div>
        </div>
      </div>
    </div>
  );
}
