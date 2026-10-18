import type { NextStreamPlan } from "../contract";
import { cx } from "./primitives";
import { Chip } from "./primitives";

/** Files a unified diff touches (from its `+++ b/...` headers). */
export function diffFiles(diff: string): string[] {
  return diff
    .split("\n")
    .filter((l) => l.startsWith("+++ "))
    .map((l) => l.slice(4).replace(/^b\//, "").trim());
}

export function DiffView({ diff }: { diff: string }) {
  const touchesConstitution = diffFiles(diff).some((f) => f.endsWith("constitution.md"));
  return (
    <div>
      {touchesConstitution && (
        <div className="error-line">This diff touches constitution.md, which is immutable. The reconciler must never propose it.</div>
      )}
      <pre className="diff" aria-label="Unified diff">
        {diff.split("\n").map((l, i) => (
          <div
            key={i}
            className={cx(
              l.startsWith("+++") || l.startsWith("---") ? "d-file" : l.startsWith("@@") ? "d-hunk" : l.startsWith("+") ? "d-add" : l.startsWith("-") ? "d-del" : "d-ctx",
            )}
          >
            {l || " "}
          </div>
        ))}
      </pre>
    </div>
  );
}

/** The chosen plan's `lord_dispatch`, rendered as an in-world letter, plus the plan's rows. */
export function PlanLetter({ plan }: { plan: NextStreamPlan }) {
  return (
    <div className="plan">
      <article className="letter" aria-label="The Lord's dispatch">
        <div className="letter-seal" aria-hidden="true" />
        <div className="letter-head">A dispatch from the Lord of the Darkness</div>
        {plan.lord_dispatch.split(/\n{2,}/).map((p, i) => (
          <p key={i}>
            {p.split("\n").map((l, j, a) => (
              <span key={j}>
                {l}
                {j < a.length - 1 && <br />}
              </span>
            ))}
          </p>
        ))}
      </article>
      <div className="plan-side">
        <div className="section-h">
          <span>Run of show</span>
          <span className="muted mono">{plan.plan_id}</span>
        </div>
        {plan.segments.map((s) => (
          <div className="tr" key={s.segment} title={s.notes}>
            <span className="c-id mono">{s.segment}</span>
            <span className="c-name">{s.minutes} min</span>
            <span className="c-grow ellipsis muted">{s.notes}</span>
          </div>
        ))}
        {plan.topics.length > 0 && (
          <>
            <div className="section-h">Topics</div>
            <div className="chips">
              {plan.topics.map((t) => (
                <Chip key={t}>{t}</Chip>
              ))}
            </div>
          </>
        )}
        <div className="section-h">Directives fed to the stream</div>
        <div className="chips">
          {plan.directives.length ? (
            plan.directives.map((d) => (
              <Chip key={d.id} tone={d.kind} title={`${d.kind}${d.scope ? ` · ${d.scope}` : ""}: ${d.text}`}>
                {d.kind === "prevent" ? "⊘" : "↗"} {d.text}
              </Chip>
            ))
          ) : (
            <span className="muted">None active</span>
          )}
        </div>
      </div>
    </div>
  );
}
