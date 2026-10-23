# The owner's review loop: logs in, directions out

The brain gets better from two sources: code changes, and the owner's directions. This note is
the second one. The owner watches or reads a show, marks lines, and writes directions in plain
prose. The next show reads them. No code is involved and no restart is needed.

**Who does this:** the owner (the person who decides how the character should behave). Nobody
else writes directions; builders read them as the source of truth on tone.

**Why it matters now:** the last voiced shows still fail two judge rules on content (an "are you
a real lich" bit left unanswered, a celebrity-impression request picked up but never aired),
41–45% of lines carry a worn phrase, and irony moves between 0.54 and 0.91 from show to show.
The tools are built; the directions themselves are still the seed draft in
`characters/malakor/directions.md`, and no segment has directions yet
(`characters/malakor/segments/` does not exist).

## 1. Start the services

```
shadowrunr daemon                      # http://127.0.0.1:7717; --scripted for a keyless rehearsal
pnpm -C apps/console dev               # the console, http://localhost:1420
pnpm -C apps/stage dev                 # optional: the stage, http://localhost:5178/?run=live&broadcast=1
```

Use `localhost`, not `127.0.0.1`, in the browser: the daemon only accepts browser origins on
`http://localhost:*`. For a daemon on another port, add `?daemon=http://127.0.0.1:<port>` to
the console URL, or start the console with `VITE_DAEMON_URL` set to it.

## 2. Run a show

```
shadowrunr stream sandbox --minutes 10 [--seed S] [--tone T] [--record]
```

- Use **10 minutes** or more for anything about the investigation segment (SEG-02A); a 5-minute
  show ends before it starts.
- Keep the judge on (no `--no-judge`): the judge's verdicts are half of what you review.
- Reuse a seed to compare a show before and after your directions on the same simulated room.
- Voice follows the daemon's `[voice]` config and the character's Voice tab; there is no flag.
- The show writes `<runs>/sandbox/sbx-…/`: transcript, ledger, `session.json`, the verdict,
  `tags.json`, `voice.jsonl` and, with `--record`, the recording. Lines are tagged after a
  judged show; `shadowrunr review tag <id>|--all` backfills older ones.

Sims run without the wall clock and are cheaper for a wide sweep (`shadowrunr sim run …`,
`sim tournament <matrix>`, `sim run --segment SEG-02A --case <id> --duration 900 --judge`;
see [the CLI guide](../guide/cli.md)). They can't be opened in Review yet; read their
transcripts directly.

## 3. Review the show

Open `http://localhost:1420/#/review`, pick the show, and work through its tabs
(`#/review/<sbx-id>/<tab>`):

| Tab | Look for |
|---|---|
| `judge` | The rule fails first: each one says what the viewer did and what the host did instead. |
| `player` | The show as it aired, line by line, with the voice and the stage. |
| `timeline` | Where the segments, cases and votes fell; long silences. |
| `types` | Which kinds of line dominate: openers, deflections, case lines, bits. |
| `moderation` | What the gates rewrote or withheld, and why. |
| `calls` | The model calls behind each turn, when a line makes no sense. |

Compare two shows with `#/review/<A>/compare/<B>`, for example the same seed before and after a
direction.

What to look for, in order of payoff:
1. **Judge fails.** A rule that fails twice is a direction to write.
2. **Flat or safe lines.** The goal is irony, rudeness with intelligence, and specifics. A line
   any assistant could say is a mark.
3. **Worn phrases.** The same opener, case name or catchphrase every few lines.
4. **Segment shape.** Whether the investigation reads as a story: a case opened, evidence
   pinned, a payoff, a goodbye.

## 4. Mark lines

In the player, keys **1–8** set one of eight marks on the current line, with an optional note.
Each mark appends to `<data>/review/feedback.jsonl`.

| Mark | Where it goes |
|---|---|
| landed, great_roast | The Tone Lab inbox, as candidates for the character's line bank |
| flat, too_mean | The Tone Lab inbox, as suggested directions |
| broke_character, off_tone, repetitive, wrong_tag | Recorded only; nothing reads them yet (see [leftovers](leftovers.md)) |

## 5. Write directions

Directions are plain prose. Say what he should do, with the line that prompted it. "When a viewer
asks if he is real, he defends his lich pride with a specific grievance; never 'I'm an AI
character'" works; "be funnier" does not.

There are two kinds, and both are reachable from the player's **Direct** buttons, which open an
editor with the current line quoted:

| Kind | Button | File it writes | Reaches the prompt as |
|---|---|---|---|
| Behaviour (the whole show) | **Direct behaviour** | `characters/<id>/directions.md` | `# Directions from the owner`, in the stable prefix of every turn |
| One segment | **Direct this segment** | `characters/<id>/segments/<SEG-ID>.md` | `## Directions from the owner for this segment`, only while that segment is live |

The same segment directions are on the **Segments** page, under each segment's table. Writes are
validated, capped at 20,000 characters and atomic; an invalid write is rolled back.

**Tone Lab** (Characters page → tone-lab tab) is for single lines: generate up to five variants of
a line, keep the good ones in the line bank (`characters/<id>/lines/<tone>.toml`), and clear the
inbox that the marks fill. **Send to Tone Lab** on a Review moment opens a line there.

## 6. Check that it worked

- A direction applies from the **next** show or sim run; one saved during a show changes nothing
  in that show. No restart is needed.
- Run the same seed again and compare it with `#/review/<before>/compare/<after>`.
- Directions are part of the sim instrument hash, so a sim tournament after a direction starts a
  new hash and is not mixed with older results.

## 7. Keep the directions

Directions are files in the repo's `characters/` folder, and **nothing commits them for you**.
After a review session, commit them (or ask the assistant to), so the next builder and the next
machine see them:

```
git add characters/malakor/directions.md characters/malakor/segments/
git commit -m "directions: <what changed>"
```

There is no in-app history or undo yet; git is the history.

## Known gaps in the loop

- Sim and tournament runs can't be opened in Review.
- Four of the eight marks feed nothing, and no mark can agree with or dispute a judge verdict.
- **Direct** exists only in the player, not on a judge-fail row or the timeline drawer.
- The UI doesn't say that a direction applies from the next show, or which character a running
  show uses.

These are listed with evidence in [leftovers.md](leftovers.md#owner-review-loop).
