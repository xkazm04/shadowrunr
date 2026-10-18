// `reconcile/chat`: the real Lord's chat over `lord.chat`. Each message goes to the daemon
// (with the report on show as context); directives the reply created are shown as chips.
// The conversation is held for the session only; the daemon keeps its own log.
import { useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { viewOfError } from "../../data";
import { ConnectionState } from "../../kit";
import { Chip, Head } from "../../ui";
import { useLongMutation } from "./hooks";
import styles from "./Layer2.module.css";
import { session, useSession, type ChatTurn } from "./session";

/** Turns kept in the DOM; older ones fold into a count (the log is bounded, not virtualised). */
export const CHAT_WINDOW = 50;

export function ChatPage({ reportId }: { reportId: string | undefined }) {
  const { chat } = useSession();
  const send = useLongMutation("lord.chat");
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
        lede="Talk to the Lord about the report. Instructions that sound like a rule become directives."
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
            <button type="submit" className={styles.primary} disabled={send.pending || text.trim().length === 0}>
              Send
            </button>
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
            <Chip key={d.id} tone={d.kind === "prevent" ? "fail" : "ok"} title={d.scope ? `Directive, ${d.scope}` : "Directive, all segments"}>
              {d.kind}: {d.text}
            </Chip>
          ))}
        </span>
      )}
    </div>
  );
}
