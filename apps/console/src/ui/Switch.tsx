import styles from "./Switch.module.css";
import { visuallyHidden } from "./VisuallyHidden";

/**
 * An on/off toggle (rules, options). The label names what is switched; `hideLabel` keeps it
 * for assistive technology only, where the row around the switch already names it.
 */
export function Switch(props: { checked: boolean; onChange: (next: boolean) => void; label: string; hideLabel?: boolean; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      className={styles.switch}
      aria-checked={props.checked}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
    >
      <span className={styles.track} aria-hidden="true">
        <span className={styles.knob} />
      </span>
      <span className={props.hideLabel ? visuallyHidden : undefined}>{props.label}</span>
      <span className={styles.state}>{props.checked ? "On" : "Off"}</span>
    </button>
  );
}
