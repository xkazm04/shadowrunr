// The chat-command grammar, as the runtime's parser reads it (crates/shadowrunr-segments/src/
// command.rs `parse_command`). The daemon sends only command names (`segments.tables`
// `command_names`), so the argument shape and one-line meaning are copy here, keyed by name.
// A name the daemon adds later still lists, just without this copy. Proposed contract change
// (in the P4 report): `SegmentTables.commands: CommandInfo[]` so this file can go.

export interface Grammar {
  readonly args: string;
  readonly what: string;
}

const GRAMMAR: Readonly<Record<string, Grammar>> = {
  summon: { args: "<foil>", what: "Call a guest foil into the room. The weighted vote decides." },
  hint: { args: "<hex> <text>", what: "Nominate a lead on the hexmap." },
  grade: { args: "<hex> <DOC|REP|CLM|SPC>", what: "Grade a hex. Jury advice, never proof." },
  propose: { args: "<text>", what: "Propose a scheme. Rank-gated." },
  vote: { args: "<option>", what: "Vote in the open poll. Weight rises with rank." },
};

export const grammarOf = (name: string): Grammar | undefined => GRAMMAR[name];
