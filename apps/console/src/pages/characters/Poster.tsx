// The poster face for Characters: the winner's motif (art.js M.characters: an arched niche, a
// crowned skull, a robed figure) and the glance chips on the hovered wall poster (app.js glance).
import { useRpc } from "../../data";
import styles from "./Poster.module.css";

export function Motif() {
  return (
    <svg className={styles.motif} viewBox="0 0 200 400" preserveAspectRatio="xMidYMid slice" aria-hidden="true" data-role="characters-motif">
      <path className={styles.o} d="M26 360V160a74 74 0 0 1 148 0V360Z" transform="translate(7 6)" />
      <path className={styles.k} d="M26 360V160a74 74 0 0 1 148 0V360Z" />
      <path className={styles.f} d="M38 348V160a62 62 0 0 1 124 0V348Z" />
      <path className={`${styles.o} ${styles.breathe}`} d="M70 150L74 112 90 128 100 98 110 128 126 112 130 150Z" />
      <ellipse className={styles.k} cx="100" cy="186" rx="34" ry="40" />
      <circle className={styles.o2} cx="88" cy="182" r="7" />
      <circle className={styles.o2} cx="112" cy="182" r="7" />
      <path className={styles.k} d="M52 348C56 290 80 262 100 262s44 28 48 86Z" />
    </svg>
  );
}

const plural = (n: number, one: string, many: string) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

export function Glance() {
  const list = useRpc("character.list", {});
  const rows = list.data;
  if (!rows) return <b>The cast</b>;
  const foils = rows[0]?.foils.length ?? 0;
  return (
    <>
      <b>{plural(rows.length, "character", "characters")}</b>
      <b>{plural(foils, "foil", "foils")}</b>
    </>
  );
}
