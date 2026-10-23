//! Helix calls (dev.twitch.tv/docs/api/reference): `Authorization: Bearer` plus `Client-Id`.
//! A 401 refreshes the token once and retries once; a 429 fails the call at once, naming
//! when the bucket resets, and later calls fail fast until then so the lane never blocks.

use reqwest::Method;
use serde_json::{json, Value};

use super::auth::twitch_message;
use super::{
    now_s, truncate_chars, PlatformError, TwitchPlatform, MAX_MESSAGE_CHARS, MAX_REASON_CHARS,
    MAX_TIMEOUT_S,
};

impl TwitchPlatform {
    /// One Helix call; the response body on success.
    pub(super) async fn helix(
        &self,
        op: &str,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<String, PlatformError> {
        if let Some(until) = self.live().rate_limited_until {
            let wait = until - now_s();
            if wait > 0 {
                return Err(PlatformError(format!(
                    "twitch: {op}: rate limited, the bucket resets in {wait}s"
                )));
            }
        }
        let url = format!("{}{}", self.ep.helix_base.trim_end_matches('/'), path);
        let mut retried = false;
        loop {
            let Some(cred) = self.current_cred() else {
                return Err(PlatformError("twitch: not connected".into()));
            };
            let mut req = self
                .http
                .request(method.clone(), &url)
                .query(query)
                .bearer_auth(cred.access_token.expose())
                .header("Client-Id", &self.cfg.client_id);
            if let Some(b) = &body {
                req = req
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(b.to_string());
            }
            let resp = req.send().await.map_err(|e| {
                PlatformError(format!("twitch: {op}: {}", self.scrub(&e.to_string())))
            })?;
            let status = resp.status().as_u16();
            if status == 401 && !retried {
                retried = true;
                self.refresh(&cred.access_token).await?;
                continue;
            }
            if status == 429 {
                let reset = resp
                    .headers()
                    .get("Ratelimit-Reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<i64>().ok());
                let wait = match reset {
                    Some(r) => {
                        if let Ok(mut l) = self.live.lock() {
                            l.rate_limited_until = Some(r);
                        }
                        (r - now_s()).max(0)
                    }
                    None => 0,
                };
                return Err(PlatformError(format!(
                    "twitch: {op}: rate limited (HTTP 429), the bucket resets in {wait}s"
                )));
            }
            let text = resp.text().await.unwrap_or_default();
            if (200..300).contains(&status) {
                return Ok(text);
            }
            return Err(PlatformError(format!(
                "twitch: {op}: HTTP {status}: {}",
                self.scrub(&twitch_message(&text))
            )));
        }
    }

    /// (broadcaster id, bot user id): both known once the session has connected.
    fn ids(&self) -> Result<(String, String), PlatformError> {
        let live = self.live();
        match (live.broadcaster_id, live.bot_id) {
            (Some(b), Some(m)) => Ok((b, m)),
            _ => Err(PlatformError("twitch: not connected".into())),
        }
    }

    /// `GET /users?login=`: the user id of a login, `None` if there is no such user.
    pub(super) async fn helix_user_id(&self, login: &str) -> Result<Option<String>, PlatformError> {
        let body = self
            .helix("get user", Method::GET, "/users", &[("login", login)], None)
            .await?;
        let v: Value = serde_json::from_str(&body)
            .map_err(|_| PlatformError("twitch: get user: unreadable response".into()))?;
        Ok(v.get("data")
            .and_then(|d| d.as_array())
            .and_then(|d| d.first())
            .and_then(|u| u.get("id"))
            .and_then(|id| id.as_str())
            .map(str::to_string))
    }

    /// `DELETE /moderation/chat`. An empty id is refused here: Twitch reads a missing
    /// `message_id` as "clear the whole chat".
    pub(super) async fn helix_delete_message(&self, message_id: &str) -> Result<(), PlatformError> {
        if message_id.trim().is_empty() {
            return Err(PlatformError("twitch: delete: no message id".into()));
        }
        let (b, m) = self.ids()?;
        self.helix(
            "delete",
            Method::DELETE,
            "/moderation/chat",
            &[
                ("broadcaster_id", &b),
                ("moderator_id", &m),
                ("message_id", message_id),
            ],
            None,
        )
        .await
        .map(|_| ())
    }

    /// `POST /moderation/bans`: a timeout when `seconds` is set, else a ban.
    pub(super) async fn helix_ban(
        &self,
        viewer_id: &str,
        seconds: Option<u32>,
        reason: &str,
    ) -> Result<(), PlatformError> {
        let op = if seconds.is_some() { "timeout" } else { "ban" };
        if viewer_id.trim().is_empty() {
            return Err(PlatformError(format!("twitch: {op}: no viewer id")));
        }
        let (b, m) = self.ids()?;
        let mut data = json!({
            "user_id": viewer_id,
            "reason": truncate_chars(reason, MAX_REASON_CHARS),
        });
        if let Some(s) = seconds {
            data["duration"] = json!(s.clamp(1, MAX_TIMEOUT_S));
        }
        self.helix(
            op,
            Method::POST,
            "/moderation/bans",
            &[("broadcaster_id", &b), ("moderator_id", &m)],
            Some(json!({ "data": data })),
        )
        .await
        .map(|_| ())
    }

    /// `DELETE /moderation/bans`.
    pub(super) async fn helix_unban(&self, viewer_id: &str) -> Result<(), PlatformError> {
        if viewer_id.trim().is_empty() {
            return Err(PlatformError("twitch: unban: no viewer id".into()));
        }
        let (b, m) = self.ids()?;
        self.helix(
            "unban",
            Method::DELETE,
            "/moderation/bans",
            &[
                ("broadcaster_id", &b),
                ("moderator_id", &m),
                ("user_id", viewer_id),
            ],
            None,
        )
        .await
        .map(|_| ())
    }

    /// `POST /chat/messages` as the bot, cut to Twitch's 500 characters. A message Twitch
    /// accepted but dropped (`is_sent: false`) is an error naming its drop reason.
    pub(super) async fn helix_say(&self, text: &str) -> Result<(), PlatformError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PlatformError("twitch: say: empty message".into()));
        }
        let (b, m) = self.ids()?;
        let body = self
            .helix(
                "say",
                Method::POST,
                "/chat/messages",
                &[],
                Some(json!({
                    "broadcaster_id": b,
                    "sender_id": m,
                    "message": truncate_chars(text, MAX_MESSAGE_CHARS),
                })),
            )
            .await?;
        let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let first = v.get("data").and_then(|d| d.get(0));
        if first
            .and_then(|f| f.get("is_sent"))
            .and_then(|s| s.as_bool())
            == Some(false)
        {
            let reason = first
                .and_then(|f| f.get("drop_reason"))
                .and_then(|r| r.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("no reason given");
            return Err(PlatformError(format!("twitch: say: dropped: {reason}")));
        }
        Ok(())
    }

    /// `POST /eventsub/subscriptions`: `channel.chat.message` v1 on this WebSocket session.
    pub(super) async fn helix_subscribe_chat(&self, session_id: &str) -> Result<(), PlatformError> {
        let (b, m) = self.ids()?;
        self.helix(
            "subscribe",
            Method::POST,
            "/eventsub/subscriptions",
            &[],
            Some(json!({
                "type": "channel.chat.message",
                "version": "1",
                "condition": { "broadcaster_user_id": b, "user_id": m },
                "transport": { "method": "websocket", "session_id": session_id },
            })),
        )
        .await
        .map(|_| ())
    }
}
