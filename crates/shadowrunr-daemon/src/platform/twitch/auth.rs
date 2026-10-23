//! OAuth against `id.twitch.tv`: the device-code grant for a public client (no secret),
//! refresh, validate and revoke (dev.twitch.tv/docs/authentication).
//!
//! Tokens only ever travel in a form body or an `Authorization` header, never a URL, so a
//! transport error (which names the URL) cannot carry one.

use std::time::{Duration, Instant};

use serde::Deserialize;
use shadowrunr_types::moderation::PlatformState;

use super::store::{Credential, Secret};
use super::{PlatformError, TwitchPlatform, SCOPES};

/// `POST /oauth2/device`.
#[derive(Debug, Deserialize)]
pub(super) struct DeviceCode {
    pub device_code: Secret,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u32,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// A token grant (device code or refresh).
#[derive(Debug, Deserialize)]
struct TokenGrant {
    access_token: Secret,
    refresh_token: Secret,
}

/// `GET /oauth2/validate`.
#[derive(Debug, Deserialize)]
pub(super) struct Validated {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

/// Why a validate failed: the token is dead, or something else went wrong.
#[derive(Debug)]
pub(super) enum ValidateError {
    Unauthorized,
    Other(String),
}

/// Twitch's OAuth error body: `{"status": 400, "message": "authorization_pending"}`.
pub(super) fn twitch_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(200).collect())
}

/// The text the operator sees when the login cannot be renewed.
pub(super) const RELOGIN: &str =
    "the Twitch login expired or was revoked; run twitch.connect again";

impl TwitchPlatform {
    fn id_url(&self, path: &str) -> String {
        format!("{}{}", self.ep.id_base.trim_end_matches('/'), path)
    }

    pub(super) async fn request_device_code(&self) -> Result<DeviceCode, PlatformError> {
        let resp = self
            .http
            .post(self.id_url("/oauth2/device"))
            .form(&[
                ("client_id", self.cfg.client_id.as_str()),
                ("scopes", SCOPES),
            ])
            .send()
            .await
            .map_err(|e| PlatformError(format!("twitch: device login: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(PlatformError(format!(
                "twitch: device login: HTTP {}: {}",
                status.as_u16(),
                twitch_message(&body)
            )));
        }
        serde_json::from_str(&body)
            .map_err(|_| PlatformError("twitch: device login: unreadable response".into()))
    }

    /// Polls `/oauth2/token` until the operator enters the code, it expires, or it is denied.
    /// On success the credential is stored and the session starts.
    pub(super) async fn poll_device_login(self: std::sync::Arc<Self>, device: DeviceCode) {
        let unit = self.ep.time_unit;
        let deadline = Instant::now() + unit * device.expires_in;
        let mut interval = device.interval.max(1);
        loop {
            tokio::time::sleep(unit * interval as u32).await;
            if Instant::now() >= deadline {
                self.set_live(
                    PlatformState::Error,
                    Some("the Twitch code expired; run twitch.connect again".into()),
                );
                return;
            }
            let sent = self
                .http
                .post(self.id_url("/oauth2/token"))
                .form(&[
                    ("client_id", self.cfg.client_id.as_str()),
                    ("scopes", SCOPES),
                    ("device_code", device.device_code.expose()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await;
            let resp = match sent {
                Ok(r) => r,
                // A network blip: try again at the next interval.
                Err(e) => {
                    eprintln!("twitch: device login poll: {e}");
                    continue;
                }
            };
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if status.is_success() {
                let Ok(grant) = serde_json::from_str::<TokenGrant>(&body) else {
                    self.set_live(
                        PlatformState::Error,
                        Some("twitch login failed: unreadable token response".into()),
                    );
                    return;
                };
                let cred = Credential {
                    access_token: grant.access_token,
                    refresh_token: grant.refresh_token,
                    login: String::new(),
                    user_id: String::new(),
                };
                if let Err(e) = self.store.save(&cred) {
                    self.set_live(PlatformState::Error, Some(e));
                    return;
                }
                self.set_cred(Some(cred));
                self.set_live(PlatformState::Connecting, None);
                self.spawn_session();
                return;
            }
            let message = twitch_message(&body);
            match message.as_str() {
                "authorization_pending" => {}
                "slow_down" => interval += 5,
                _ => {
                    self.set_live(
                        PlatformState::Error,
                        Some(format!("twitch login failed: {message}")),
                    );
                    return;
                }
            }
        }
    }

    /// Exchanges the refresh token for a new pair, unless another caller already did since
    /// `stale` was read. The new pair is stored (public-client refresh tokens are one-time).
    pub(super) async fn refresh(&self, stale: &Secret) -> Result<(), PlatformError> {
        let _guard = self.refresh_lock.lock().await;
        let Some(cur) = self.current_cred() else {
            return Err(PlatformError("twitch: not connected".into()));
        };
        if cur.access_token != *stale {
            return Ok(());
        }
        let resp = self
            .http
            .post(self.id_url("/oauth2/token"))
            .form(&[
                ("client_id", self.cfg.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", cur.refresh_token.expose()),
            ])
            .send()
            .await
            .map_err(|e| PlatformError(format!("twitch: refresh: {e}")))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            if status.as_u16() == 400 || status.as_u16() == 401 {
                self.set_live(PlatformState::Error, Some(RELOGIN.into()));
                return Err(PlatformError(format!("twitch: {RELOGIN}")));
            }
            return Err(PlatformError(format!(
                "twitch: refresh: HTTP {}: {}",
                status.as_u16(),
                self.scrub(&twitch_message(&body))
            )));
        }
        let grant: TokenGrant = serde_json::from_str(&body)
            .map_err(|_| PlatformError("twitch: refresh: unreadable response".into()))?;
        let next = Credential {
            access_token: grant.access_token,
            refresh_token: grant.refresh_token,
            ..cur
        };
        if let Err(e) = self.store.save(&next) {
            eprintln!("twitch: {e}");
        }
        self.set_cred(Some(next));
        Ok(())
    }

    /// `GET /oauth2/validate` with the current token.
    pub(super) async fn validate_once(&self, token: &Secret) -> Result<Validated, ValidateError> {
        let resp = self
            .http
            .get(self.id_url("/oauth2/validate"))
            .header("Authorization", format!("OAuth {}", token.expose()))
            .send()
            .await
            .map_err(|e| ValidateError::Other(format!("twitch: validate: {e}")))?;
        let status = resp.status();
        if status.as_u16() == 401 {
            return Err(ValidateError::Unauthorized);
        }
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ValidateError::Other(format!(
                "twitch: validate: HTTP {}: {}",
                status.as_u16(),
                self.scrub(&twitch_message(&body))
            )));
        }
        serde_json::from_str(&body)
            .map_err(|_| ValidateError::Other("twitch: validate: unreadable response".into()))
    }

    /// Validates the token (refreshing once if it is dead), then records the bot's login and
    /// user id, storing them with the credential when they changed.
    pub(super) async fn validate(&self) -> Result<(), ValidateError> {
        let Some(cred) = self.current_cred() else {
            return Err(ValidateError::Unauthorized);
        };
        let v = match self.validate_once(&cred.access_token).await {
            Ok(v) => v,
            Err(ValidateError::Unauthorized) => {
                self.refresh(&cred.access_token)
                    .await
                    .map_err(|_| ValidateError::Unauthorized)?;
                let fresh = self.current_cred().ok_or(ValidateError::Unauthorized)?;
                self.validate_once(&fresh.access_token).await?
            }
            Err(e) => return Err(e),
        };
        let (Some(login), Some(user_id)) = (v.login, v.user_id) else {
            return Err(ValidateError::Other(
                "twitch: the token is not a user token".into(),
            ));
        };
        if let Some(mut cur) = self.current_cred() {
            if cur.login != login || cur.user_id != user_id {
                cur.login = login.clone();
                cur.user_id = user_id.clone();
                if let Err(e) = self.store.save(&cur) {
                    eprintln!("twitch: {e}");
                }
                self.set_cred(Some(cur));
            }
        }
        if let Ok(mut l) = self.live.lock() {
            l.bot_login = Some(login);
            l.bot_id = Some(user_id);
        }
        Ok(())
    }

    /// `POST /oauth2/revoke`, best effort with a short deadline.
    pub(super) async fn revoke(&self, token: &Secret) {
        let call = self
            .http
            .post(self.id_url("/oauth2/revoke"))
            .form(&[
                ("client_id", self.cfg.client_id.as_str()),
                ("token", token.expose()),
            ])
            .send();
        match tokio::time::timeout(Duration::from_secs(5), call).await {
            Ok(Ok(r)) if r.status().is_success() => {}
            Ok(Ok(r)) => eprintln!("twitch: revoke: HTTP {}", r.status().as_u16()),
            Ok(Err(e)) => eprintln!("twitch: revoke: {e}"),
            Err(_) => eprintln!("twitch: revoke: timed out"),
        }
    }
}
