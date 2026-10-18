// characters/all: every character in one virtualised table (the winner's charsAll), then what a
// character is and its special guests.
import { int } from "../../kit/events";
import { FoilArt, MalakorArt } from "@shadowrunr/stage-kit/ui";
import type { CharacterSummary } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useRpc } from "../../data";
import { RouteLink, useRouter } from "../../router";
import { Chip } from "../../ui/Chip";
import { FilterTable, type Column } from "../../ui/FilterTable";
import { Head, Section } from "../../ui/Section";
import { ART_CHARACTER, GenericAvatar } from "./Avatar";
import styles from "./Layer2.module.css";
import { foilName, toneLabel } from "./model";
import { pkgRoute } from "./routes";


const COLUMNS: readonly Column<CharacterSummary>[] = [
  {
    id: "face",
    header: "",
    cell: (c) => <span className={styles.avc}>{c.id === ART_CHARACTER ? <MalakorArt /> : <GenericAvatar id={c.id} />}</span>,
  },
  {
    id: "name",
    header: "Character",
    cell: (c) => (
      <span className={styles.who}>
        <b>{c.name}</b>
        <small>{c.id}</small>
      </span>
    ),
    sortValue: (c) => c.name,
  },
  { id: "tone", header: "Default tone", cell: (c) => toneLabel(c.default_tone), sortValue: (c) => c.default_tone },
  { id: "tones", header: "Tones", cell: (c) => int(c.tones.length), sortValue: (c) => c.tones.length, align: "end" },
  { id: "foils", header: "Foils", cell: (c) => int(c.foils.length), sortValue: (c) => c.foils.length, align: "end" },
  {
    id: "rules",
    header: "Optional rules on",
    cell: (c) => (c.rules_total === 0 ? <span className={styles.dim}>none</span> : <Chip tone={c.rules_enabled === c.rules_total ? "ok" : "caution"}>{`${c.rules_enabled} of ${c.rules_total}`}</Chip>),
    sortValue: (c) => c.rules_enabled,
    align: "end",
  },
];

export function AllPage() {
  const router = useRouter();
  const list = useRpc("character.list", {});
  return (
    <>
      <Head
        kicker="Characters · layer 2"
        title={list.data === undefined ? "The cast" : list.data.length === 1 ? "One character, so far" : `${int(list.data.length)} characters`}
        lede="A character is a folder. The selector on the poster holds as many as you make; this table is where you search, sort and open one."
      />
      <QueryView query={list} isEmpty={(rows) => rows.length === 0} emptyTitle="No characters yet" emptyHint="Add a package folder under characters/ (character.toml, constitution.md and the rest) and it is listed here.">
        {(rows) => (
          <>
            <div className={styles.table} data-role="characters-all-table">
              <FilterTable
                rows={rows}
                columns={COLUMNS}
                getKey={(c) => c.id}
                haystack={(c) => `${c.name} ${c.id} ${c.default_tone} ${c.tones.join(" ")}`}
                label="All characters"
                placeholder="Search name, id or tone"
                estimateRem={4.6}
                onActivate={(c) => router.navigate(pkgRoute(c.id, "constitution"))}
              />
            </div>
            {rows[0] && <Anatomy first={rows[0]} />}
          </>
        )}
      </QueryView>
    </>
  );
}

/** What a character package holds, counted from the first character, and its guests. */
function Anatomy({ first }: { first: CharacterSummary }) {
  const detail = useRpc("character.get", { id: first.id });
  const situations = detail.data?.situations.length;
  const names = new Map((detail.data?.foils ?? []).map((f) => [f.id, foilName(f)]));
  const tree: readonly [string, string][] = [
    ["character.toml", "identity, default tone, rank titles, optional rules"],
    ["constitution.md", "immutable: identity, awareness, red lines"],
    ["bible.md", "lore, want, long arc, running jokes"],
    ["rules/", `${first.rules_total} optional ${first.rules_total === 1 ? "rule" : "rules"}, switched by the operator`],
    ["tones/", `${first.tones.length} variants, the tournament axis`],
    ["foils/", `${first.foils.length} original parody guests`],
    ["situations.json", situations === undefined ? "the sim's situation bank" : `${situations} sim situations`],
  ];
  return (
    <>
      <Section title="What a character is" right={<span className={styles.dim}>counted from {first.name}</span>}>
        <ul className={styles.tree}>
          {tree.map(([path, what]) => (
            <li key={path}>
              <code>{path}</code> <span>{what}</span>
            </li>
          ))}
        </ul>
      </Section>
      {first.foils.length > 0 && (
        <Section title="Special guests">
          <div className={styles.foilRow}>
            {first.foils.map((id) => (
              <RouteLink key={id} to={pkgRoute(first.id, "foils")} className={styles.foilMini}>
                <FoilArt id={id} />
                <b>{names.get(id) ?? foilName({ id })}</b>
              </RouteLink>
            ))}
          </div>
        </Section>
      )}
    </>
  );
}
