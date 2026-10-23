# The moderator goes live on Twitch (deferred)

The chat moderator is built and on main (`5b69f42..64439a3`, 2026-10-23), but it has never talked to the real Twitch. Its tests ran against local mock servers, and the smoke run used the scripted engine with Twitch off. This note holds what is left to put it on a real channel, so the follow-up can start from here. How it works is in [docs/guide/moderator.md](../guide/moderator.md), and the wire contract is in [docs/spec/contract.md](../spec/contract.md) ("Appendix (moderation)").

Sizes are as in [leftovers.md](leftovers.md): **S** is focused, **M** takes about a day, **L** needs design first.

## The owner's part (one sitting)

1. ~~Pick the moderator's look~~. Done 2026-10-23: **Clerk-Bailiff Vellum**, the Ledger Clerk look of the Deadpan Bailiff (`characters/moderator/`). The console face is `apps/console/src/pages/characters/art/vellum.webp`. The realistic render for a future stage avatar is kept locally in `data/avatar/portraits/mod-A2/portrait.jpg`. Name the bot account after it (for example `ClerkBailiffVellum`).
2. **Register a Twitch developer app** at dev.twitch.tv:
   - client type **Public** (device-code flow, so no secret and no redirect server);
   - copy its client id.
3. **Create the bot account** and make it a moderator of the channel (`/mod <bot>` from the broadcaster).
4. **Configure** `shadowrunr.toml`:
   ```toml
   [twitch]
   client_id = "<the app's client id>"   # public, not a secret
   channel = "<broadcaster login>"
   ```
   Restart the daemon: config is read once at start.
5. **Connect.** Ask Claude Code (with `claude mcp add shadowrunr -- shadowrunr mcp` and `[moderator] mcp_allow_writes = true`) to connect the bot, or press Connect in the console's Moderation tab. Open the URL, sign in **as the bot**, and enter the code. The token goes to Windows Credential Manager (`bot.shadowrunr-twitch`).
6. **Run one stream in shadow mode** (the default). Read the decision log, tune the rules, then arm.

## Checks for the first live connection

None of these could be checked without a real channel:

| Check | Why | Where |
|---|---|---|
| Device login completes, and the status reads `connected` with the bot login and channel | first run of the real `id.twitch.tv` flow | `platform/twitch/auth.rs` |
| `channel.chat.message` arrives, `is_mod` is set for mods and the broadcaster, and the bot's own lines are dropped | the event shape and badges are mapped from the docs, not from live traffic | `platform/twitch/eventsub.rs` |
| A subscriber badge maps to the right `sub_tier` | the 2000/3000 tier numbering is common practice; the docs do not state it | `eventsub.rs` badge mapping |
| An armed timeout, ban, delete, unban and say each succeed on the channel | Helix scopes and moderator status are in practice only proven live | `platform/twitch/helix.rs` |
| The token refresh survives a long session (refresh tokens are one-time use) | the refresh lock was only tested against a mock | `platform/twitch/mod.rs` |
| Reconnect after a dropped socket and after `session_reconnect` | as above | `eventsub.rs` |
| Classifier and voice-line latency and cost on real chat volume | the per-batch `mod_classifier` and per-action `mod_voice` calls were never measured; read them in the engine ledger | `moderator/rules.rs`, `moderator/voice.rs` |

## Open items found while building

| Item | Evidence | Size |
|---|---|---|
| **Setting the identity by chat.** `moderator_character_set` always refuses, because no RPC writes `character.toml`. It needs a `character.identity.set` RPC that keeps the file's comments (`toml_edit`). This blocks renaming the moderator by chat after the look pick. | `integrations/mcp_tools.rs` returns `not_available` | M |
| **Look-alike normalisation.** Pattern rules do not catch look-alike characters (Cyrillic а for a) or spaced letters ("k y s"). The brain's `util::normalize` only lowercases and folds separators; the lane adds stretched-letter collapsing only. The contract appendix claims more than the code does, so fix the code or the claim. | `crates/shadowrunr-brain/src/util.rs`, `moderator/rules.rs` | M |
| **Shared Chat.** Messages from another channel in a Shared Chat session reach the lane. Deleting them may fail, though banning still works. Decide whether to skip them, or to act only by ban. | `eventsub.rs` passes `source_broadcaster_user_id` messages through | S |
| **Pre-flight does not check `role`.** PF-02 is green when `[sim] character` points at the moderator, though the show start is refused. | `preflight.rs` PF-02 | S |
| **`months` from the subscriber badge.** It is left at the default, though the badge's `info` carries it. | `eventsub.rs` | S |
| **The host reading real chat.** The lane publishes screened messages (`ModeratorHub::screened()`), but nothing subscribes. Malakor still sees only simulated chat. | `moderator/mod.rs` | L |
| **Kick.** Only the `ChatPlatform` trait exists. | `platform/mod.rs` | L |
