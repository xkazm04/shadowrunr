import { useEffect, useRef, useState } from "react";
import type { Directive } from "../contract";
import { useApi, useRpc } from "../api/context";
import { dateTime } from "../lib/format";
import { useDesk } from "../state";
import { Button, Chip, ErrorLine, ErrorState, Inspector, Popover, Toolbar, cx } from "../ui/primitives";

interface Turn {
  id: number;
  who: "lord" | "reconciler";
  text: string;
  directives?: Directive[];
  error?: unknown;
}

export function DirectiveChip({ d, onDeactivate }: { d: Directive; onDeactivate?: (d: Directive) => void }) {
  return (
    <Popover
      trigger={(open) => (
        <Chip tone={d.active ? d.kind : "neutral"} onClick={open} title={d.text}>
          {d.kind === "prevent" ? "⊘" : "↗"} {d.kind}
          {d.scope ? ` · ${d.scope}` : ""}
          {!d.active && " · off"}
        </Chip>
      )}
    >
      {(close) => (
        <div className="pop-directive">
          <div className="pop-h">
            <Chip tone={d.kind}>{d.kind}</Chip>
            <span className="muted">{d.scope ?? "all segments"}</span>
            <span className="muted mono">{d.id}</span>
          </div>
          <p className="prose">{d.text}</p>
          <div className="pop-f">
            <span className="muted small">{dateTime(d.created_ms)}</span>
            {d.active && onDeactivate && (
              <Button
                variant="danger"
                onClick={() => {
                  onDeactivate(d);
                  close();
                }}
              >
                Deactivate
              </Button>
            )}
          </div>
        </div>
      )}
    </Popover>
  );
}

export function LordChatView() {
  const { client } = useApi();
  const { reportId } = useDesk();
  const directives = useRpc("directive.list", {});
  const [turns, setTurns] = useState<Turn[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [useReport, setUseReport] = useState(true);
  const [deactError, setDeactError] = useState<unknown>(null);
  const seq = useRef(0);
  const threadRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [turns]);
  useEffect(() => inputRef.current?.focus(), []);

  const send = async () => {
    const message = draft.trim();
    if (!message || sending) return;
    setDraft("");
    setSending(true);
    setTurns((t) => [...t, { id: ++seq.current, who: "lord", text: message }]);
    try {
      const params = reportId && useReport ? { report_id: reportId, message } : { message };
      const r = await client.call("lord.chat", params);
      setTurns((t) => [...t, { id: ++seq.current, who: "reconciler", text: r.reply, directives: r.directives_added }]);
      if (r.directives_added.length) directives.reload();
    } catch (e) {
      setTurns((t) => [...t, { id: ++seq.current, who: "reconciler", text: "", error: e }]);
    } finally {
      setSending(false);
    }
  };

  const deactivate = async (d: Directive) => {
    setDeactError(null);
    try {
      await client.call("directive.deactivate", { id: d.id });
      directives.reload();
      setTurns((ts) => ts.map((t) => (t.directives ? { ...t, directives: t.directives.map((x) => (x.id === d.id ? { ...x, active: false } : x)) } : t)));
    } catch (e) {
      setDeactError(e);
    }
  };

  const active = (directives.data ?? []).filter((d) => d.active);

  return (
    <div className="view">
      <div className="main">
        <Toolbar
          title="Lord's chat"
          subtitle="Prevent / try instructions for the next streams. No live control."
        >
          {reportId && (
            <Chip tone={useReport ? "accent" : "neutral"} onClick={() => setUseReport((u) => !u)} title="Send the current reconcile report as context">
              {useReport ? "about" : "not using"} {reportId}
            </Chip>
          )}
        </Toolbar>
        <div className="chat">
          <div className="chat-thread" ref={threadRef} aria-live="polite">
            {turns.length === 0 && (
              <div className="chat-hint muted">
                Write as the Lord: “Never roast what a viewer cannot choose.” · “Try a cold open with the Inspector in SEG-01.”
              </div>
            )}
            {turns.map((t) => (
              <div key={t.id} className={cx("bubble", t.who)}>
                {t.error ? (
                  <ErrorState error={t.error} />
                ) : (
                  <>
                    <div className="bubble-text">{t.text}</div>
                    {!!t.directives?.length && (
                      <div className="chips">
                        {t.directives.map((d) => (
                          <DirectiveChip key={d.id} d={d} onDeactivate={(x) => void deactivate(x)} />
                        ))}
                      </div>
                    )}
                  </>
                )}
              </div>
            ))}
            {sending && <div className="bubble reconciler muted">…</div>}
          </div>
          <form
            className="composer"
            onSubmit={(e) => {
              e.preventDefault();
              void send();
            }}
          >
            <textarea
              ref={inputRef}
              value={draft}
              rows={2}
              placeholder="Instruct the reconciler (Enter to send, Shift+Enter for a new line)"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  void send();
                }
              }}
              aria-label="Message to the reconciler"
            />
            <Button type="submit" variant="primary" disabled={!draft.trim() || sending}>
              Send
            </Button>
          </form>
        </div>
      </div>
      <Inspector title={`Active directives · ${active.length}`}>
        <ErrorLine error={deactError} />
        {directives.error ? (
          <ErrorState error={directives.error} onRetry={directives.reload} />
        ) : (
          <div className="dir-list">
            {active.map((d) => (
              <div key={d.id} className="dir-row">
                <Chip tone={d.kind}>{d.kind === "prevent" ? "⊘" : "↗"}</Chip>
                <span className="ellipsis" title={d.text}>
                  {d.text}
                </span>
                <button type="button" className="icon-btn" onClick={() => void deactivate(d)} title="Deactivate" aria-label={`Deactivate ${d.id}`}>
                  ⏻
                </button>
              </div>
            ))}
            {!active.length && !directives.loading && <div className="muted">No active directives</div>}
          </div>
        )}
      </Inspector>
    </div>
  );
}
