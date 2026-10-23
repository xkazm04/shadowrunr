//! The EventSub WebSocket session (dev.twitch.tv/docs/eventsub/handling-websocket-events).
//!
//! Connect, read `session_welcome`, subscribe `channel.chat.message` within Twitch's 10 s
//! window, then read: `notification` becomes a [`ChatMsg`] in the inbox, `session_keepalive`
//! resets the silence timer, `session_reconnect` moves to the new URL (the subscription
//! carries over; the old socket keeps delivering until the new one says welcome),
//! `revocation` stops the session. A dropped socket reconnects with backoff and resubscribes,
//! since a fresh session carries nothing over. The client never sends a frame of its own
//! (Twitch closes a socket that does, code 4001) beyond the protocol's pong and close.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use shadowrunr_types::moderation::PlatformState;
use shadowrunr_types::{ChatMsg, Platform, Rank, Viewer};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::auth::{ValidateError, RELOGIN};
use super::TwitchPlatform;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How many notification ids are remembered to drop Twitch's occasional duplicates.
const SEEN_IDS: usize = 512;
/// Twitch's default keepalive window when the welcome does not name one.
const DEFAULT_KEEPALIVE_S: u64 = 10;
/// Slack on top of the keepalive window before the socket is presumed dead.
const KEEPALIVE_SLACK_S: u64 = 5;
/// The longest reconnect backoff, in time units.
const MAX_BACKOFF: u32 = 60;

/// One frame from Twitch, as far as the session cares.
#[derive(Debug)]
enum Frame {
    Welcome {
        session_id: String,
        keepalive_s: u64,
    },
    Keepalive,
    Reconnect(String),
    Notification {
        id: String,
        kind: String,
        event: Value,
    },
    Revocation(String),
    Other,
}

#[derive(Deserialize)]
struct Envelope {
    metadata: Metadata,
    #[serde(default)]
    payload: Value,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    message_id: String,
    message_type: String,
    #[serde(default)]
    subscription_type: Option<String>,
}

fn parse_frame(text: &str) -> Frame {
    let Ok(env) = serde_json::from_str::<Envelope>(text) else {
        return Frame::Other;
    };
    let p = &env.payload;
    let str_at = |v: &Value, path: &[&str]| -> Option<String> {
        let mut cur = v;
        for k in path {
            cur = cur.get(*k)?;
        }
        cur.as_str().map(str::to_string)
    };
    match env.metadata.message_type.as_str() {
        "session_welcome" => match str_at(p, &["session", "id"]) {
            Some(session_id) => Frame::Welcome {
                session_id,
                keepalive_s: p
                    .pointer("/session/keepalive_timeout_seconds")
                    .and_then(|k| k.as_u64())
                    .unwrap_or(DEFAULT_KEEPALIVE_S),
            },
            None => Frame::Other,
        },
        "session_keepalive" => Frame::Keepalive,
        "session_reconnect" => match str_at(p, &["session", "reconnect_url"]) {
            Some(url) => Frame::Reconnect(url),
            None => Frame::Other,
        },
        "notification" => Frame::Notification {
            id: env.metadata.message_id,
            kind: env
                .metadata
                .subscription_type
                .or_else(|| str_at(p, &["subscription", "type"]))
                .unwrap_or_default(),
            event: p.get("event").cloned().unwrap_or(Value::Null),
        },
        "revocation" => Frame::Revocation(
            str_at(p, &["subscription", "status"]).unwrap_or_else(|| "revoked".into()),
        ),
        _ => Frame::Other,
    }
}

#[derive(Deserialize)]
struct ChatEvent {
    chatter_user_id: String,
    chatter_user_name: String,
    message_id: String,
    message: ChatText,
    #[serde(default)]
    badges: Vec<Badge>,
}

#[derive(Deserialize)]
struct ChatText {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct Badge {
    set_id: String,
    #[serde(default)]
    id: String,
}

/// Maps a `channel.chat.message` event to a [`ChatMsg`]. The bot's own messages map to
/// `None`: the moderator never moderates itself.
pub(super) fn map_chat(event: &Value, bot_id: &str, ts_ms: i64) -> Option<ChatMsg> {
    let e: ChatEvent = serde_json::from_value(event.clone()).ok()?;
    if e.chatter_user_id.is_empty() || e.chatter_user_id == bot_id {
        return None;
    }
    let has = |set: &str| e.badges.iter().any(|b| b.set_id == set);
    let is_mod = has("moderator") || has("broadcaster");
    // The subscriber badge's version is the tenure badge; tier 2 and 3 badges are numbered
    // from 2000 and 3000. A founder is a subscriber whose tier the badge does not say.
    let sub_tier = e
        .badges
        .iter()
        .find(|b| b.set_id == "subscriber")
        .map(|b| match b.id.parse::<u32>().unwrap_or(0) {
            n if n >= 3000 => 3,
            n if n >= 2000 => 2,
            _ => 1,
        })
        .or_else(|| has("founder").then_some(1));
    Some(ChatMsg {
        id: e.message_id,
        ts_ms,
        viewer: Viewer {
            id: e.chatter_user_id,
            name: e.chatter_user_name,
            platform: Platform::Twitch,
            rank: Rank::Thrall,
            sub_tier,
            months: 0,
            gifted: 0,
            is_mod,
        },
        command: shadowrunr_segments::command::parse_command(&e.message.text),
        text: e.message.text,
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// How a session attempt ended.
enum End {
    /// Nothing more to do until the operator acts (state already set).
    Stop,
    /// Try again after backoff.
    Retry(String),
}

impl TwitchPlatform {
    /// The session task: prepare, connect, serve, and reconnect with backoff until stopped.
    pub(super) async fn run_session(self: Arc<Self>) {
        let mut attempt: u32 = 0;
        loop {
            let end = match self.prepare().await {
                Ok(()) => self.serve(&mut attempt).await,
                Err(end) => end,
            };
            match end {
                End::Stop => return,
                End::Retry(why) => {
                    attempt = attempt.saturating_add(1);
                    let wait = 1u32
                        .checked_shl(attempt.saturating_sub(1).min(6))
                        .unwrap_or(MAX_BACKOFF)
                        .min(MAX_BACKOFF);
                    eprintln!("twitch: {why}; reconnecting in {wait} units");
                    self.set_live(
                        PlatformState::Connecting,
                        Some(format!("reconnecting: {why}")),
                    );
                    tokio::time::sleep(self.ep.time_unit * wait).await;
                }
            }
        }
    }

    /// Loads the credential, validates it and resolves the channel.
    async fn prepare(&self) -> Result<(), End> {
        if self.current_cred().is_none() {
            match self.store.load() {
                Ok(Some(c)) => self.set_cred(Some(c)),
                Ok(None) => {
                    self.set_live(
                        PlatformState::Disconnected,
                        Some("not logged in; run twitch.connect".into()),
                    );
                    return Err(End::Stop);
                }
                Err(e) => {
                    self.set_live(PlatformState::Error, Some(e));
                    return Err(End::Stop);
                }
            }
        }
        if self.cfg.channel.trim().is_empty() {
            self.set_live(
                PlatformState::Error,
                Some("no [twitch] channel configured".into()),
            );
            return Err(End::Stop);
        }
        if self.live().state != PlatformState::Connecting {
            self.set_live(PlatformState::Connecting, None);
        }
        match self.validate().await {
            Ok(()) => {}
            Err(ValidateError::Unauthorized) => {
                self.set_live(PlatformState::Error, Some(RELOGIN.into()));
                return Err(End::Stop);
            }
            Err(ValidateError::Other(e)) => return Err(End::Retry(e)),
        }
        if self.live().broadcaster_id.is_none() {
            let channel = self.cfg.channel.trim().to_ascii_lowercase();
            match self.helix_user_id(&channel).await {
                Ok(Some(id)) => {
                    if let Ok(mut l) = self.live.lock() {
                        l.broadcaster_id = Some(id);
                    }
                }
                Ok(None) => {
                    self.set_live(
                        PlatformState::Error,
                        Some(format!("twitch channel {channel} not found")),
                    );
                    return Err(End::Stop);
                }
                Err(e) => return Err(self.retry_or_stop(e.0)),
            }
        }
        Ok(())
    }

    /// A failed call is retried unless the login itself is gone.
    fn retry_or_stop(&self, why: String) -> End {
        if why.contains(RELOGIN) {
            self.set_live(PlatformState::Error, Some(RELOGIN.into()));
            End::Stop
        } else {
            End::Retry(why)
        }
    }

    /// One session: connect, welcome, subscribe, read until it ends.
    async fn serve(&self, attempt: &mut u32) -> End {
        let mut ws = match tokio_tungstenite::connect_async(self.ep.eventsub_url.as_str()).await {
            Ok((ws, _)) => ws,
            Err(e) => return End::Retry(format!("eventsub connect: {e}")),
        };
        let (session_id, mut keepalive_s) = match read_welcome(&mut ws).await {
            Ok(w) => w,
            Err(e) => return End::Retry(e),
        };
        if let Err(e) = self.helix_subscribe_chat(&session_id).await {
            return self.retry_or_stop(e.0);
        }
        *attempt = 0;
        self.set_live(PlatformState::Connected, None);
        eprintln!(
            "twitch: connected as {} to #{}",
            self.live().bot_login.unwrap_or_default(),
            self.cfg.channel
        );
        let mut seen: VecDeque<String> = VecDeque::new();
        let every = self.ep.validate_every;
        let mut validate_tick =
            tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        loop {
            let window = Duration::from_secs(keepalive_s + KEEPALIVE_SLACK_S);
            tokio::select! {
                next = tokio::time::timeout(window, ws.next()) => {
                    let msg = match next {
                        Err(_) => return End::Retry("eventsub: keepalive missed".into()),
                        Ok(None) => return End::Retry("eventsub: socket closed".into()),
                        Ok(Some(Err(e))) => return End::Retry(format!("eventsub: {e}")),
                        Ok(Some(Ok(m))) => m,
                    };
                    match msg {
                        Message::Text(t) => match parse_frame(t.as_str()) {
                            Frame::Notification { id, kind, event } => {
                                self.on_notification(&mut seen, id, &kind, &event).await;
                            }
                            Frame::Revocation(status) => return self.revoked(&status),
                            Frame::Reconnect(url) => {
                                match self.reconnect(ws, &url, &mut seen).await {
                                    Ok((next, ka)) => {
                                        ws = next;
                                        keepalive_s = ka;
                                    }
                                    Err(end) => return end,
                                }
                            }
                            Frame::Welcome { .. } | Frame::Keepalive | Frame::Other => {}
                        },
                        Message::Close(frame) => {
                            let code = frame.map(|f| u16::from(f.code)).unwrap_or(0);
                            return End::Retry(format!("eventsub: closed by twitch (code {code})"));
                        }
                        _ => {}
                    }
                }
                _ = validate_tick.tick() => {
                    match self.validate().await {
                        Ok(()) => {}
                        Err(ValidateError::Unauthorized) => {
                            self.set_live(PlatformState::Error, Some(RELOGIN.into()));
                            let _ = ws.close(None).await;
                            return End::Stop;
                        }
                        Err(ValidateError::Other(e)) => eprintln!("twitch: {e}"),
                    }
                }
            }
        }
    }

    /// `session_reconnect`: connect to the new URL and keep reading the old socket until the
    /// new one says welcome, then close the old one. No resubscribe: subscriptions carry over.
    async fn reconnect(
        &self,
        old: Ws,
        url: &str,
        seen: &mut VecDeque<String>,
    ) -> Result<(Ws, u64), End> {
        let mut new = match tokio_tungstenite::connect_async(url).await {
            Ok((ws, _)) => ws,
            Err(e) => return Err(End::Retry(format!("eventsub reconnect: {e}"))),
        };
        let mut old = Some(old);
        let deadline = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                m = new.next() => match m {
                    Some(Ok(Message::Text(t))) => match parse_frame(t.as_str()) {
                        Frame::Welcome { keepalive_s, .. } => {
                            if let Some(mut o) = old.take() {
                                let _ = o.close(None).await;
                            }
                            return Ok((new, keepalive_s));
                        }
                        Frame::Notification { id, kind, event } => {
                            self.on_notification(seen, id, &kind, &event).await;
                        }
                        _ => {}
                    },
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                        return Err(End::Retry("eventsub reconnect: the new socket closed".into()));
                    }
                    Some(Ok(_)) => {}
                },
                m = next_or_pending(&mut old) => match m {
                    Some(Ok(Message::Text(t))) => match parse_frame(t.as_str()) {
                        Frame::Notification { id, kind, event } => {
                            self.on_notification(seen, id, &kind, &event).await;
                        }
                        Frame::Revocation(status) => return Err(self.revoked(&status)),
                        _ => {}
                    },
                    Some(Ok(_)) => {}
                    // The old socket is done; wait for the new one alone.
                    Some(Err(_)) | None => old = None,
                },
                _ = &mut deadline => {
                    return Err(End::Retry("eventsub reconnect: no welcome on the new socket".into()));
                }
            }
        }
    }

    fn revoked(&self, status: &str) -> End {
        self.set_live(
            PlatformState::Error,
            Some(format!(
                "twitch revoked the chat subscription ({status}); run twitch.connect again"
            )),
        );
        End::Stop
    }

    async fn on_notification(
        &self,
        seen: &mut VecDeque<String>,
        id: String,
        kind: &str,
        event: &Value,
    ) {
        if kind != "channel.chat.message" {
            return;
        }
        if !id.is_empty() {
            if seen.contains(&id) {
                return;
            }
            seen.push_back(id);
            if seen.len() > SEEN_IDS {
                seen.pop_front();
            }
        }
        let bot_id = self.live().bot_id.unwrap_or_default();
        let Some(msg) = map_chat(event, &bot_id, now_ms()) else {
            return;
        };
        if let Some(inbox) = self.inbox() {
            let _ = inbox.send(msg).await;
        }
    }
}

async fn read_welcome(ws: &mut Ws) -> Result<(String, u64), String> {
    let wait = async {
        while let Some(m) = ws.next().await {
            match m {
                Ok(Message::Text(t)) => {
                    if let Frame::Welcome {
                        session_id,
                        keepalive_s,
                    } = parse_frame(t.as_str())
                    {
                        return Ok((session_id, keepalive_s));
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(e) => return Err(format!("eventsub: {e}")),
            }
        }
        Err("eventsub: closed before welcome".to_string())
    };
    tokio::time::timeout(Duration::from_secs(15), wait)
        .await
        .unwrap_or_else(|_| Err("eventsub: no welcome".into()))
}

async fn next_or_pending(
    ws: &mut Option<Ws>,
) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
    match ws.as_mut() {
        Some(w) => w.next().await,
        None => std::future::pending().await,
    }
}
