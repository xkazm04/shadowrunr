// `reconcile/chat`: the real Lord's chat over `lord.chat`. Each message goes to the daemon
// (with the report on show as context). Directives the reply proposes are inactive until the
// owner decides (WP14): each shows as a chip with Keep (`directive.add` with the same kind,
// text and scope) and Drop (`directive.deactivate`). A chip's state is read from
// `directive.list`, so it stays right across tab switches and never offers Keep twice.
// The conversation is held for the session only; the daemon keeps its own log.
import type { Directive } from "@shadowrunr/contract";
import { useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { LLM_TIMEOUT_MS, useMutation, useRpc, viewOfError } from "../../data";
import { ConnectionState } from "../../kit";
import { Chip } from "../../ui/Chip";
import { Head } from "../../ui/Section";
import styles from "./Layer2.module.css";
import { session, useSession, type ChatTurn } from "./session";
import { Button } from "../../ui/Button";

/** Turns kept in the DOM; older ones fold into a count (the log is bounded, not virtualised). */
export const CHAT_WINDOW = 50;

export function ChatPage({ reportId }: { reportId: string | undefined }) {
  const { chat } = useSession();
  const send = useMutation("lord.chat", { timeoutMs: LLM_TIMEOUT_MS });
  const [text, setText] = useState("");
  const logRef = useRef<HTMLDivElement>(null);

  const shown = chat.slice(-CHAT_WINDOW);
  const hidden = chat.length - shown.length;

  useLayoutEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [chat.length, send.pending]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    const message = text.trim();
    if (!message || send.pending) return;
    setText("");
    session.say({ who: "me", text: message });
    const r = await send.run({ message, ...(reportId === undefined ? {} : { report_id: reportId }) });
    if (r.ok) session.say({ who: "lord", text: r.value.reply, directives: r.value.directives_added });
    else session.say({ who: "error", text: `${viewOfError(r.error).title}: ${r.error.message}` });
  }

  return (
    <>
      <Head
        kicker="Reconcile & plan · layer 2"
        title="The Lord’s chat"
        lede="Talk to the Lord about the report. Instructions that sound like a rule come back as proposed directives: keep or drop each one."
      />
      <p className={styles.meta}>{reportId === undefined ? "No report on show: the Lord answers without one." : <>About report <code>{reportId}</code></>}</p>
      <ConnectionState>
        <div className={styles.chatbox} data-role="reconcile-chat">
          <div className={styles.msgs} ref={logRef} aria-live="polite">
            {chat.length === 0 && <p className={styles.dim}>Nothing said yet. Tell the Lord what to change, and keep it short.</p>}
            {hidden > 0 && <p className={styles.dim}>{hidden} earlier messages this session</p>}
            {shown.map((t) => (
              <Turn key={t.id} turn={t} />
            ))}
            {send.pending && (
              <p className={styles.dim} role="status">
                The Lord is considering…
              </p>
            )}
          </div>
          <form className={styles.cform} onSubmit={(e) => void onSubmit(e)}>
            <label className={`${styles.field} ${styles.grow}`}>
              <span className={styles.srOnly}>Message the Lord</span>
              <input
                type="text"
                value={text}
                maxLength={240}
                autoComplete="off"
                onChange={(e) => setText(e.target.value)}
                placeholder="e.g. Never let him repeat the poll line in an empty room."
              />
            </label>
            <Button type="submit" variant="primary" disabled={send.pending || text.trim().length === 0}>
              Send
            </Button>
          </form>
        </div>
      </ConnectionState>
    </>
  );
}

function Turn({ turn }: { turn: ChatTurn }) {
  if (turn.who === "me") {
    return (
      <div className={`${styles.m} ${styles.me}`}>
        <b>You</b>
        <p>{turn.text}</p>
      </div>
    );
  }
  if (turn.who === "error") {
    return (
      <div className={`${styles.m} ${styles.mError}`} role="alert">
        <b>Not delivered</b>
        <p>{turn.text}</p>
      </div>
    );
  }
  return (
    <div className={`${styles.m} ${styles.lord}`}>
      <b>The Lord</b>
      <p>{turn.text}</p>
      {turn.directives.length > 0 && (
        <span className={styles.dirChips} data-role="reconcile-chat-directives">
          {turn.directives.map((d) => (
            <Proposal key={d.id} d={d} />
          ))}
        </span>
      )}
    </div>
  );
}

/** One directive the reply proposed: Keep or Drop while pending, then its decided state. */
function Proposal({ d }: { d: Directive }) {
  const list = useRpc("directive.list", {});
  const keep = useMutation("directive.add");
  const drop = useMutation("directive.deactivate");
  const [error, setError] = useState<string | undefined>();
  const live = list.data?.find((x) => x.id === d.id) ?? d;
  const title = d.scope ? `Directive, ${d.scope}` : "Directive, all segments";
  const tone = d.kind === "prevent" ? "fail" : "ok";
  const label = `${d.kind}: ${d.text}`;
  if (!live.proposed) {
    const state = live.active ? "kept" : "dropped";
    return (
      <span className={styles.rowAction} data-role="reconcile-chat-directive" data-state={state}>
        <Chip tone={live.active ? tone : "muted"} title={`${title}, ${state}`}>
          {label}
        </Chip>
        {!live.active && <span className={styles.dim}>dropped</span>}
      </span>
    );
  }
  // Until the list has loaded, the turn's copy may be stale: wait rather than risk a second Keep.
  const busy = keep.pending || drop.pending || (list.data === undefined && list.error === undefined);
  return (
    <span className={styles.rowAction} data-role="reconcile-chat-directive" data-state="proposed">
      <Chip tone="caution" title={`${title}, proposed: inactive until kept`}>
        {label}
      </Chip>
      <Button
        type="button"
        variant="secondary"
        disabled={busy}
        aria-label={`Keep ${label}`}
        onClick={() => void keep.run({ kind: d.kind, text: d.text, ...(d.scope === undefined ? {} : { scope: d.scope }) }).then((r) => setError(r.ok ? undefined : viewOfError(r.error).title))}
      >
        Keep
      </Button>
      <Button type="button" variant="secondary" disabled={busy} aria-label={`Drop ${label}`} onClick={() => void drop.run({ id: d.id }).then((r) => setError(r.ok ? undefined : viewOfError(r.error).title))}>
        Drop
      </Button>
      {error && <small role="alert">{error}</small>}
    </span>
  );
}
