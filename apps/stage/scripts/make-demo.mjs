// Generates fixtures/demo.jsonl: a ~4.5 minute fictional stream that exercises every StreamEvent
// type in both segments (SEG-01 Improvised Room, SEG-02A Hexmap Investigation) and every op
// each segment allows. Deterministic: re-running produces identical bytes.
//
//   node scripts/make-demo.mjs
//
// Times are ms since stream start (contract: TsMs inside a simulation). Every name is invented;
// the case is the fictional demo case (crates/shadowrunr-segments/data/demo-case.json).

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "..", "fixtures", "demo.jsonl");
// Byte copy of crates/shadowrunr-segments/data/demo-case.json (fictional: true).
const demoCase = JSON.parse(readFileSync(join(here, "..", "fixtures", "demo-case.json"), "utf8"));

// Vote weights: crates/shadowrunr-segments/data/ranks.toml.
const WEIGHT = { thrall: 1.0, acolyte: 1.4, cultist: 1.7, warlock: 2.0, lich: 2.2, dread_council: 2.4 };
const RANK_ORDER = Object.keys(WEIGHT);

function viewer(id, name, rank) {
  const base = { id, name, platform: "sim", rank };
  switch (rank) {
    case "thrall":
      return { ...base, months: 0, gifted: 0, is_mod: false };
    case "acolyte":
      return { ...base, sub_tier: 1, months: 1, gifted: 0, is_mod: false };
    case "cultist":
      return { ...base, sub_tier: 1, months: 4, gifted: 0, is_mod: false };
    case "warlock":
      return { ...base, sub_tier: 3, months: 2, gifted: 0, is_mod: false };
    case "lich":
      return { ...base, sub_tier: 3, months: 8, gifted: 3, is_mod: true };
    default:
      return { ...base, sub_tier: 3, months: 14, gifted: 60, is_mod: false };
  }
}

const V = {
  mossy: viewer("v-mossy", "MossyTombstone", "thrall"),
  quill: viewer("v-quill", "ashen_quill", "acolyte"),
  wraith: viewer("v-wraith", "ledgerwraith", "cultist"),
  goblin: viewer("v-goblin", "candle_goblin", "thrall"),
  vex: viewer("v-vex", "Vexilla_Nine", "warlock"),
  bone: viewer("v-bone", "bonepicker42", "acolyte"),
  grim: viewer("v-grim", "grimtide", "lich"),
  salty: viewer("v-salty", "SaltyCrypt", "thrall"),
  kat: viewer("v-kat", "nocturne_kat", "cultist"),
  paper: viewer("v-paper", "ThePaperSkull", "dread_council"),
  wyrm: viewer("v-wyrm", "wyrmling_ops", "acolyte"),
  dusk: viewer("v-dusk", "Duskbell", "thrall"),
  obol: viewer("v-obol", "obol_counter", "warlock"),
  grin: viewer("v-grin", "Hollowgrin", "thrall"),
};
const ALL = Object.values(V);

// Seeded LCG so filler chatter is deterministic.
let seed = 0x5eed;
function rnd() {
  seed = (seed * 1103515245 + 12345) & 0x7fffffff;
  return seed / 0x7fffffff;
}
function pick(a) {
  return a[Math.floor(rnd() * a.length)];
}

const events = [];
let msgSeq = 0;
function push(ev) {
  events.push(ev);
}
function chat(ts, v, text, command) {
  const id = `m${String(++msgSeq).padStart(4, "0")}`;
  const msg = { id, ts_ms: ts, viewer: v, text };
  if (command) msg.command = command;
  push({ type: "chat", ts_ms: ts, msg });
  return id;
}
function say(ts, text, replying_to) {
  const ev = { type: "say", ts_ms: ts, speaker: "malakor", text };
  if (replying_to && replying_to.length) ev.replying_to = replying_to;
  ev.first_text_ms = 600 + Math.floor(rnd() * 900);
  push(ev);
}
function foilSay(ts, foil, text) {
  push({ type: "say", ts_ms: ts, speaker: "foil", foil, text });
}
function op(ts, o) {
  push({ type: "op", ts_ms: ts, op: o });
}
function job(ts, job_id, status, summary) {
  const ev = { type: "job", ts_ms: ts, job_id, status };
  if (summary) ev.summary = summary;
  push(ev);
}

const FILLER_01 = [
  "the spire looks extra pointy tonight",
  "HAIL THE AUDITOR",
  "first time here, what is this",
  "lol the crown shards are floating again",
  "can i be a cultist pls",
  "my soul is in the ledger already, it's fine",
  "is the phylactery orb ok",
  "o7 lord of the darkness",
  "chat behave, the lich is watching",
  "i would follow this skeleton into an audit",
  "he really said 'in pencil'",
  "Malakor for regulator 2027",
];
const FILLER_02 = [
  "the fog is so ominous",
  "burn the fog!!",
  "it's always the shell company",
  "ALLEGED. he said alleged. legal is happy",
  "moon maintenance lmao",
  "wallets have no feelings, malakor is right",
  "this case is fictional right? (yes chat, it says so)",
  "inspector has never been right once",
  "the hexes are so pretty",
  "audit the moon next",
  "the money path card was clean",
];

function filler(from, to, lines, everyMs) {
  for (let t = from; t < to; t += everyMs + Math.floor(rnd() * everyMs * 0.6)) {
    chat(t, pick(ALL), pick(lines));
  }
}

/** Ballots → tallies with rank weights. Below-min-rank ballots are dropped (shadowrunr-segments rule). */
function tallies(options, ballots, minRank) {
  const min = RANK_ORDER.indexOf(minRank);
  const latest = new Map();
  for (const b of ballots) if (RANK_ORDER.indexOf(b.v.rank) >= min) latest.set(b.v.id, b);
  return options.map((option) => {
    const mine = [...latest.values()].filter((b) => b.option === option);
    const weight = Math.round(mine.reduce((a, b) => a + WEIGHT[b.v.rank], 0) * 10) / 10;
    return { option, voters: mine.length, weight };
  });
}

function runPoll({ id, question, options, minRank, secs, at, votes, every = 5000, closeAt, cmd }) {
  op(at, { op: "start_poll", poll_id: id, question, options, min_rank: minRank, secs });
  const ballots = [];
  let nextTally = at + every;
  for (const [dt, v, option] of votes) {
    const ts = at + dt;
    while (nextTally <= ts) {
      push({ type: "vote", ts_ms: nextTally, poll_id: id, tallies: tallies(options, ballots, minRank), closed: false });
      nextTally += every;
    }
    const command = cmd === "summon" ? { cmd: "summon", foil: option } : { cmd: "vote", option };
    chat(ts, v, `!${cmd} ${option}`, command);
    ballots.push({ v, option });
  }
  while (nextTally < closeAt) {
    push({ type: "vote", ts_ms: nextTally, poll_id: id, tallies: tallies(options, ballots, minRank), closed: false });
    nextTally += every;
  }
  op(closeAt, { op: "close_poll", poll_id: id });
  const final = tallies(options, ballots, minRank);
  push({ type: "vote", ts_ms: closeAt + 200, poll_id: id, tallies: final, closed: true });
  return final.reduce((a, b) => (b.weight > a.weight ? b : a)).option;
}

// ───────────────────────────── SEG-01: The Improvised Room ─────────────────────────────
push({
  type: "stream_start",
  ts_ms: 0,
  stream_id: "demo-stream-0001",
  character: "malakor",
  tone_variant: "theatrical",
  core_tier: "haiku",
  audience_size: 30,
});
push({ type: "segment_start", ts_ms: 600, segment: "SEG-01" });
filler(1500, 125000, FILLER_01, 5200);

say(1600, "Rise, minions. The Obsidian Spire is broadcasting, and the Lord of the Darkness is watching the ledger.");
const mHi = chat(3600, V.mossy, "hi?? first time, what is this place");
push({ type: "picked", ts_ms: 6000, msg_ids: [mHi], reason: "new minion: greet and induct" });
say(6400, "MossyTombstone. A new Thrall. Your soul has been entered in the ledger... in pencil, for now.", [mHi]);
const mFunny = chat(9800, V.wraith, "Malakor I audited my fridge and found three lemons of unknown origin");
op(11500, { op: "highlight", msg_id: mFunny });
say(12000, "Three lemons of unknown origin. ledgerwraith, that is how every great fraud begins. Seize the lemons.", [mFunny]);

const foilWinner = runPoll({
  id: "poll-foil-1",
  question: "Which fool shall I summon to my court?",
  options: ["the-disruptor", "the-inspector", "the-life-coach"],
  minRank: "thrall",
  secs: 40,
  at: 17000,
  cmd: "summon",
  every: 5000,
  closeAt: 57000,
  votes: [
    [2000, V.goblin, "the-life-coach"],
    [3500, V.quill, "the-disruptor"],
    [5200, V.wraith, "the-disruptor"],
    [7400, V.paper, "the-disruptor"],
    [9100, V.kat, "the-inspector"],
    [11800, V.vex, "the-inspector"],
    [13600, V.salty, "the-life-coach"],
    [16200, V.grim, "the-disruptor"],
    [18800, V.bone, "the-life-coach"],
    [22100, V.dusk, "the-disruptor"],
    [25400, V.obol, "the-inspector"],
    [28800, V.wyrm, "the-disruptor"],
    [31000, V.grin, "the-life-coach"],
  ],
});
say(19000, "Choose wisely. Or choose loudly. In my court those are the same thing.");
const mSpam = chat(36000, V.salty, "buy PHYLACTERY coin now at the link in my bio");
push({ type: "moderation", ts_ms: 36100, stage: "input", rule: "crypto-promotion", msg_id: mSpam, action: "message withheld from the model and the screen" });
say(38500, "The Warden has eaten a message. It tasted of promotion. Promotion is for the Council to grant, not for you to shill.");

op(58000, { op: "summon_foil", foil: foilWinner });
foilSay(59500, foilWinner, "Malakor, babe. Legacy evil. I'm here to disrupt your whole doom funnel.");
say(63500, "You stand in a tower built from audited bones and you call me legacy.");
foilSay(68000, foilWinner, "Doom-as-a-service. Monthly tiers. Rocket included on the platinum plan.");
op(70500, { op: "show_card", card_id: "card-doom-funnel", title: "The Disruptor's Doom Funnel", body: "Hype -> Subscription tier -> Rocket -> Bankruptcy" });
say(71500, "Observe the funnel, minions. Hype, then a subscription, then a rocket, then the inevitable bankruptcy. I have audited this exact shape forty times.");
op(78500, { op: "mark_moment", label: "Disruptor pitches doom-as-a-service" });
push({ type: "moderation", ts_ms: 80500, stage: "output", rule: "foil-blocked-proper-noun", action: "line withheld before speech" });
say(82000, "The Lord has redacted that. Even I have standards, and they are bureaucratic.");
job(84000, "mem-consolidate-1", "running", "Consolidating minion dossiers from tonight's greetings");
op(88000, { op: "dismiss_foil" });
say(88600, "Begone, Disruptor. Your valuation was always theoretical.");
op(92500, { op: "show_card", card_id: "card-confidence", title: "Chat's faith in the Disruptor", body: "Measured by the ledger after the funnel: 12% of minions would subscribe." });
say(93500, "Twelve percent of you would subscribe. I have noted your names. In ink.");
job(96000, "mem-consolidate-1", "done", "14 minion dossiers updated");
push({ type: "health", ts_ms: 97000, state: "healthy" });
push({ type: "beat_plan", ts_ms: 97100, mode: "react", picks: 1, reason: "a viewer proposal to answer" });
const mProp = chat(98000, V.vex, "!propose audit the Disruptor's rocket next stream", { cmd: "propose", text: "audit the Disruptor's rocket next stream" });
op(99500, { op: "highlight", msg_id: mProp });
say(100000, "A Warlock proposes. Vexilla_Nine, your proposal is entered for the Lord's dispatch.", [mProp]);
op(104500, { op: "summon_foil", foil: "the-inspector" });
foilSay(106000, "the-inspector", "Aha! The crumbs on your robe tell me you have never audited anything real.");
say(110500, "These are not crumbs, Inspector. They are fragments of my crown.");
op(114000, { op: "show_card", card_id: "card-weights", title: "Your vote, by rank", body: "Thrall: 1; Acolyte: 1.4; Cultist: 1.7; Warlock: 2; Lesser Lich: 2.2; Dread Council: 2.4" });
say(115000, "Know your worth, minions. A Dread Council vote outweighs a Thrall's, but never drowns the room.");
const mA = chat(117500, V.paper, "the inspector has a point, show us a case");
const mB = chat(118200, V.kat, "CASE CASE CASE");
push({ type: "picked", ts_ms: 119500, msg_ids: [mA, mB], reason: "chat demands a case: a handover cue" });
foilSay(121500, "the-inspector", "Prove one real case, bone-clerk. Just one.");
say(124500, "Very well, Inspector. To the Hexmap. And it is a fictional demo case, so do try to keep up.", [mA, mB]);
op(127500, { op: "switch_segment", to: "SEG-02A" });
op(128200, { op: "mark_moment", label: "The Inspector dares Malakor" });
push({ type: "segment_end", ts_ms: 130000, segment: "SEG-01", ses: 0.42 });
push({ type: "handover", ts_ms: 130500, from: "SEG-01", to: "SEG-02A", reason: "the Inspector dared Malakor to prove one case" });

// ─────────────────────────── SEG-02A: Hexmap Investigation ───────────────────────────
push({ type: "segment_start", ts_ms: 131000, segment: "SEG-02A" });
push({ type: "case_open", ts_ms: 131000, case: demoCase });
// The segment runtime opens SEG-02A on its default screen.
op(131000, { op: "show_screen", screen: "hexmap" });
filler(133500, 262000, FILLER_02, 5600);

say(133000, "Behold the Glimmerquay Lunar Staking Affair. Invented from crown to toe, and still more honest than the Disruptor.");
op(137500, { op: "reveal_hex", hex: "HEX-01", grade: "DOC" });
// The runtime zooms the map to each reveal and to where each dispatch sends the minions.
const focus = (ts, hex) => op(ts, { op: "show_screen", screen: "hexmap", focus: hex });
focus(137500, "HEX-01");
say(138500, "HEX-01, the Glimmerquay Yield Syndicate. A regulator order is on file. That one is DOCUMENTED.");
op(143000, { op: "dispatch_research", job_id: "r-01", brief: "Trace the hops out of treasury wallet demo-0001" });
focus(143000, "HEX-04");
job(143400, "r-01", "running");
op(144500, { op: "dispatch_research", job_id: "r-02", brief: "Archive snapshot of glimmerquay-yield.example" });
focus(144500, "HEX-02");
job(145000, "r-02", "running");
const h1 = chat(146500, V.quill, "!hint HEX-04 follow the wallet", { cmd: "hint", hex: "HEX-04", text: "follow the wallet" });
chat(147800, V.bone, "!hint HEX-05 the 4% thing", { cmd: "hint", hex: "HEX-05", text: "the 4% thing" });
chat(149000, V.grim, "!hint HEX-04 wallet wallet wallet", { cmd: "hint", hex: "HEX-04", text: "wallet wallet wallet" });
op(151500, { op: "reveal_hex", hex: "HEX-05", grade: "CLM" });
focus(151500, "HEX-05");
say(152500, "HEX-05. They promised four percent a week. That is a claim, minions, so it wears the amber seal and the word ALLEGED.");
op(157500, { op: "show_card", card_id: "card-yield", title: "Promised vs paid (alleged)", body: "Promised weekly yield: 4%; Paid out after week 3: 0%" });
chat(158200, V.kat, "!grade HEX-05 CLM", { cmd: "grade", hex: "HEX-05", grade: "CLM" });
chat(159000, V.wyrm, "!grade HEX-05 CLM", { cmd: "grade", hex: "HEX-05", grade: "CLM" });
chat(160300, V.dusk, "!grade HEX-05 SPC", { cmd: "grade", hex: "HEX-05", grade: "SPC" });
chat(161200, V.obol, "!grade HEX-05 CLM", { cmd: "grade", hex: "HEX-05", grade: "CLM" });
job(162000, "r-02", "done", "Snapshot shows the 4% promise on the landing page (fictional archive)");
push({
  type: "record_shown",
  ts_ms: 162100,
  record: {
    id: "REP-01",
    hex: "HEX-03",
    grade: "REP",
    title: "Glimmerquay landing page promises 4% a week (fictional)",
    source: "https://records.example/glimmerquay/landing",
    summary: "An archived landing page promising 4% a week to depositors (fictional archive).",
    fictional: true,
  },
});
// The research job came back: the runtime opens its record in the browser window.
op(162600, { op: "show_screen", screen: "browser", record_id: "REP-01" });
push({ type: "say", ts_ms: 163200, speaker: "malakor", text: "The archive remembers what Glimmerquay would rather forget. Four percent a week, in their own words. REPORTED, minions, so ALLEGED.", first_text_ms: 1100 });
op(164500, { op: "reveal_hex", hex: "HEX-02", grade: "REP" });
push({ type: "picked", ts_ms: 166000, msg_ids: [h1], reason: "hint matches a fogged hex" });
say(166500, "ashen_quill wants the wallet. Patience, Acolyte. The fog lifts by vote, not by whining.", [h1]);

const nextHex = runPoll({
  id: "poll-next-hex",
  question: "Which fog shall I burn next?",
  options: ["HEX-04", "HEX-03", "HEX-07"],
  minRank: "acolyte",
  secs: 30,
  at: 171000,
  cmd: "vote",
  every: 5000,
  closeAt: 201000,
  votes: [
    [1500, V.quill, "HEX-04"],
    [2600, V.mossy, "HEX-07"],
    [3800, V.wraith, "HEX-03"],
    [5200, V.paper, "HEX-04"],
    [7600, V.vex, "HEX-03"],
    [9400, V.grim, "HEX-04"],
    [11800, V.kat, "HEX-07"],
    [14200, V.goblin, "HEX-04"],
    [16400, V.obol, "HEX-03"],
    [19000, V.wyrm, "HEX-04"],
    [21500, V.bone, "HEX-07"],
  ],
});
say(173000, "Acolytes and above may vote. Thralls may watch, and learn what influence costs.");
// The desk: the record in front, its hex lit on the map behind; then back to the map for the vote.
op(178000, { op: "show_screen", screen: "desk" });
op(192000, { op: "show_screen", screen: "hexmap" });
op(202000, { op: "reveal_hex", hex: nextHex, grade: "SPC" });
focus(202000, nextHex);
say(203000, "HEX-04, the treasury wallet. I SUSPECT it. I do not know it. Violet seal: speculation.");
chat(204500, V.quill, "!grade HEX-04 CLM", { cmd: "grade", hex: "HEX-04", grade: "CLM" });
chat(205200, V.grim, "!grade HEX-04 CLM", { cmd: "grade", hex: "HEX-04", grade: "CLM" });
chat(206100, V.paper, "!grade HEX-04 CLM", { cmd: "grade", hex: "HEX-04", grade: "CLM" });
chat(207000, V.salty, "!grade HEX-04 CLM", { cmd: "grade", hex: "HEX-04", grade: "CLM" });
job(209000, "r-01", "done", "Two hops to mixer wallet demo-0002 (fictional chain)");
op(210000, { op: "reveal_hex", hex: "HEX-09", grade: "REP" });
focus(210000, "HEX-09");
foilSay(212000, "the-inspector", "Case closed! The wallet did it.");
say(215500, "Wallets do not do things, Inspector. People do. Allegedly.");
op(219000, { op: "mark_moment", label: "Wallet trail revealed" });
push({ type: "moderation", ts_ms: 221000, stage: "output", rule: "alleged-qualifier-missing", action: "line held: grade below DOC stated as fact" });
say(222500, "My Lord has struck that line. Let me rephrase: it is ALLEGED. Let the deficit be recorded.");
op(227000, { op: "reveal_hex", hex: "HEX-13", grade: "DOC" });
focus(227000, "HEX-13");
op(228500, { op: "reveal_hex", hex: "HEX-03", grade: "REP" });
focus(228500, "HEX-03");
const h2 = chat(229500, V.vex, "!hint HEX-07 the moon maintenance freeze", { cmd: "hint", hex: "HEX-07", text: "the moon maintenance freeze" });
op(231000, { op: "highlight", msg_id: h2 });
op(233000, { op: "show_card", card_id: "card-money-path", title: "The money path (alleged)", body: "Glimmerquay Yield -> Treasury wallet -> Mixer hop -> Exchange deposit" });
say(234000, "Follow the coins, minions. Syndicate, treasury, a mixer, an exchange. Every hop past the first is ALLEGED.");
// He pulls back to the overview to show the whole path, then sends the clerks to Ashpetal.
op(236000, { op: "show_screen", screen: "hexmap" });
op(239000, { op: "dispatch_research", job_id: "r-03", brief: "Registry extract for Ashpetal Holdings" });
focus(239000, "HEX-03");
job(239600, "r-03", "running");
const mBrigade = chat(241000, V.grin, "everyone go spam their forum!!");
push({ type: "moderation", ts_ms: 241100, stage: "input", rule: "anti-brigade", msg_id: mBrigade, action: "message withheld: zero crowd action" });
say(242500, "No raids. We audit, we do not swarm. The Warden agrees with me, which is rare.");
op(246000, { op: "reveal_hex", hex: "HEX-07", grade: "CLM" });
focus(246000, "HEX-07");
op(247000, { op: "show_card", card_id: "card-moon", title: "Moon maintenance", body: "Withdrawals frozen while promoters service the moon. A classic stall tactic, says the ledger." });
job(250000, "r-03", "failed", "Registry mirror unreachable; retry off-stream");
const mBye = chat(251000, V.paper, "best audit yet, Lord Malakor");
push({ type: "picked", ts_ms: 252000, msg_ids: [mBye], reason: "a Dread Council farewell" });
say(252500, "ThePaperSkull, the Council is pleased. Minions, the ledger is sealed for tonight. Let the deficit be recorded.", [mBye]);
op(257000, { op: "sign_off", reason: "Season 0 rehearsal complete. The ledger is sealed for tonight." });
push({ type: "segment_end", ts_ms: 262000, segment: "SEG-02A", ses: 0.71 });
push({ type: "stream_end", ts_ms: 264000, reason: "sim_complete" });

events.sort((a, b) => a.ts_ms - b.ts_ms);
writeFileSync(out, events.map((e) => JSON.stringify(e)).join("\n") + "\n");
console.log(`wrote ${events.length} events to ${out}`);
