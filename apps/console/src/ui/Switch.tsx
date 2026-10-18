import styles from "./Switch.module.css";

/** An on/off toggle (rules, options). The label names what is switched. */
export function Switch(props: { checked: boolean; onChange: (next: boolean) => void; label: string; disabled?: boolean }) {
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
      <span>{props.label}</span>
      <span className={styles.state}>{props.checked ? "On" : "Off"}</span>
    </button>
  );
}
