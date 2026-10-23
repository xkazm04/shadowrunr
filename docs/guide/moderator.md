# The chat moderator

The moderator is a second character that guards your Twitch chat from its own bot account. It
reads every chat message, whether or not a show is running, and checks it against your rules:

- **Pattern rules** match a term or a regex at once, with no model call. The match is
  case-insensitive. It runs on the text as written, on the normalised text (lowercase,
  punctuation and separators folded into spaces) and on the normalised text with stretched
  letters collapsed ("KYYYYS" matches `\bkys\b`).
- **Plain rules** are a sentence such as "no spoilers for the finale". A small, fast classifier
  model (`mod_classifier`) judges them in batches. A batch closes every
  `[moderator] classifier_batch_ms` (1.5 s by default). The model is only called while at least
  one plain rule is enabled, and only for messages no pattern rule took.
- **Strikes** escalate a rule: after `count` hits by the same viewer within `window_s`, the
  action becomes `then` (for example delete, delete, then ban).

Two things are never actioned:

- **A self-harm disclosure** ("i want to die", "thinking about hurting myself"). Safety outranks
  moderation, even when the message also contains a pattern term.
- **Moderators and the broadcaster.** This comes from Twitch's own badges and never from what a
  message claims.

Each decision gets a freshly written, in-character line from the moderator (`mod_voice`). The
line names the rule and never quotes the viewer. The platform action never waits for the line.

## Setup

1. **Create a Twitch application.** In the Twitch developer console
   (dev.twitch.tv/console/apps), register an application:
   - Client type: **Public**.
   - OAuth redirect URL: `http://localhost` (the device-code flow does not use it, but the form
     requires one).
   - Enable the **device code** grant flow for it.

   Copy its **Client ID**. A client id is public: it names the app, not an account. There is no
   client secret.
2. **Configure `shadowrunr.toml`:**

   ```toml
   [twitch]
   client_id = "your-app-client-id"
   channel = "your_channel_login"     # the broadcaster's login, the channel to guard

   [moderator]
   character = "moderator"            # the package under characters/ with role = "moderator"
   mcp_allow_writes = true            # let Claude Code change rules and mode (see below)
   ```

   An empty `client_id` keeps Twitch off.
3. **Make the bot account a moderator.** In your channel's chat, type `/mod <bot_login>`. The
   bot can only delete, time out and ban once it is a channel moderator.
4. **Start the daemon** (`shadowrunr daemon`) and connect the bot (see "Connecting the bot"
   below).

<!-- WP2 -->

<!-- WP3 -->

## Configuring it from Claude Code

`shadowrunr mcp` exposes the moderator as tools. Add it to your own Claude Code once:

```sh
claude mcp add shadowrunr -- shadowrunr mcp
```

Run that from the repo directory, or pass `--config <path>` after `mcp` so it finds your
`shadowrunr.toml`.

Read tools always work:

- `moderator_status`
- `moderator_rules_list`
- `moderator_rules_test`
- `moderator_log`
- `moderator_character_get`

Write tools run only with `[moderator] mcp_allow_writes = true`:

- `moderator_rules_set`
- `moderator_mode_set`
- `moderator_unban`
- `twitch_connect`
- `twitch_disconnect`
- `moderator_character_set`

When a daemon is running, every moderator tool goes to it, so the live lane picks up a new rule
list on the very next message. With no daemon, rules, mode and log are read and written in place
(the same files), and the Twitch tools refuse.

`moderator_character_set` is not available yet: no daemon call writes `character.toml`. Edit
`characters/<id>/character.toml` by hand and restart the daemon.

### Example prompts

- "Show me the moderator's status and its rules."
- "Add a rule: time out for 10 minutes anyone who posts a link to a crypto giveaway. Test it on
  'free eth at giveaway-scam dot com' first."
- "Add a plain rule 'no spoilers for the season finale', delete only, and after 3 hits in 10
  minutes time them out for 5 minutes."
- "What did the moderator do in the last hour? Anything that looks like a false positive?"
- "Disable the caps rule; it is catching hype."
- "Unban viewer 123456789 on Twitch."
- "Arm the moderator."

Claude reads the list, edits it and writes the whole list back with `moderator_rules_set`. An
invalid rule refuses the entire write, names the rule, and leaves the old list in place.

## Shadow, then review, then arm

The moderator starts in **shadow** mode. It decides and logs what it *would have* done
(`executed: false`), makes no platform call and posts nothing. Run it like that for a stream or
two:

1. Read the decisions: `moderator_log`, or the console's Moderation tab.
2. Fix false positives: narrow the pattern, reword the plain rule, add strikes instead of an
   instant ban.
3. Try edge cases with `moderator_rules_test`. It has no side effects: nothing is logged, done
   or posted.
4. When the log reads right, switch to **armed** (`moderator_mode_set` with `armed`). The mode
   persists across restarts.

In armed mode:

- delete removes the message;
- timeout and ban also remove it (Twitch does that itself);
- the moderator's line is posted in chat once the action succeeded.

## Unban

`moderator_unban` takes `{ platform, viewer_id }`. Take the `viewer_id` from the decision in the
log. It lifts a ban or a timeout. The unban is appended to the log either way. When Twitch is not
connected, `executed` is `false` and `error` says why.

## What is logged where

All of it lives under `<data>/moderator/` (`[paths] data`, `data/` by default). None of it is
committed.

| file | what |
|---|---|
| `rules.json` | your rule list, `{ "rules": [...] }`. It is seeded once, on first start, from the Twitch `block` rules of `policy/platform-rules.json` (as ten-minute timeouts). An empty list stays empty. |
| `state.json` | the mode, `{ "mode": "shadow" }` or `"armed"`. |
| `decisions.jsonl` | append-only, one entry per line, newest last: every decision (`kind: "decision"`) and unban (`kind: "unban"`). `moderator_log` reads it newest first (100 by default, up to 1000). |

A decision records:

- the viewer's id and name, the message id and the rule;
- the action after strikes, with `timeout_s`;
- the mode, whether it was executed, and the platform error if any;
- the moderator's line (absent when the line was not ready within the `mod_voice` timeout).

It never records the message text.

Strikes are held in memory and reset when the daemon restarts. `moderator_status` counts
decisions and executed actions since the daemon started. Twitch tokens are never in any of these
files. They live only in the OS credential store.
