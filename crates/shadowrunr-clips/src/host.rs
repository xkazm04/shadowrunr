//! Who the stream's host is, as a clip names them: the words of the run's character package
//! (`[identity]`, through its persona), carried on each draft's `clip.json` so a re-render or a
//! dry-run upload names the same host. A draft written before the host was recorded gets the
//! engine's neutral host.

use serde::{Deserialize, Serialize};
use shadowrunr_types::character::Persona;

/// The caption speaker of a host line (`Speaker::Host`).
pub const HOST_SPEAKER: &str = "host";

/// Whether a caption's speaker is the host: `host`, or `malakor` on a draft stored before
/// the speaker was renamed.
pub fn is_host(speaker: &str) -> bool {
    speaker == HOST_SPEAKER || speaker == "malakor"
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipHost {
    /// Display name, e.g. "Arch-Lich Malakor": the poster's speaker line and upload text.
    pub full_name: String,
    /// Short name, e.g. "Malakor": the burnt-in speaker label, the caption style and a tag.
    pub name: String,
    /// What the host calls one viewer, e.g. "minion" ("A MINION ASKS").
    pub follower: String,
}

impl From<&Persona> for ClipHost {
    fn from(p: &Persona) -> Self {
        Self {
            full_name: p.full_name.clone(),
            name: p.name.clone(),
            follower: p.follower.clone(),
        }
    }
}

impl Default for ClipHost {
    fn default() -> Self {
        Self::from(&Persona::default())
    }
}

impl ClipHost {
    /// The caption style of the host's lines in an ASS track: the short name without the
    /// characters ASS gives meaning to (`,` separates fields), else `Host`.
    pub fn style(&self) -> String {
        let s: String = self
            .name
            .chars()
            .filter(|c| !matches!(c, ',' | '{' | '}' | '\\' | '\n' | '\r'))
            .collect();
        let s = s.trim();
        if s.is_empty() || s == "Other" {
            "Host".into()
        } else {
            s.to_string()
        }
    }

    /// The label over a viewer's question, e.g. "A MINION ASKS".
    pub fn viewer_asks(&self) -> String {
        format!("A {} ASKS", self.follower.to_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_named_from_the_persona_and_neutral_by_default() {
        let h = ClipHost::default();
        assert_eq!((h.name.as_str(), h.full_name.as_str()), ("Host", "Host"));
        assert_eq!(h.viewer_asks(), "A VIEWER ASKS");
        assert_eq!(h.style(), "Host");
        let odd = ClipHost {
            name: "Fen, the {Bold}".into(),
            ..ClipHost::default()
        };
        assert_eq!(odd.style(), "Fen the Bold");
        assert!(is_host("host") && is_host("malakor") && !is_host("the-disruptor"));
    }
}
