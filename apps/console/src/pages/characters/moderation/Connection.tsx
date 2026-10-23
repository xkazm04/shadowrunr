// The Twitch connection: its state chip, the device-code login (the code to type at Twitch's
// verification page, with its countdown) and a confirmed disconnect. The bot account's tokens
// never reach the console; only the code the operator types does.
import type { ModeratorStatus, TwitchConnect } from "@shadowrunr/contract";
import { useEffect, useState } from "react";
import { useMutation, viewOfError } from "../../../data";
import { openExternal } from "../../../kit/openExternal";
import { Button } from "../../../ui/Button";
import { Chip } from "../../../ui/Chip";
import { countdown, STATE_NAME, STATE_TONE, twitchOf } from "./model";
import s from "./Moderation.module.css";
import { Confirm, PanelHead, usePoll } from "./parts";

/** How often the status is asked while a login is in progress. */
export const CONNECT_POLL_MS = 3000;

interface Pending extends TwitchConnect {
  readonly at: number;
}

export function ConnectionPanel({ st, reload }: { st: ModeratorStatus; reload: () => void }) {
  const tw = twitchOf(st.platforms);
  const connect = useMutation("twitch.connect");
  const disconnect = useMutation("twitch.disconnect");
  const [code, setCode] = useState<Pending | undefined>(undefined);
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);
  const [now, setNow] = useState(() => Date.now());
  // A finished login (or a disconnect elsewhere) clears the code on screen.
  if (code !== undefined && tw.state === "connected") setCode(undefined);
  const left = code === undefined ? 0 : Math.ceil(code.expires_s - (now - code.at) / 1000);
  const waiting = tw.state === "connecting" || (code !== undefined && left > 0);
  usePoll(reload, CONNECT_POLL_MS, waiting);
  useEffect(() => {
    if (code === undefined) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [code]);

  const onConnect = async () => {
    setError(undefined);
    const r = await connect.run({});
    if (r.ok) {
      setNow(Date.now());
      setCode({ ...r.value, at: Date.now() });
    } else setError(`${viewOfError(r.error).title}: ${r.error.message}`);
  };
  const onDisconnect = async () => {
    setError(undefined);
    const r = await disconnect.run({});
    setAsking(false);
    if (r.ok) setCode(undefined);
    else setError(`${viewOfError(r.error).title}: ${r.error.message}`);
  };
  const on = tw.state === "connected" || tw.state === "connecting";
  return (
    <section className={s.panel} data-role="moderation-connection" aria-labelledby="moderation-connection">
      <PanelHead id="moderation-connection" title="Twitch" />
      <div className={s.row}>
        <span data-role="moderation-twitch-state" data-state={tw.state}>
          <Chip tone={STATE_TONE[tw.state]}>{STATE_NAME[tw.state]}</Chip>
        </span>
        {tw.bot_login !== undefined && <span className={s.note}>bot {tw.bot_login}</span>}
        {tw.channel !== undefined && <span className={s.note}>in #{tw.channel}</span>}
      </div>
      {tw.detail !== undefined && (
        <p className={s.note} data-role="moderation-twitch-detail">
          {tw.detail}
        </p>
      )}
      <div className={s.actions}>
        {on ? (
          <Button disabled={disconnect.pending} onClick={() => setAsking(true)} data-role="moderation-disconnect">
            Disconnect
          </Button>
        ) : (
          <Button variant="ink" disabled={connect.pending} onClick={() => void onConnect()} data-role="moderation-connect">
            {connect.pending ? "Asking Twitch…" : "Connect"}
          </Button>
        )}
      </div>
      {asking && (
        <Confirm
          role="moderation-disconnect-confirm"
          text="Disconnect the bot? The stored login is deleted, and the moderator stops screening the chat until it connects again."
          yes="Disconnect"
          danger
          pending={disconnect.pending}
          onYes={() => void onDisconnect()}
          onNo={() => setAsking(false)}
        />
      )}
      {code !== undefined && tw.state !== "connected" && (
        <div className={s.code} data-role="moderation-device-code">
          <span className={s.label}>Log the bot account in at</span>
          <a
            className={s.link}
            href={code.verification_uri}
            target="_blank"
            rel="noreferrer"
            data-role="moderation-verification-uri"
            onClick={(e) => {
              e.preventDefault();
              openExternal(code.verification_uri).catch((x: unknown) => setError(x instanceof Error ? x.message : String(x)));
            }}
          >
            {code.verification_uri}
          </a>
          <span className={s.label}>and enter</span>
          <span className={s.userCode} data-role="moderation-user-code">
            {code.user_code}
          </span>
          <span className={s.note} data-role="moderation-code-expiry">
            {left > 0 ? `Expires in ${countdown(left)}` : "The code expired. Connect again for a new one."}
          </span>
        </div>
      )}
      {error !== undefined && <p className={s.err}>{error}</p>}
    </section>
  );
}
