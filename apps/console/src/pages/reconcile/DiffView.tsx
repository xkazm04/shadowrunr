// A proposed unified diff: the files it touches, then its lines in add / del / hunk inks.
// A diff that touches the constitution is flagged; the option cannot be chosen.
import { useMemo } from "react";
import styles from "./Layer2.module.css";
import { isConstitutionPath, parseDiff, type DiffLineKind } from "./model";

const LINE_CLASS: Readonly<Record<DiffLineKind, string | undefined>> = {
  add: styles.add,
  del: styles.del,
  hunk: styles.hunk,
  file: styles.file,
  ctx: undefined,
};

export function DiffView({ diff }: { diff: string }) {
  const parsed = useMemo(() => parseDiff(diff), [diff]);
  return (
    <details className={styles.diffBox} open={parsed.touchesConstitution}>
      <summary>
        The proposed diff
        {parsed.files.map((f) => (
          <code key={f} className={isConstitutionPath(f) ? styles.fileBad : styles.fileOk}>
            {f}
          </code>
        ))}
      </summary>
      {parsed.touchesConstitution && (
        <p className={styles.constitution} role="alert" data-role="reconcile-constitution-flag">
          This diff touches the constitution. The constitution is never changed by a plan, so this option cannot be chosen.
        </p>
      )}
      <pre className={styles.diff}>
        {parsed.lines.map((l, i) => (
          <span key={i} className={LINE_CLASS[l.kind]}>
            {l.text}
            {"\n"}
          </span>
        ))}
      </pre>
    </details>
  );
}
