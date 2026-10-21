//! `GET /voice`: one WebSocket session per client.
//!
//! - On connect the client gets the current `status`, and every change after it.
//! - It sends `{"follow": {"run_id"?}}` to choose a live run; each message replaces the last.
//!   Without `run_id` it follows the newest live run and moves to each run as it starts; a named
//!   run that is not live yet is picked up when it starts. Nothing is followed before the first
//!   `follow`. Replays are never voiced: only runs in the `LiveRegistry` are followed.
//! - The stage also reports on this socket (`played`, `stage_dropped`, `face`; contract
//!   `VoiceClientFrame`). Each goes to its run's `voice.jsonl` (`log`) and is never answered;
//!   a report never changes the follow.
//! - **Malformed client frames are ignored and logged** (one stderr line, the frame cut to 120
//!   characters). The socket carries only contract frames (`status`, `utterance`, `skipped`),
//!   and the current follow stays as it was.
//! - **A slow client loses frames; it never slows the service.** Frames go through a bounded
//!   per-socket queue ([`SOCKET_QUEUE`]) with `try_send`. When an utterance does not fit, it and
//!   the rest of its line are dropped, and the client is owed one
//!   `skipped {run_id, seq, part, reason: queue_full}` from the first dropped part, sent as soon
//!   as the queue has room (the session waits for room while anything is owed, so owed frames
//!   go out without waiting for the next frame). A status change that does not fit is kept (the newest wins) and sent
//!   first when there is room.

use std::collections::VecDeque;
use std::sync::Arc;

use axum::extract::ws::{close_code, CloseFrame, Message, Utf8Bytes, WebSocket};
use futures::{SinkExt, StreamExt};
use shadowrunr_types::voice::{VoiceClientFrame, VoiceFrame, VoiceSkip};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use super::service::{Following, VoiceHub, VoiceItem};
use crate::events::SHUTDOWN_REASON;
use crate::live::LiveRun;
use crate::server::AppState;

/// Frames queued for one socket. An utterance is a sentence of base64 PCM (about 64 KB a
/// second of audio), so this holds a few lines' worth.
pub const SOCKET_QUEUE: usize = 16;
/// Owed `skipped` frames kept for a stuck client; past it the oldest is forgotten.
const OWED_MAX: usize = 64;

pub enum Out {
    Text(Arc<str>),
    Close,
}

/// The per-socket queue and what the client is owed when it has room again.
pub struct Outbox {
    tx: mpsc::Sender<Out>,
    status: Option<Arc<str>>,
    owed: VecDeque<VoiceItem>,
    /// Lines whose rest is being dropped: (run_id, seq).
    dropping: VecDeque<(String, u64)>,
    closed: bool,
}

impl Outbox {
    pub fn new(tx: mpsc::Sender<Out>) -> Self {
        Self {
            tx,
            status: None,
            owed: VecDeque::new(),
            dropping: VecDeque::new(),
            closed: false,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Whether frames wait for room in the queue.
    pub fn owes(&self) -> bool {
        self.status.is_some() || !self.owed.is_empty()
    }

    /// Try to queue one frame; false when the queue is full.
    fn try_send(&mut self, json: &Arc<str>) -> bool {
        match self.tx.try_send(Out::Text(json.clone())) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Closed(_)) => {
                self.closed = true;
                false
            }
        }
    }

    /// Send what is owed, oldest first, while there is room.
    pub fn flush(&mut self) {
        if let Some(s) = self.status.clone() {
            if !self.try_send(&s) {
                return;
            }
            self.status = None;
        }
        while let Some(item) = self.owed.front().cloned() {
            if !self.try_send(&item.json) {
                return;
            }
            self.owed.pop_front();
        }
    }

    pub fn status(&mut self, item: &VoiceItem) {
        self.flush();
        if self.status.is_some() || !self.try_send(&item.json) {
            self.status = Some(item.json.clone());
        }
    }

    fn owe(&mut self, item: VoiceItem) {
        if self.owed.len() == OWED_MAX {
            self.owed.pop_front();
        }
        self.owed.push_back(item);
    }

    fn dropping(&self, run_id: &str, seq: u64) -> bool {
        self.dropping.iter().any(|(r, s)| r == run_id && *s == seq)
    }

    fn stop_dropping(&mut self, run_id: &str, seq: u64) {
        self.dropping.retain(|(r, s)| !(r == run_id && *s == seq));
    }

    /// One utterance or skipped frame of the followed run.
    pub fn frame(&mut self, item: VoiceItem) {
        self.flush();
        match &*item.frame {
            VoiceFrame::Utterance {
                run_id,
                seq,
                part,
                last,
                ..
            } => {
                if self.dropping(run_id, *seq) {
                    if *last {
                        self.stop_dropping(run_id, *seq);
                    }
                    return;
                }
                if !self.owed.is_empty() || !self.try_send(&item.json) {
                    if !*last {
                        if self.dropping.len() == OWED_MAX {
                            self.dropping.pop_front();
                        }
                        self.dropping.push_back((run_id.clone(), *seq));
                    }
                    self.owe(VoiceItem::new(VoiceFrame::Skipped {
                        run_id: run_id.clone(),
                        seq: *seq,
                        part: *part,
                        reason: VoiceSkip::QueueFull,
                    }));
                }
            }
            VoiceFrame::Skipped { run_id, seq, .. } => {
                // A line already skipped for this client from an earlier part says no more.
                if self.dropping(run_id, *seq) {
                    self.stop_dropping(run_id, *seq);
                    return;
                }
                if !self.owed.is_empty() || !self.try_send(&item.json) {
                    self.owe(item);
                }
            }
            VoiceFrame::Status { .. } => self.status(&item),
        }
    }
}

/// What the client chose.
#[derive(Debug, Clone, PartialEq)]
enum Follow {
    Newest,
    Run(String),
}

/// What a client frame is: a follow, a stage report, or nothing this socket knows.
#[derive(Debug, Clone, PartialEq)]
enum Client {
    Follow(Follow),
    Report(VoiceClientFrame),
}

fn parse_client(text: &str) -> Option<Client> {
    Some(match serde_json::from_str::<VoiceClientFrame>(text).ok()? {
        VoiceClientFrame::Follow(f) => Client::Follow(match f.follow.run_id {
            Some(id) if !id.trim().is_empty() => Follow::Run(id),
            _ => Follow::Newest,
        }),
        report => Client::Report(report),
    })
}

/// The follow a client frame asks for; `None` for anything that is not a `follow` frame.
#[cfg(test)]
fn parse_follow(text: &str) -> Option<Follow> {
    match parse_client(text)? {
        Client::Follow(f) => Some(f),
        Client::Report(_) => None,
    }
}

/// The live run `follow` points at now, if any.
fn target(state: &AppState, follow: &Follow) -> Option<Arc<LiveRun>> {
    match follow {
        Follow::Newest => state.live.latest().borrow().clone(),
        Follow::Run(id) => state.live.get(id),
    }
    .filter(|r| !r.is_done())
}

/// Move `sub` to `follow`'s run when it is a different one. `fresh` voices the run from its
/// start (it began after the client chose).
fn retarget(
    hub: &Arc<VoiceHub>,
    state: &AppState,
    follow: &Follow,
    sub: &mut Option<Following>,
    fresh: bool,
) {
    let Some(run) = target(state, follow) else {
        return;
    };
    if sub.as_ref().is_some_and(|s| s.run_id == run.run_id) {
        return;
    }
    *sub = None;
    *sub = Some(hub.follow(&run, fresh));
}

async fn next_frame(sub: &mut Option<Following>) -> Result<VoiceItem, RecvError> {
    match sub {
        Some(s) => s.rx.recv().await,
        None => std::future::pending().await,
    }
}

pub async fn session(state: Arc<AppState>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Out>(SOCKET_QUEUE);
    let writer = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            let msg = match out {
                Out::Text(t) => Message::Text(Utf8Bytes::from(t.as_ref())),
                Out::Close => {
                    let frame = CloseFrame {
                        code: close_code::AWAY,
                        reason: Utf8Bytes::from_static(SHUTDOWN_REASON),
                    };
                    let _ = sink.send(Message::Close(Some(frame))).await;
                    break;
                }
            };
            if sink.send(msg).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let hub = state.voice.clone();
    let mut out = Outbox::new(tx.clone());
    // Waits for room in the queue while something is owed, so owed frames go out promptly.
    let room = tx.clone();
    let mut status = hub.status();
    let first = status.borrow_and_update().clone();
    out.status(&first);
    let mut latest = state.live.latest();
    latest.borrow_and_update();
    let shutdown = state.shutdown_signal();
    tokio::pin!(shutdown);
    let mut follow: Option<Follow> = None;
    let mut sub: Option<Following> = None;
    loop {
        tokio::select! {
            msg = stream.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => continue,
                };
                match parse_client(&text) {
                    Some(Client::Report(r)) => hub.logs().stage(&r),
                    Some(Client::Follow(f)) => {
                        if follow.as_ref() != Some(&f) {
                            sub = None;
                        }
                        if hub.enabled() {
                            retarget(&hub, &state, &f, &mut sub, false);
                        }
                        follow = Some(f);
                    }
                    None => {
                        let short: String = text.chars().take(120).collect();
                        eprintln!("/voice: ignoring a frame that is not a client frame: {short}");
                    }
                }
            }
            r = status.changed() => {
                if r.is_err() {
                    break;
                }
                let s = status.borrow_and_update().clone();
                out.status(&s);
            }
            r = latest.changed() => {
                if r.is_err() {
                    break;
                }
                latest.borrow_and_update();
                if let (Some(f), true) = (&follow, hub.enabled()) {
                    retarget(&hub, &state, f, &mut sub, true);
                }
            }
            permit = room.reserve(), if out.owes() => {
                drop(permit);
                out.flush();
            }
            item = next_frame(&mut sub) => match item {
                Ok(item) => out.frame(item),
                Err(RecvError::Lagged(n)) => {
                    eprintln!("/voice: a client fell {n} frames behind the voice; they are lost");
                }
                Err(RecvError::Closed) => sub = None,
            },
            () = &mut shutdown => {
                let _ = tx.send(Out::Close).await;
                break;
            }
        }
        if out.is_closed() {
            break;
        }
    }
    drop(sub);
    drop(out);
    drop(room);
    drop(tx);
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utt(seq: u64, part: u32, last: bool) -> VoiceItem {
        VoiceItem::new(VoiceFrame::Utterance {
            id: format!("r:{seq}:{part}"),
            run_id: "r".into(),
            seq,
            part,
            last,
            text: "x".into(),
            sample_rate: 24_000,
            duration_ms: 1,
            pcm: String::new(),
            visemes: Vec::new(),
        })
    }

    fn drain(rx: &mut mpsc::Receiver<Out>) -> Vec<VoiceFrame> {
        let mut v = Vec::new();
        while let Ok(Out::Text(t)) = rx.try_recv() {
            v.push(serde_json::from_str(&t).unwrap());
        }
        v
    }

    fn key(f: &VoiceFrame) -> String {
        match f {
            VoiceFrame::Utterance { seq, part, .. } => format!("u{seq}.{part}"),
            VoiceFrame::Skipped {
                seq, part, reason, ..
            } => format!("s{seq}.{part}:{}", serde_json::to_string(reason).unwrap()),
            VoiceFrame::Status { state, .. } => {
                format!("status:{}", serde_json::to_string(state).unwrap())
            }
        }
    }

    #[test]
    fn follow_frames_parse_and_anything_else_is_ignored() {
        assert_eq!(parse_follow(r#"{"follow":{}}"#), Some(Follow::Newest));
        assert_eq!(
            parse_follow(r#"{"follow":{"run_id":"sbx-1"}}"#),
            Some(Follow::Run("sbx-1".into()))
        );
        assert_eq!(
            parse_follow(r#"{"follow":{"run_id":""}}"#),
            Some(Follow::Newest)
        );
        assert_eq!(parse_follow(r#"{"subscribe":"live"}"#), None);
        assert!(matches!(
            parse_client(r#"{"played":{"run_id":"r","seq":3,"part":0,"at_wall_ms":9}}"#),
            Some(Client::Report(VoiceClientFrame::Played(_)))
        ));
        assert_eq!(
            parse_follow(r#"{"played":{"run_id":"r","seq":3,"part":0,"at_wall_ms":9}}"#),
            None
        );
        assert_eq!(parse_client(r#"{"played":{"run_id":"r"}}"#), None);
        assert_eq!(parse_follow("nope"), None);
    }

    #[test]
    fn a_slow_client_loses_the_rest_of_a_line_and_is_told_once() {
        let (tx, mut rx) = mpsc::channel(2);
        let mut out = Outbox::new(tx);
        // Line 1 has four sentences; only two fit.
        for p in 0..4 {
            out.frame(utt(1, p, p == 3));
        }
        // Line 2 arrives while the client is still stuck: all of it is owed as one skip.
        out.frame(utt(2, 0, false));
        out.frame(utt(2, 1, true));
        assert_eq!(
            drain(&mut rx).iter().map(key).collect::<Vec<_>>(),
            ["u1.0", "u1.1"]
        );
        // Room again: the owed skips go first, then new frames.
        out.flush();
        out.frame(utt(3, 0, true));
        assert_eq!(
            drain(&mut rx).iter().map(key).collect::<Vec<_>>(),
            ["s1.2:\"queue_full\"", "s2.0:\"queue_full\""]
        );
        out.flush();
        // Line 3's single sentence did not fit behind the owed frames: owed too.
        assert_eq!(
            drain(&mut rx).iter().map(key).collect::<Vec<_>>(),
            ["s3.0:\"queue_full\""]
        );
        assert!(out.dropping.is_empty() && out.owed.is_empty());
    }

    #[test]
    fn a_service_skip_for_a_line_already_dropped_is_not_repeated() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut out = Outbox::new(tx);
        out.frame(utt(1, 0, false));
        out.frame(utt(1, 1, false));
        out.frame(VoiceItem::new(VoiceFrame::Skipped {
            run_id: "r".into(),
            seq: 1,
            part: 2,
            reason: VoiceSkip::Error,
        }));
        drain(&mut rx);
        out.flush();
        assert_eq!(
            drain(&mut rx).iter().map(key).collect::<Vec<_>>(),
            ["s1.1:\"queue_full\""]
        );
        out.flush();
        assert!(drain(&mut rx).is_empty());
    }

    #[test]
    fn a_status_that_does_not_fit_is_kept_newest_first() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut out = Outbox::new(tx);
        out.frame(utt(1, 0, true));
        let st = |s| {
            VoiceItem::new(VoiceFrame::Status {
                state: s,
                reason: None,
                voice: None,
                visemes: false,
            })
        };
        out.status(&st(shadowrunr_types::voice::VoiceState::Degraded));
        out.status(&st(shadowrunr_types::voice::VoiceState::Ready));
        assert_eq!(drain(&mut rx).iter().map(key).collect::<Vec<_>>(), ["u1.0"]);
        out.flush();
        assert_eq!(
            drain(&mut rx).iter().map(key).collect::<Vec<_>>(),
            ["status:\"ready\""]
        );
    }
}
