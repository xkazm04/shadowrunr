// "Open in stage": the broadcast stage (apps/stage) replaying one run, opened outside the
// console. Brain's transcript viewer and On Air's run detail share it; each page styles it.
import { useState } from "react";
import { stageReplayUrl } from "../data/config";
import { openExternal } from "./openExternal";
import styles from "./OpenInStage.module.css";

export function OpenInStage(props: {
  /** The run id, tournament-qualified (`<tournament_id>/<run_id>`) when the caller knows it. */
  readonly runId: string;
  readonly className?: string | undefined;
  /** `data-role` of the button. */
  readonly role: string;
}) {
  const [error, setError] = useState<string | undefined>(undefined);
  const url = stageReplayUrl(props.runId);
  const open = () => {
    setError(undefined);
    openExternal(url).catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  };
  return (
    <>
      <button type="button" className={props.className} data-role={props.role} title={`Open ${url}`} onClick={open}>
        Open in stage
      </button>
      {error !== undefined && (
        <span className={styles.error} role="alert">
          The stage did not open: {error}
        </span>
      )}
    </>
  );
}
