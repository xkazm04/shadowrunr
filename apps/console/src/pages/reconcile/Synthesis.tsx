// What the synthesis prompt carried (WP12 `synthesis_input`): the reconciler's LLM reads a
// summary sized to a token budget, so a large tournament is cut. This says what went in and
// what was left out, so a claim in the report is read against what its author saw.
import type { Counted, SynthesisInput } from "@shadowrunr/contract";
import { int } from "../../kit/events";
import { Section } from "../../ui/Section";
import styles from "./Layer2.module.css";

const ROWS: readonly (readonly [keyof SynthesisInput, string])[] = [
  ["runs", "Runs with transcript excerpts"],
  ["failure_refs", "Lines cited for rule failures"],
  ["quality_refs", "Turns marked as failed quality"],
  ["incidents", "Incidents listed one by one"],
  ["tone_cells", "Tone cells"],
  ["rule_failures", "Rule-failure rows"],
];

const isCounted = (v: unknown): v is Counted => typeof v === "object" && v !== null && "included" in v && "omitted" in v;

export function Synthesis({ input }: { input: SynthesisInput | undefined }) {
  if (input === undefined) return null;
  const cut = ROWS.some(([k]) => {
    const v = input[k];
    return isCounted(v) && v.omitted > 0;
  });
  return (
    <Section title="What the reconciler read" right={`${int(input.estimated_tokens)} of ${int(input.budget_tokens)} tokens`}>
      <p className={styles.dim} data-role="reconcile-synthesis-state">
        {cut ? "The prompt was cut to fit its budget; every count below is exact, but the reconciler saw only the included share." : "Everything fit: the reconciler saw the whole summary."}{" "}
        {int(input.excerpt_lines)} transcript lines were excerpted.
      </p>
      <div className={styles.tblwrap}>
        <table className={styles.tbl} data-role="reconcile-synthesis">
          <thead>
            <tr>
              <th>In the prompt</th>
              <th className={styles.num}>Included</th>
              <th className={styles.num}>Left out</th>
            </tr>
          </thead>
          <tbody>
            {ROWS.map(([k, label]) => {
              const v = input[k];
              if (!isCounted(v)) return null;
              return (
                <tr key={k} data-omitted={v.omitted > 0 || undefined}>
                  <td>{label}</td>
                  <td className={styles.num}>{int(v.included)}</td>
                  <td className={styles.num}>{int(v.omitted)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {input.omitted && input.omitted.length > 0 && (
        <ul className={styles.plainList}>
          {input.omitted.map((o) => (
            <li key={o}>{o}</li>
          ))}
        </ul>
      )}
    </Section>
  );
}
