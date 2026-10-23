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

## Connecting Twitch

The moderator acts from its own Twitch account (the bot), not the broadcaster's. The bot reads
chat over EventSub WebSocket and deletes, times out, bans and speaks through the Helix API. It
logs in with Twitch's device-code flow, so there is no client secret and no redirect server. Its
tokens live only in Windows Credential Manager.

### 1. Create the bot account and make it a moderator

1. Create a separate Twitch account for the bot, for example `yourchannel_mod`. Its login is
   the name viewers see on the moderator's lines.
2. In the broadcaster's chat, type `/mod yourchannel_mod`. Twitch accepts a delete, timeout
   or ban only from a moderator of the channel. Without this step every action fails with
   HTTP 403.

### 2. Register a Twitch developer application

1. Sign in at <https://dev.twitch.tv/console> as either account. Twitch requires two-factor
   authentication on the account before it lets you register an app.
2. Go to **Applications**, then **Register Your Application**.
   - **Name**: any name that is unique on Twitch, for example `yourchannel-moderator`. The bot
     sees this name when it approves the login.
   - **OAuth Redirect URLs**: the form requires one. Enter `http://localhost`. The device flow
     never uses it.
   - **Category**: Chat Bot.
   - **Client Type**: **Public**. A public client logs in with the device flow and refreshes
     without a secret. Do not create a client secret: Shadowrunr never uses one.
3. Open **Manage** and copy the **Client ID**. A client ID is public. It names the app, not a
   user, so it may sit in a config file.

### 3. Configure `shadowrunr.toml`

```toml
[twitch]
client_id = "your-client-id"   # public; an empty client_id keeps Twitch off
channel   = "yourchannel"      # the broadcaster's login: the channel the bot guards
```

Restart the daemon. Until `client_id` is set, `moderator.status` reports Twitch as
`disconnected` with the detail "no [twitch] client_id configured".

### 4. Log the bot in (device code)

1. Call `twitch_connect` from your Claude Code (the `shadowrunr mcp` tool), or press
   **Connect Twitch** on the console's Moderation tab. It returns a `verification_uri` and a
   `user_code`. While it waits, the status is `connecting` with the detail
   "waiting for the code at <uri>".
2. Open the URI in a browser **signed in as the bot account** (a private window is the easy
   way). Enter the code if Twitch asks for it, then approve.
3. The daemon polls Twitch at the interval Twitch asks for. Once you approve, the daemon stores
   the tokens, checks who the bot is, finds the channel, opens EventSub, and goes to
   `connected` with `bot_login` set. The code expires after `expires_s` seconds (Twitch gives
   30 minutes). After that, run `twitch_connect` again.

The bot asks for these scopes, and only these:

| Scope | What it allows |
|---|---|
| `user:read:chat` | Read the channel's chat as the bot (EventSub `channel.chat.message`). |
| `user:write:chat` | Post the moderator's lines as the bot. |
| `moderator:manage:chat_messages` | Delete one message. |
| `moderator:manage:banned_users` | Time out, ban and unban. |

### Where the token lives

- The token is a generic credential in **Windows Credential Manager**:
  - service `shadowrunr-twitch`, account `bot`;
  - Credential Manager shows it as the target `bot.shadowrunr-twitch`;
  - value: a small JSON holding the access token, the refresh token, the bot's login and the
    bot's user id.
- To see it, open **Control Panel**, then **Credential Manager**, then **Windows
  Credentials**, and look under **Generic Credentials**.
- The token is never written to `shadowrunr.toml`, a log, an error, the console or an RPC
  result.
- An access token lasts about four hours. The daemon validates it on start and every hour, and
  refreshes it on its own.
- A public client's refresh token can be used once, and it expires after 30 days without use.
  If the daemon has not run for a month, log in again.

### Disconnecting and revoking

- `twitch_disconnect` (or **Disconnect** on the Moderation tab) does four things. It revokes
  the access token at Twitch, deletes the Credential Manager entry, closes the chat socket, and
  sets the status to `disconnected`.
- To cut the bot off from Twitch's side as well, sign in as the bot and open
  <https://www.twitch.tv/settings/connections>. Find your app under **Other Connections** and
  click **Disconnect**. This kills every token the app holds for the bot, including copies you
  do not know about.
- To remove its moderator rights, type `/unmod yourchannel_mod` in the broadcaster's chat.
- To remove the credential by hand, delete `bot.shadowrunr-twitch` from Credential Manager, or
  run `cmdkey /delete:bot.shadowrunr-twitch`.

### Troubleshooting

| Status or error | Cause and fix |
|---|---|
| `disconnected`, "no [twitch] client_id configured" | Set `[twitch] client_id` and restart the daemon. |
| `disconnected`, "not logged in; run twitch.connect" | There is no stored credential. Run `twitch_connect`. |
| `error`, "no [twitch] channel configured" | Set `[twitch] channel` to the broadcaster's login. |
| `error`, "twitch channel X not found" | The `channel` login is wrong. Use the login, not the display name. |
| `error`, "the Twitch code expired; run twitch.connect again" | The code was not entered within its lifetime. Run it again. |
| `error`, "twitch login failed: ..." | Twitch refused the login: the request was denied, or the code had been used already. Run `twitch_connect` again and approve it as the bot. |
| `error`, "the Twitch login expired or was revoked; run twitch.connect again" | The refresh token is dead: 30 days unused, the connection was removed in the bot's settings, or the bot's password was changed. Log in again. |
| `error`, "twitch revoked the chat subscription (...)" | Twitch withdrew the chat subscription: `authorization_revoked`, `user_removed` or `version_removed`. Log in again. If it repeats, check that the bot account still exists and is not suspended. |
| `connecting`, "reconnecting: ..." | The chat socket dropped. The daemon retries with backoff of up to a minute and resubscribes. Check the network if it does not settle. |
| Actions fail with "HTTP 403" | The bot is not a moderator of the channel. Run `/mod <bot>` in the broadcaster's chat. |
| A delete fails with "HTTP 400" | Twitch does not let anyone delete a moderator's or the broadcaster's messages. The lane never acts on them anyway. |
| A delete fails with "HTTP 404" | The message is gone, or is more than six hours old. |
| A ban fails with "HTTP 400 ... already banned" | The viewer is already banned. A banned viewer cannot be timed out; unban them first. |
| "rate limited ... resets in Ns" | Twitch's rate-limit bucket is empty. Calls fail fast until the reset so the lane never stalls. They resume on their own. |
| "say: dropped: ..." | Twitch accepted the line but did not post it. The reason comes from Twitch, for example AutoMod or followers-only mode. |
| `cmdkey` or Credential Manager shows no entry after connecting | The daemon could not write to Credential Manager. The status detail names the store error. Run the daemon as the Windows user who owns the credential store. |

## The moderator character

The moderator is a second character package, `characters/moderator/`, with
`role = "moderator"` in its `character.toml`. It never hosts a show. It gives the chat
moderator a name, a voice and a constitution, and each decision's chat line is written from
them. `[moderator] character` in `shadowrunr.toml` names the package (default `moderator`).

### What the package holds

| File | What it is |
| --- | --- |
| `character.toml` | `id`, `name`, `role = "moderator"`, `tagline`, and `[identity]` (name, title, patron, follower words, sign-off, disclosure). `default_tone = "none"` is there only because the manifest schema requires the key. A moderator has no tones and no file is read for it. |
| `constitution.md` | The moderator's rules. Every line prompt carries the whole file. It is terse and procedural, the straight man to the host. It never repeats, quotes or decodes the removed message, names only the rule, never threatens beyond the action taken, never mocks a protected group, never addresses a self-disclosure, and writes one line under 200 characters with no emojis and no @-mention except the actioned viewer. |
| `bible.md` | The fiction and the look: Clerk-Bailiff Vellum of the Ninth Dark, its Ledger and its Writs. The line prompt does not read it today; it is canon for whoever writes or renames the character. |
| `moderator.toml` | `fallback`: the model-free line (see below). |

A moderator package needs no `tones/`, `foils/`, `segments/`, `rules/`, rails, ranks or
tuning. `shadowrunr character validate characters/moderator` checks it like any other package.
It also checks that `moderator.toml` exists, that `fallback` is one non-empty line, and that it
uses only the `{viewer}` and `{rule}` placeholders. The package shows up in `character.list` and
`character.get` so the console can see it.

**A moderator never hosts a show.** Any show started with `character = "moderator"` is refused
before a run file is written: a sim run, a sandbox session or a stream. The error says
"... is a moderator package (role = "moderator"): it guards chat and cannot host a show". Point
`[sim] character` at a host character instead.

### Renaming it

The identity is **Clerk-Bailiff Vellum**: the Ledger Clerk look of the Deadpan Bailiff, the
owner's pick on 2026-10-23. Its 2D illustration is the console face
(`apps/console/src/pages/characters/art/vellum.webp`). The realistic render is the modelling
reference for a future stage avatar, kept locally in
`data/avatar/portraits/mod-A2/portrait.jpg`, which is not committed. To rename it:

1. In `character.toml`, set `name` (the display name, for example `"The Bailiff"`),
   `[identity] name` (the short name chat sees, for example `"Bailiff"`), and `title` if
   wanted. Rewrite `sign_off` and `disclosure` in the new voice. The disclosure must stay an
   honest "I am an AI character" line.
2. Change the name in `constitution.md` and `bible.md` to the new one, and change the
   fiction to fit the look if needed.
3. Reword `moderator.toml` `fallback` if it names the old character.
4. Keep `id = "moderator"` and the directory name. If you change them, set
   `[moderator] character` in `shadowrunr.toml` to the new id.
5. Run `shadowrunr character validate <path to the package>` and restart the daemon. The
   moderator loads its package at start, and `moderator.status` reports a package that did not
   load.

### The fallback line

`moderator.toml`:

```toml
fallback = "{viewer}, your message is struck from the record under the rule: {rule}. Vellum has entered it in the ledger."
```

`{viewer}` is the actioned viewer's display name and `{rule}` is the rule's name. The fallback
is posted (or logged, in shadow) in place of a model line in these cases:

- the model call fails or times out (the `mod_voice` route's timeout);
- the reply is empty;
- the output gate changes the reply in any way (only a warning passes);
- the reply breaks the constitution's form: an emoji, or an @-mention of anyone but the viewer.

The fallback goes through the same gate. If the viewer's name is what trips it, the follower
word ("minion") takes the name's place. If the line still does not pass, the moderator posts
"A message was removed under the channel rules.", which names nobody.

If the package is missing or does not load, the moderator uses a neutral voice named
"Moderator" with a generic fallback, and `moderator.status` carries the error.

### How the line prompt works

Every decision makes one `ModVoice` engine call: the `mod_voice` route (Haiku, low effort,
15 s by default, overridable under `[engine]` routes), with no tools and no JSON schema. Its
correlation id is `mod:<decision id>`.

- **System (stable, cached):** the constitution in full, then an identity block (the full and
  short name, the patron, the follower word, and the honest disclosure to use if asked), then
  the output form: one line, no speaker label, no quotes, under 200 characters.
- **Prompt (per decision):** the viewer's display name; the action (deleted; deleted and timed
  out for N minutes; or deleted and banned); the rule's name; and the mode (armed: posted;
  shadow: logged only). It ends with the ask: one line addressed to the viewer, naming only the
  rule, without guessing at the message.

**The viewer's message text never enters the prompt.** A decision has no text field, so the
moderator cannot quote what it never saw.

The reply is cleaned before it is gated. The moderator keeps the first non-empty line, strips
a leading speaker label (the persona's `reply_prefixes`, or `Name:` / `**Name:**` for the short
or full name) and wrapping quotes, and caps the line at 300 characters. Then it runs the brain's
output gate (`gate_text`) with the platform rules and a `PersonaGate` built from the
moderator's own identity. Anything the gate would change sends the fallback instead. The
action never waits on the line.

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
