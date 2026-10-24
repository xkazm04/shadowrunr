//! SEG-03 scene playback (contract appendix "SEG-03 Table Read").
//!
//! The segment runtime decides which scene is up and says so in the state summary
//! (`SCENE UP: <id> ...`, [`scene_up`]); the brain holds the stream's scenes
//! ([`crate::Brain::set_table_read`]) and performs the one named, over as many beats as its
//! speech takes: `scene_start`, then each line in order as a `say` (the guest's as
//! `speaker: foil`, `foil: <guest>`), then `scene_end`, which the caller hands back to the
//! runtime (`SegmentRuntime::scene_ended`).
//!
//! - **The gate.** Every line passes the output gate when it plays, the guest's on the foil
//!   surface and the host's on the spoken one. A line the gate recasts is spoken as recast; a
//!   line it blocks (dropped, or replaced for a blocking term) is skipped, never invented
//!   ([`SCENE_RULE`] / [`SKIPPED`]).
//! - **Pace.** A scene beat speaks up to the beat's speech budget (always at least one line);
//!   the orchestrator's speech hold spaces the beats, so a scene never dumps in one beat.
//! - **Live slots.** At `scene_start` every `live` host line is written by the core model, one
//!   call per slot, in order, in the background ([`spawn_prefetch`]), each prompt carrying the
//!   scene so far, the slot's direction, the guest and the honest register. A slot whose call
//!   failed, or (on a live host) is not back when its turn comes, speaks its `fallback`
//!   ([`FALLBACK`]); in simulated time the turn waits for the call, so a run is reproducible.
//! - **Not model output.** Scene lines stay out of the repetition guard and out of the host's
//!   turn record, like stall lines: the model did not write the fixed ones.
//!
//! Outside a scene the guest is the active foil for the whole segment, and its turns keep the
//! foil rules ([`guest_turns`]): one sentence a turn, and the host speaks after every one.

use std::collections::HashMap;
use std::sync::Arc;

use shadowrunr_types::scene::{Scene, SceneSpeaker};
use shadowrunr_types::{Engine, EngineRequest, StreamEvent, TsMs};
use tokio::sync::mpsc;

use crate::grammar::{parse_reply, Line, Spoken};
use crate::moderation::output_event;

/// The state summary line naming the scene to perform (the runtime's
/// `shadowrunr_segments::rounds::SCENE_UP_PREFIX`).
pub const SCENE_UP_PREFIX: &str = "SCENE UP: ";
/// The state summary line naming the active foil.
pub const ACTIVE_FOIL_PREFIX: &str = "active foil: ";
/// The output record of a scene line that did not play as written.
pub const SCENE_RULE: &str = "scene_line";
/// Its action for a line the gate blocked: skipped.
pub const SKIPPED: &str = "skipped";
/// Its action for a live slot that spoke its fallback.
pub const FALLBACK: &str = "fallback";
/// The output record of the guest's turn rules outside a scene.
pub const GUEST_TURN_RULE: &str = "guest_turn";
/// Its action for a guest line cut to its first sentence.
pub const TRIMMED: &str = "trimmed";
/// Its action for a guest line no host line followed: dropped (the host has the last word).
pub const LAST_WORD: &str = "last_word";

/// The scene the state summary puts up, by id.
pub fn scene_up(summary: &str) -> Option<&str> {
    summary
        .lines()
        .find_map(|l| l.trim_start().strip_prefix(SCENE_UP_PREFIX))
        .and_then(|rest| rest.split_whitespace().next())
}

/// The active foil the state summary names.
pub fn summary_foil(summary: &str) -> Option<&str> {
    summary
        .lines()
        .find_map(|l| l.trim_start().strip_prefix(ACTIVE_FOIL_PREFIX))
        .map(str::trim)
}

/// A scene being performed.
pub(crate) struct Playing {
    pub scene: Scene,
    /// The next line to play.
    pub next: usize,
    /// Live slot results by line index: `Some(text)`, or `None` when the call failed.
    live: HashMap<usize, Option<String>>,
    rx: Option<mpsc::UnboundedReceiver<(usize, Option<String>)>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Playing {
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            next: 0,
            live: HashMap::new(),
            rx: None,
            task: None,
        }
    }

    pub fn with_prefetch(
        mut self,
        rx: mpsc::UnboundedReceiver<(usize, Option<String>)>,
        task: tokio::task::JoinHandle<()>,
    ) -> Self {
        self.rx = Some(rx);
        self.task = Some(task);
        self
    }

    /// The results back so far.
    fn drain(&mut self) {
        if let Some(rx) = self.rx.as_mut() {
            while let Ok((i, r)) = rx.try_recv() {
                self.live.insert(i, r);
            }
        }
    }

    /// Live slot `i`'s text: `Some(Some(text))` when it came back, `Some(None)` when its call
    /// failed, `None` when it is not back. With `wait` (simulated time) it waits for it.
    pub async fn live_text(&mut self, i: usize, wait: bool) -> Option<Option<String>> {
        self.drain();
        if let Some(r) = self.live.get(&i) {
            return Some(r.clone());
        }
        if !wait {
            return None;
        }
        let rx = self.rx.as_mut()?;
        while let Some((k, r)) = rx.recv().await {
            self.live.insert(k, r);
            if k == i {
                break;
            }
        }
        // A prefetch that ended without this slot failed it.
        Some(self.live.get(&i).cloned().flatten())
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        // A slot nobody will speak costs nothing more.
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

/// Who speaks a scene line, as the transcript and the prompts name them.
fn speaker_name<'a>(s: SceneSpeaker, host: &'a str, guest: &'a str) -> &'a str {
    match s {
        SceneSpeaker::Host => host,
        SceneSpeaker::Guest => guest,
    }
}

/// The prompt for live slot `i` of `scene`: the scene so far (`so_far`: speaker and text, in
/// order), the slot's direction, the guest and the honest register.
pub fn slot_prompt(
    scene: &Scene,
    so_far: &[(String, String)],
    direction: &str,
    host: &str,
    guest: &str,
    max_chars: usize,
) -> String {
    let mut p = format!(
        "# Table Read: your live line\nYou and tonight's guest, {guest}, are performing the scene \
         \"{}\" on air ({}).\n",
        scene.title, scene.pitch
    );
    if so_far.is_empty() {
        p.push_str("The scene opens with your line.\n");
    } else {
        p.push_str("The scene so far:\n");
        for (who, text) in so_far {
            p.push_str(&format!("{who}: {text}\n"));
        }
    }
    p.push_str(&format!(
        "\nYour next line, {host}, in your own voice: {direction}.\n\
         Say it in the honest register: roast yourself and own your flops; say the unsaid, \
         bluntly (the scene is bad, the guest is bombing, chat is lying to you); give your real \
         opinion of the material. Never about how the show is made.\n\
         One or two sentences, at most {max_chars} characters, spoken by you alone: no FOIL, OP \
         or NOTE lines and no stage directions. Reply with the line only."
    ));
    p
}

/// The spoken text of a live slot's reply: its first one or two spoken lines, at most
/// `max_chars` (the first alone when two run over). `None` when it says nothing.
pub fn slot_text(reply: &str, prefixes: &[String], max_chars: usize) -> Option<String> {
    let said: Vec<String> = parse_reply(reply, &[], None, prefixes)
        .into_iter()
        .filter_map(|l| match l {
            Line::Say(Spoken { text, .. }) => Some(text),
            _ => None,
        })
        .take(2)
        .collect();
    let first = said.first()?.clone();
    let both = said.join(" ");
    Some(if both.chars().count() <= max_chars {
        both
    } else {
        first
    })
}

/// A running prefetch: its results, `(line index, text or None)`, and its task.
pub(crate) type Prefetch = (
    mpsc::UnboundedReceiver<(usize, Option<String>)>,
    tokio::task::JoinHandle<()>,
);

/// Write `scene`'s live slots in the background, one core call per slot, in order. Each
/// prompt carries the lines before the slot (an earlier slot's text as it came back, else its
/// fallback). `None` for a scene with no live slot.
pub(crate) fn spawn_prefetch(
    engine: Arc<dyn Engine>,
    template: EngineRequest,
    scene: Scene,
    host: String,
    guest: String,
    prefixes: Vec<String>,
    max_chars: usize,
) -> Option<Prefetch> {
    if !scene.lines.iter().any(|l| l.live.is_some()) {
        return None;
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let mut so_far: Vec<(String, String)> = Vec::new();
        for (i, line) in scene.lines.iter().enumerate() {
            let who = speaker_name(line.speaker, &host, &guest).to_string();
            let text = match (&line.text, &line.live) {
                (Some(t), _) => t.clone(),
                (None, Some(direction)) => {
                    let mut req = template.clone();
                    req.prompt = slot_prompt(&scene, &so_far, direction, &host, &guest, max_chars);
                    let got = match engine.complete(req).await {
                        Ok(r) => slot_text(&r.text, &prefixes, max_chars),
                        Err(_) => None,
                    };
                    if tx.send((i, got.clone())).is_err() {
                        return;
                    }
                    got.or_else(|| line.fallback.clone()).unwrap_or_default()
                }
                _ => continue,
            };
            so_far.push((who, text));
        }
    });
    Some((rx, task))
}

/// The scene event that opens a performance.
pub fn scene_start(now_ms: TsMs, scene: &Scene, guest: &str) -> StreamEvent {
    StreamEvent::SceneStart {
        ts_ms: now_ms,
        scene_id: scene.id.clone(),
        title: scene.title.clone(),
        guest: guest.to_string(),
    }
}

/// Whether the gate's records say a blocking term replaced the line: in a scene that is a
/// block (the canned replacement is not the scene's line).
pub fn blocked_by_term(events: &[StreamEvent]) -> bool {
    events.iter().any(|e| {
        matches!(e, StreamEvent::Moderation { rule, action, .. }
            if rule.starts_with("term:") && action == "replaced")
    })
}

/// The first sentence of `text` (all of it when it has one).
pub fn first_sentence(text: &str) -> &str {
    let t = text.trim();
    let mut prev_end = false;
    for (i, c) in t.char_indices() {
        if prev_end && c.is_whitespace() {
            return t[..i].trim_end();
        }
        prev_end = matches!(c, '.' | '!' | '?') && i > 0;
    }
    t
}

/// The guest's turn rules outside a scene: each of `guest`'s lines is cut to its first
/// sentence, and a guest line with no host line after it (before the next guest line or the
/// end of the reply) is dropped, so the host speaks after every guest turn. Each change leaves
/// a [`GUEST_TURN_RULE`] record.
pub fn guest_turns(lines: &mut Vec<Line>, guest: &str, now_ms: TsMs, ev: &mut Vec<StreamEvent>) {
    for l in lines.iter_mut() {
        if let Line::Foil { foil, text } = l {
            if foil == guest {
                let first = first_sentence(text).to_string();
                if first != text.trim() {
                    *text = first;
                    ev.push(output_event(now_ms, GUEST_TURN_RULE, TRIMMED));
                }
            }
        }
    }
    let is_guest = |l: &Line| matches!(l, Line::Foil { foil, .. } if foil == guest);
    let mut drop: Vec<usize> = Vec::new();
    for (k, l) in lines.iter().enumerate() {
        if !is_guest(l) {
            continue;
        }
        let answered = lines[k + 1..]
            .iter()
            .take_while(|m| !is_guest(m))
            .any(|m| matches!(m, Line::Say(_)));
        if !answered {
            drop.push(k);
        }
    }
    for &k in drop.iter().rev() {
        lines.remove(k);
        ev.push(output_event(now_ms, GUEST_TURN_RULE, LAST_WORD));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_names_the_scene_and_the_foil() {
        let s = "segment SEG-03 (Table Read), state scene\nactive foil: the-painter\n\
                 SCENE UP: the-gallery \"The Gallery\" (chosen by the ballot): ...";
        assert_eq!(scene_up(s), Some("the-gallery"));
        assert_eq!(summary_foil(s), Some("the-painter"));
        assert_eq!(scene_up("state talk"), None);
    }

    #[test]
    fn first_sentences() {
        assert_eq!(first_sentence("Behold. My masterpiece."), "Behold.");
        assert_eq!(first_sentence("What?! No way."), "What?!");
        assert_eq!(first_sentence("A single line"), "A single line");
    }

    #[test]
    fn a_guest_line_with_no_host_line_after_it_is_dropped_and_long_ones_cut() {
        let foil = |t: &str| Line::Foil {
            foil: "the-painter".into(),
            text: t.into(),
        };
        let say = |t: &str| Line::Say(Spoken::plain(t));
        let mut lines = vec![
            foil("I am a genius. Truly."),
            say("You are not."),
            foil("Silence!"),
            foil("I said silence."),
            say("Fine."),
            foil("Ha."),
        ];
        let mut ev = Vec::new();
        guest_turns(&mut lines, "the-painter", 0, &mut ev);
        let kept: Vec<String> = lines
            .iter()
            .map(|l| match l {
                Line::Foil { text, .. } => format!("G {text}"),
                Line::Say(s) => format!("H {}", s.text),
                _ => String::new(),
            })
            .collect();
        assert_eq!(
            kept,
            [
                "G I am a genius.",
                "H You are not.",
                "G I said silence.",
                "H Fine."
            ]
        );
        let actions: Vec<String> = ev
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Moderation { action, .. } => Some(action.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(actions, [TRIMMED, LAST_WORD, LAST_WORD]);
    }

    #[test]
    fn a_slot_reply_keeps_its_spoken_lines() {
        let p = vec!["Malakor:".to_string()];
        assert_eq!(
            slot_text("Malakor: I bombed.\nOP: {\"op\":\"sign_off\"}", &p, 200).as_deref(),
            Some("I bombed.")
        );
        assert_eq!(
            slot_text("One line.\nTwo line.", &p, 200).as_deref(),
            Some("One line. Two line.")
        );
        assert_eq!(
            slot_text("One line.\nTwo line.", &p, 12).as_deref(),
            Some("One line.")
        );
        assert_eq!(slot_text("NOTE: nothing", &p, 200), None);
    }
}
