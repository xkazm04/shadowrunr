// `tournament.results` is typed by the contract (crates/shadowrunr-types results.rs); the
// door validates every reply, so the page reads the generated types as they are.
export type { Bound, CellKey, CellResult, QualityTally, RuleTally, TournamentResults as SimResults, UsageTotals as Usage } from "@shadowrunr/contract";
