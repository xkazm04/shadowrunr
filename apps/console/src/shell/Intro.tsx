import styles from "./Intro.module.css";

/** Posters slam in (the shell staggers them), the title flashes, then On Air takes the screen. */
export const INTRO = { slamStartMs: 120, slamStepMs: 150, hoverAtMs: 1500, endMs: 2750 } as const;

/** The intro overlay: any key (keymap mode `intro`) or any pointer press skips it. */
export function Intro(props: { narrow: boolean; onSkip: () => void }) {
  return (
    <div className={styles.intro} data-narrow={props.narrow} data-role="intro" onPointerDown={props.onSkip}>
      <div className={styles.title} aria-hidden="true">
        <span>Shadowrunr OS</span>
        <em>Season 0 · six posters · two layers each</em>
      </div>
      <button type="button" className={styles.skip} onClick={props.onSkip}>
        Skip · any key
      </button>
    </div>
  );
}
