import type { ReactNode } from "react";
import styles from "./VisuallyHidden.module.css";

/** The class that hides text visually and keeps it for screen readers. */
export const visuallyHidden = styles.hidden;

/** Text for assistive technology only (a label the design does not print). */
export function VisuallyHidden({ children }: { children: ReactNode }) {
  return <span className={styles.hidden}>{children}</span>;
}
