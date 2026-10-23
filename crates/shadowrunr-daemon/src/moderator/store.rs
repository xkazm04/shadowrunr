//! `<data>/moderator/`: `rules.json` (the operator list, hot-applied on write; seeded once
//! from the Twitch hard rules of the policy file), `state.json` (the mode) and
//! `decisions.jsonl` (append-only [`shadowrunr_types::moderation::ModLogEntry`] lines).
