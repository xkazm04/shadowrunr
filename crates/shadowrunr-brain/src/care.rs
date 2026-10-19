//! Care state (WP16): after a self-harm disclosure the viewer is "in care" for a long window.
//! While any care state is active, every per-turn block carries a care note naming who is in
//! care and that they are not to be roasted or joked about, nor what they shared. The brain
//! keeps this state, not the model's memory: the night tournament saw a disclosure handled
//! well and the same viewer roasted about its detail ~165 s later.
//!
//! A new disclosure from the same viewer restarts their window. When it ends, roasting that
//! viewer is allowed again, never required. The state lives in memory for one stream (the
//! brain persists no per-stream state beyond its episodes).

use shadowrunr_types::TsMs;

use crate::util::one_line;

/// How long a viewer stays in care after their latest disclosure (30 minutes).
pub const DEFAULT_CARE_WINDOW_S: u64 = 1_800;

/// The input moderation rule recording care states in the transcript: action `started`,
/// `restarted` or `expired`, on the disclosure's message id.
pub const CARE_RULE: &str = "care_state";

/// One viewer in care.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CareEntry {
    pub viewer_id: String,
    /// Display name at the latest disclosure.
    pub name: String,
    /// Message id of the latest disclosure (the transcript anchor of the care events).
    pub msg_id: String,
    /// Time of the latest disclosure; the window runs from here.
    pub since_ms: TsMs,
}

/// What [`CareLedger::begin`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CareStart {
    Started,
    /// The viewer was in care already; the window starts again.
    Restarted,
}

impl CareStart {
    pub fn as_str(self) -> &'static str {
        match self {
            CareStart::Started => "started",
            CareStart::Restarted => "restarted",
        }
    }
}

/// The viewers in care, oldest disclosure first.
#[derive(Debug, Clone)]
pub struct CareLedger {
    window_ms: i64,
    entries: Vec<CareEntry>,
}

impl CareLedger {
    /// A ledger whose care states last `window_s` seconds. 0 ends each one at the next beat,
    /// before any turn is composed (care off; not recommended).
    pub fn new(window_s: u64) -> Self {
        Self {
            window_ms: i64::try_from(window_s.saturating_mul(1_000)).unwrap_or(i64::MAX),
            entries: Vec::new(),
        }
    }

    /// Put a viewer in care from `now_ms`, or restart their window.
    pub fn begin(&mut self, viewer_id: &str, name: &str, msg_id: &str, now_ms: TsMs) -> CareStart {
        let entry = CareEntry {
            viewer_id: viewer_id.to_string(),
            name: name.to_string(),
            msg_id: msg_id.to_string(),
            since_ms: now_ms,
        };
        let restarted = match self.entries.iter().position(|e| e.viewer_id == viewer_id) {
            Some(i) => {
                self.entries.remove(i);
                CareStart::Restarted
            }
            None => CareStart::Started,
        };
        self.entries.push(entry);
        restarted
    }

    /// End every care state whose window has run out at `now_ms`, returning them.
    pub fn expire(&mut self, now_ms: TsMs) -> Vec<CareEntry> {
        let window = self.window_ms;
        let (ended, kept): (Vec<CareEntry>, Vec<CareEntry>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|e| now_ms.saturating_sub(e.since_ms) >= window);
        self.entries = kept;
        ended
    }

    /// Viewers in care, oldest disclosure first.
    pub fn active(&self) -> &[CareEntry] {
        &self.entries
    }

    pub fn contains(&self, viewer_id: &str) -> bool {
        self.entries.iter().any(|e| e.viewer_id == viewer_id)
    }
}

/// The per-turn care note while anyone is in care, `None` otherwise. Names are viewer text:
/// flattened to one line and capped, as everywhere outside the chat fence.
pub fn care_note(entries: &[CareEntry], now_ms: TsMs) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let who: Vec<String> = entries
        .iter()
        .map(|e| {
            let min = now_ms.saturating_sub(e.since_ms).max(0) / 60_000;
            format!(
                "{} (id {}, {min} min ago)",
                one_line(&e.name, 40),
                one_line(&e.viewer_id, 60)
            )
        })
        .collect();
    Some(format!(
        "CARE, NO ROASTING: {} told chat they may harm themselves. While this note stands, never \
         roast, tease, mock or joke about them or anything they shared, and never bring up what \
         they shared. Do not allude to them either: not their absence, their empty seat or \
         their removal from chat, not even inside a roast of someone else. If they speak, \
         answer warmly and briefly, or leave them be. Never ask them where they are or for any \
         detail about them.",
        who.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_care_state_lasts_its_window_and_a_new_disclosure_restarts_it() {
        let mut c = CareLedger::new(DEFAULT_CARE_WINDOW_S);
        assert_eq!(c.begin("v1", "Soot", "m1", 1_000), CareStart::Started);
        assert!(c.contains("v1"));
        // 29 minutes on: still in care.
        assert!(c.expire(1_000 + 29 * 60_000).is_empty());
        // A new disclosure at 29 min restarts the 30-minute window.
        assert_eq!(
            c.begin("v1", "Soot", "m2", 1_000 + 29 * 60_000),
            CareStart::Restarted
        );
        assert_eq!(c.active().len(), 1);
        assert!(
            c.expire(1_000 + 31 * 60_000).is_empty(),
            "restarted at 29 min"
        );
        let ended = c.expire(1_000 + 59 * 60_000);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].msg_id, "m2");
        assert!(!c.contains("v1"));
        assert!(care_note(c.active(), 0).is_none());
    }

    #[test]
    fn the_care_note_names_everyone_in_care() {
        let mut c = CareLedger::new(60);
        c.begin("v1", "Soot\nGoblin", "m1", 0);
        c.begin("v2", "Moth", "m2", 30_000);
        let n = care_note(c.active(), 150_000).unwrap();
        assert!(n.contains("Soot Goblin (id v1, 2 min ago)"), "{n}");
        assert!(n.contains("Moth (id v2, 2 min ago)"), "{n}");
        assert!(n.contains("never roast"));
        assert!(n.contains("Never ask them where they are"));
        assert_eq!(c.expire(60_000).len(), 1, "v1's minute is up");
        assert!(c.contains("v2"));
    }
}
