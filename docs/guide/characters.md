# Character packages

A character package is a directory under `characters/<id>/` that holds everything that makes
one character that character: who it is, how it talks, what it calls its viewers, its rank
titles, its voice and face, and the lessons tuned on its runs. The engine itself is
character-neutral. Everything a stream says or shows about its host is read from the package
of the character the stream runs (`stream_start.character`), so a second character needs no
engine change.

The wire format of the manifest is in [docs/spec/contract.md](../spec/contract.md) (the
character package section). This guide covers how to make a package and what each part does.

## The files

```
character.toml      the manifest: id, name, tones, foils, rules and the v2 tables below
constitution.md     IMMUTABLE: identity, awareness, red lines (the brain never edits it)
bible.md            optional: lore, wants, the long arc, running jokes
tones/<variant>.md  one file per tone variant; `default_tone` names one
foils/<foil>.md     optional SEG-01 guest foils (original parody archetypes, never real people)
rules/<rule>.md     constitution rules, switched under [rules]; a rail is also listed in `rails`
stalls.toml         optional: model-free lines spoken while the engine fleet fails
situations.json     optional: the simulator's situation bank
```

The brain reads `constitution.md`, the enabled rules, the active tone and `bible.md` as the
stable prefix of every prompt. The operator's choices (the active tone, rule overrides) live in
`<data>/characters/<id>/`, never in the package.

## `character.toml`

Only `id`, `name` and `default_tone` are required. Every other table is optional: an absent
table or key means the engine's neutral default. An unknown key is an error.

| Table | What it holds | When absent |
|---|---|---|
| top level | `id` (the directory name), `name` (display name), `tagline`, `default_tone`, `foils`, `rails` | `tagline`, `foils` and `rails` empty |
| `[identity]` | `name`, `title`, `address_words`, `reply_prefixes`, `patron`, `follower`, `researcher`, `sign_off`, `disclosure` | a neutral identity built from `name` (viewers, producer, researchers) |
| `[voice]` | `voice`, `sid`, `speed` | the daemon's `[voice]` |
| `[avatar]` | `rig` (the rig spec in the package), `face` (`glow`, `cut` or `depth`) | the daemon's `[stage] avatar` |
| `[ranks]` | a title per rank (`thrall` ... `dread_council`), `weights`, `thresholds`, `propose_min_rank` | the engine's titles and ladder (`segments/data/ranks.toml`) |
| `[rules]` | `<rule-id> = true/false`: each rule's default | every rule file off |
| `[lines]` | `blocked`, `private_details`, `recast`, `rebuttals`: model-free lines | neutral lines |
| `[tuning]` | the lesson-tuned constants, each with its provenance | the engine defaults |
| `[segments."<id>"]` | `rules_prompt` and `states` flavour appended to a segment's prompts | none |

**`[identity]`** is who the character is in the engine's own text. `patron` is who the
character answers to in the fiction (written without "the": the engine's text adds it);
`follower` is what it calls one viewer and many; `researcher` is what it calls the workers it
sends to search the case records. `address_words` are the lowercase words chat calls it by
(addressing detection, and never withheld as a viewer's name); `reply_prefixes` are speaker
labels stripped from a model reply. `sign_off` is spoken when the engine fleet stays lost;
`disclosure` replaces a reply that disclosed being an AI clumsily.

**`[ranks]`.** The six rank wire names are fixed; the titles are the character's. The ladder
overrides (vote `weights`, `thresholds` by gifts and subscription tiers, the `!propose` gate)
replace only what they name, and the merged ladder must still rise. The console's ranks table
(`ranks.table`) and every stream use the merged ladder; a broken one turns pre-flight PF-04
red.

**`[rules]` and `rails`.** Each `rules/<id>.md` is a constitution rule; `[rules]` sets its
default. The operator can switch a rule in `<data>/characters/<id>/rules.json`, except a rail:
a rule listed in `rails` must be `true` and no override reaches it.

## Placeholders

The `[identity]`, `[lines]` and `[segments]` texts, and the engine's own prompt text, are
templates. `{key}` is replaced by the persona's value; `{Key}` (first letter upper case) also
upper-cases the value's first letter. The keys are `name`, `title`, `full_name`, `patron`,
`patron_title`, `follower`, `followers`, `researcher` and `researchers`. Any other `{word}` is
an error at load; a `{` not followed by `word}` is plain text, so JSON in a prompt needs no
escaping.

## Tuning and provenance

`[tuning]` holds the constants that runs have tuned: how many messages one beat answers, how
long an unanswered message is carried, the screen dwell times, the closing window and others
(the full list, with each key's engine default, is `TUNING_DEFAULTS` in `shadowrunr-types`).
Each entry is a table:

```toml
max_picks = { value = 2, provenance = "measured", source = "sbx-20261021-011413-5m: ..." }
carry_ms = { value = 25000, provenance = "assumption", source = "two to five beats, ..." }
```

- `value` is a positive whole number in the key's unit (`_ms` milliseconds, `_s` seconds,
  else a count).
- `provenance` is `measured` (a run showed it; name the run in `source`) or `assumption` (a
  judgement not yet calibrated).
- `source` names the run or lesson the value comes from.

The console's Characters page shows the whole table under Parameters: each key's value, the
engine default and the provenance, with the keys whose value differs from the default marked.

**How a lesson becomes a parameter.** When a run shows that a constant should change (a
session review, a sandbox report, a tournament), write the new value into the character's
`[tuning]` with `provenance = "measured"` and the run id in `source`. The lesson then travels
with the character: a new package made from the template starts from the engine defaults and
their evidence, and its own runs refine them. A lesson about what the character says belongs in
the tone files, `bible.md`, `[lines]` or segment flavour instead; a lesson every character
shares belongs in the engine.

## Making a new character

```
shadowrunr character new nova
shadowrunr character validate nova
```

`character new <id>` copies `characters/_template/` (or `--from <dir>`) to
`characters/<id>/`, sets its `id`, and validates the copy. It refuses an id starting with `_`
and never overwrites an existing directory. The template is neutral: every parameter in its
`character.toml` carries three comment lines, its question, its options and its default (always
one of the options), and every value you must write yourself holds a `FILL:` placeholder. The
constitution keeps the red lines; the template's rules are the safety rules every character
carries (the crisis handoff is a rail).

`character validate <dir|id>` loads the whole package (every tone, the rules, the line bank,
the optional files) and prints each issue as `file: key: reason`. A value still holding
`FILL:` is a warning; anything else is an error. It exits 0 when there are no errors. The
console's `character.validate {id}` RPC returns the same list. Directories whose name starts
with `_` are templates, not characters: `character.list` leaves them out.

Fill in, at least: `name`, `tagline` and `[identity]`; the constitution's identity and
awareness sections; one tone; `bible.md`; `[lines]`; `stalls.toml`. Then run a sandbox
session or a short sim and review it before tuning anything.

## Voice, face and clips

- **Voice.** The daemon's `[voice]` in `shadowrunr.toml` sets up the engine. The configured
  character's (`[sim] character`) `[voice] voice` and `sid` override it field by field when the
  daemon starts. `speed` is part of the schema but not applied yet: the voice sidecar has no
  rate flag.
- **Face renderer.** The configured character's `[avatar] face` overrides `[stage] avatar` in
  the stage URLs the daemon hands out.
- **Face rig.** `/avatar/{file}` serves the rig of the character on air (the latest run in
  flight, else `[sim] character`) from `<data>/characters/<id>/avatar/rig/`. When that
  directory does not exist it falls back to the shared `<data>/avatar/rig/`, where rigs lived
  before packages carried them.

  **One-time migration.** To give the existing rig to its character, move it yourself, for
  example `<data>/avatar/rig/` to `<data>/characters/malakor/avatar/rig/`. Nothing is moved
  automatically, and the shared directory keeps working until you do.
- **Stage.** `stream_start` carries `host_name` (the character's short name); the stage labels
  the host's captions and names the avatar with it, and says "Host" for a stream without it.
- **Clips.** A clip names its host from the run's character: the caption speaker label, the
  caption style, the poster, the hook read's prompt, and the upload description, tags and
  disclosure. The caption speaker of a host line is `host` (a draft stored as `malakor` still
  reads as the host).
- **Reconcile and review.** The Reconciler's prompts and the session tagger's prompt are filled
  from the persona.
