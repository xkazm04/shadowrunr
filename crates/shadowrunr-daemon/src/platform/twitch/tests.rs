//! The Twitch adapter against a local mock: an axum server standing in for `id.twitch.tv`
//! and Helix, and a WebSocket server standing in for EventSub, both on 127.0.0.1:0. No
//! network, no OS credential store (a [`MemoryStore`]). All tokens here are fictional.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use shadowrunr_types::moderation::PlatformState;
use shadowrunr_types::{ChatMsg, Command};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::store::{Credential, CredentialStore, MemoryStore, Secret};
use super::{ChatPlatform, Endpoints, TwitchConfig, TwitchPlatform, SCOPES};

const ACCESS: &str = "tok-ACCESS-fictional-1a2b";
const REFRESH: &str = "tok-REFRESH-fictional-3c4d";
const ACCESS2: &str = "tok-ACCESS-NEW-fictional-5e6f";
const REFRESH2: &str = "tok-REFRESH-NEW-fictional-7a8b";
const BOT_ID: &str = "999";
const BROADCASTER_ID: &str = "111";

#[derive(Debug, Clone)]
struct Call {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    body: String,
    auth: String,
}

impl Call {
    fn form(&self) -> Vec<(String, String)> {
        pairs(&self.body)
    }
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

#[derive(Default)]
struct Mock {
    calls: Mutex<Vec<Call>>,
    device_polls: AtomicU32,
    /// Helix answers 401 to this bearer token.
    reject: Mutex<Option<String>>,
    /// Helix answers 429 with a reset 30 s out.
    rate_limit: AtomicBool,
    /// Helix answers 403 with a message that echoes the bearer token.
    echo_403: AtomicBool,
}

impl Mock {
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
    fn calls_to(&self, method: &str, path: &str) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|c| c.method == method && c.path == path)
            .collect()
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pairs(s: &str) -> Vec<(String, String)> {
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

fn get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn reply(status: u16, body: Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if status == 204 {
        return code.into_response();
    }
    (
        code,
        [("content-type", "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn handle(
    State(m): State<Arc<Mock>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let header = |k: &str| {
        headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let call = Call {
        method: method.to_string(),
        path: uri.path().to_string(),
        query: pairs(uri.query().unwrap_or("")),
        body: String::from_utf8_lossy(&body).into_owned(),
        auth: header("authorization"),
    };
    if let Ok(mut c) = m.calls.lock() {
        c.push(call.clone());
    }
    let form = call.form();
    match (call.method.as_str(), call.path.as_str()) {
        ("POST", "/oauth2/device") => reply(
            200,
            json!({
                "device_code": "dc-fictional",
                "expires_in": 1800,
                "interval": 1,
                "user_code": "WXYZ1234",
                "verification_uri": "https://www.twitch.tv/activate?public=true&device-code=WXYZ1234",
            }),
        ),
        ("POST", "/oauth2/token") => match get(&form, "grant_type") {
            Some("urn:ietf:params:oauth:grant-type:device_code") => {
                if m.device_polls.fetch_add(1, Ordering::SeqCst) == 0 {
                    reply(
                        400,
                        json!({"status": 400, "message": "authorization_pending"}),
                    )
                } else {
                    reply(
                        200,
                        json!({"access_token": ACCESS, "refresh_token": REFRESH,
                               "expires_in": 14400, "scope": ["user:read:chat"],
                               "token_type": "bearer"}),
                    )
                }
            }
            Some("refresh_token")
                if get(&form, "refresh_token") == Some(REFRESH)
                    && get(&form, "client_id") == Some("cid")
                    && get(&form, "client_secret").is_none() =>
            {
                reply(
                    200,
                    json!({"access_token": ACCESS2, "refresh_token": REFRESH2,
                           "expires_in": 14400, "scope": ["user:read:chat"],
                           "token_type": "bearer"}),
                )
            }
            _ => reply(
                400,
                json!({"error": "Bad Request", "status": 400, "message": "Invalid refresh token"}),
            ),
        },
        ("GET", "/oauth2/validate") => {
            let token = call.auth.strip_prefix("OAuth ").unwrap_or("");
            if token.starts_with("tok-ACCESS") {
                reply(
                    200,
                    json!({"client_id": "cid", "login": "modbot", "user_id": BOT_ID,
                           "scopes": SCOPES.split(' ').collect::<Vec<_>>(), "expires_in": 3600}),
                )
            } else {
                reply(
                    401,
                    json!({"status": 401, "message": "invalid access token"}),
                )
            }
        }
        ("POST", "/oauth2/revoke") => StatusCode::OK.into_response(),
        (_, p) if p.starts_with("/helix/") => {
            let token = call.auth.strip_prefix("Bearer ").unwrap_or("").to_string();
            if header("client-id") != "cid" || !token.starts_with("tok-ACCESS") {
                return reply(401, json!({"status": 401, "message": "missing auth"}));
            }
            if m.reject.lock().ok().and_then(|r| r.clone()).as_deref() == Some(token.as_str()) {
                return reply(
                    401,
                    json!({"error": "Unauthorized", "status": 401, "message": "Invalid OAuth token"}),
                );
            }
            if m.echo_403.load(Ordering::SeqCst) {
                return reply(
                    403,
                    json!({"status": 403, "message": format!("token {token} may not do this")}),
                );
            }
            if m.rate_limit.load(Ordering::SeqCst) {
                let reset = super::now_s() + 30;
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [("Ratelimit-Reset", reset.to_string())],
                    "{}",
                )
                    .into_response();
            }
            match (call.method.as_str(), p) {
                ("GET", "/helix/users") => {
                    if get(&call.query, "login") == Some("chan") {
                        reply(
                            200,
                            json!({"data": [{"id": BROADCASTER_ID, "login": "chan"}]}),
                        )
                    } else {
                        reply(200, json!({"data": []}))
                    }
                }
                ("POST", "/helix/eventsub/subscriptions") => reply(
                    202,
                    json!({"data": [{"id": "sub-1", "status": "enabled",
                                     "type": "channel.chat.message", "version": "1"}]}),
                ),
                ("DELETE", "/helix/moderation/chat") => reply(204, Value::Null),
                ("POST", "/helix/moderation/bans") => reply(200, json!({"data": [{}]})),
                ("DELETE", "/helix/moderation/bans") => reply(204, Value::Null),
                ("POST", "/helix/chat/messages") => reply(
                    200,
                    json!({"data": [{"message_id": "sent-1", "is_sent": true}]}),
                ),
                _ => reply(404, json!({"status": 404, "message": "not found"})),
            }
        }
        _ => reply(404, json!({"status": 404, "message": "not found"})),
    }
}

type Conn = WebSocketStream<TcpStream>;

struct Harness {
    mock: Arc<Mock>,
    http: String,
    ws_url: String,
    conns: mpsc::Receiver<Conn>,
    store: Arc<MemoryStore>,
}

async fn harness() -> Harness {
    let mock = Arc::new(Mock::default());
    let app = Router::new().fallback(handle).with_state(mock.clone());
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind http");
    let http = format!("http://{}", l.local_addr().expect("addr"));
    tokio::spawn(async move { axum::serve(l, app).await });

    let wl = TcpListener::bind("127.0.0.1:0").await.expect("bind ws");
    let ws_url = format!("ws://{}/ws", wl.local_addr().expect("addr"));
    let (tx, conns) = mpsc::channel(8);
    tokio::spawn(async move {
        while let Ok((s, _)) = wl.accept().await {
            if let Ok(ws) = tokio_tungstenite::accept_async(s).await {
                let _ = tx.send(ws).await;
            }
        }
    });
    Harness {
        mock,
        http,
        ws_url,
        conns,
        store: Arc::new(MemoryStore::new()),
    }
}

impl Harness {
    fn platform(&self) -> Arc<TwitchPlatform> {
        TwitchPlatform::with_endpoints(
            TwitchConfig {
                client_id: "cid".into(),
                channel: "chan".into(),
            },
            Endpoints {
                id_base: self.http.clone(),
                helix_base: format!("{}/helix", self.http),
                eventsub_url: self.ws_url.clone(),
                time_unit: Duration::from_millis(10),
                validate_every: Duration::from_secs(3600),
            },
            self.store.clone(),
        )
    }

    fn store_login(&self) {
        self.store
            .save(&Credential {
                access_token: Secret::new(ACCESS),
                refresh_token: Secret::new(REFRESH),
                login: "modbot".into(),
                user_id: BOT_ID.into(),
            })
            .expect("save");
    }

    async fn accept(&mut self) -> Conn {
        tokio::time::timeout(Duration::from_secs(5), self.conns.recv())
            .await
            .expect("no eventsub connection")
            .expect("ws listener gone")
    }

    /// A platform with a stored login, started, welcomed and subscribed.
    async fn connected(&mut self) -> (Arc<TwitchPlatform>, Conn, mpsc::Receiver<ChatMsg>) {
        self.store_login();
        let p = self.platform();
        let (tx, rx) = mpsc::channel(16);
        p.start(tx);
        let mut ws = self.accept().await;
        send(&mut ws, welcome("S1", None)).await;
        let pc = p.clone();
        until("connected", move || {
            pc.status().state == PlatformState::Connected
        })
        .await;
        (p, ws, rx)
    }
}

async fn until(what: &str, f: impl Fn() -> bool) {
    for _ in 0..500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn send(ws: &mut Conn, v: Value) {
    ws.send(Message::Text(v.to_string().into()))
        .await
        .expect("ws send");
}

fn welcome(id: &str, reconnect: Option<&str>) -> Value {
    json!({
        "metadata": {"message_id": format!("w-{id}"), "message_type": "session_welcome",
                     "message_timestamp": "2026-10-23T12:00:00Z"},
        "payload": {"session": {"id": id, "status": "connected",
                                "connected_at": "2026-10-23T12:00:00Z",
                                "keepalive_timeout_seconds": 10,
                                "reconnect_url": reconnect}}
    })
}

fn reconnect_frame(url: &str) -> Value {
    json!({
        "metadata": {"message_id": "r-1", "message_type": "session_reconnect",
                     "message_timestamp": "2026-10-23T12:00:00Z"},
        "payload": {"session": {"id": "S1", "status": "reconnecting",
                                "keepalive_timeout_seconds": null,
                                "reconnect_url": url,
                                "connected_at": "2026-10-23T12:00:00Z"}}
    })
}

fn chat_event(chatter: &str, name: &str, message_id: &str, text: &str, badges: Value) -> Value {
    json!({
        "broadcaster_user_id": BROADCASTER_ID, "broadcaster_user_login": "chan",
        "broadcaster_user_name": "Chan",
        "chatter_user_id": chatter, "chatter_user_login": name.to_lowercase(),
        "chatter_user_name": name,
        "message_id": message_id,
        "message": {"text": text, "fragments": [{"type": "text", "text": text}]},
        "color": "#00FF00",
        "badges": badges,
        "message_type": "text",
    })
}

fn notification(id: &str, event: Value) -> Value {
    json!({
        "metadata": {"message_id": id, "message_type": "notification",
                     "message_timestamp": "2026-10-23T12:00:01Z",
                     "subscription_type": "channel.chat.message", "subscription_version": "1"},
        "payload": {"subscription": {"id": "sub-1", "status": "enabled",
                                     "type": "channel.chat.message", "version": "1",
                                     "condition": {"broadcaster_user_id": BROADCASTER_ID,
                                                   "user_id": BOT_ID},
                                     "transport": {"method": "websocket", "session_id": "S1"}},
                    "event": event}
    })
}

async fn recv(rx: &mut mpsc::Receiver<ChatMsg>) -> ChatMsg {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("no chat message")
        .expect("inbox closed")
}

fn assert_no_secret(text: &str) {
    for s in [ACCESS, REFRESH, ACCESS2, REFRESH2] {
        assert!(!text.contains(s), "a token leaked into: {text}");
    }
}

// ---- (a) device login -----------------------------------------------------------------------

#[tokio::test]
async fn device_login_polls_through_pending_and_stores_the_token() {
    let mut h = harness().await;
    let p = h.platform();
    let (tx, _rx) = mpsc::channel(16);
    p.start(tx);
    let pc = p.clone();
    until("not logged in", move || {
        pc.status().detail.as_deref() == Some("not logged in; run twitch.connect")
    })
    .await;
    assert_eq!(p.status().state, PlatformState::Disconnected);

    let c = p.begin_device_login().await.expect("device login");
    assert_eq!(c.user_code, "WXYZ1234");
    assert_eq!(c.expires_s, 1800);
    assert!(c
        .verification_uri
        .starts_with("https://www.twitch.tv/activate"));
    let st = p.status();
    assert_eq!(st.state, PlatformState::Connecting);
    assert_eq!(
        st.detail,
        Some(format!("waiting for the code at {}", c.verification_uri))
    );

    let device = h.mock.calls_to("POST", "/oauth2/device");
    assert_eq!(device.len(), 1);
    assert_eq!(get(&device[0].form(), "client_id"), Some("cid"));
    assert_eq!(get(&device[0].form(), "scopes"), Some(SCOPES));

    let store = h.store.clone();
    until("token stored", move || store.raw().is_some()).await;
    assert!(
        h.mock.device_polls.load(Ordering::SeqCst) >= 2,
        "pending was not polled through"
    );
    let polls = h.mock.calls_to("POST", "/oauth2/token");
    let form = polls[0].form();
    assert_eq!(
        get(&form, "grant_type"),
        Some("urn:ietf:params:oauth:grant-type:device_code")
    );
    assert_eq!(get(&form, "device_code"), Some("dc-fictional"));
    assert!(get(&form, "client_secret").is_none());

    let saved = h.store.load().expect("load").expect("entry");
    assert_eq!(saved.access_token.expose(), ACCESS);
    assert_eq!(saved.refresh_token.expose(), REFRESH);

    // The login connects on its own: validate, resolve the channel, open EventSub.
    let mut ws = h.accept().await;
    send(&mut ws, welcome("S1", None)).await;
    let pc = p.clone();
    until("connected", move || {
        pc.status().state == PlatformState::Connected
    })
    .await;
    assert_eq!(p.status().bot_login.as_deref(), Some("modbot"));
    let saved = h.store.load().expect("load").expect("entry");
    assert_eq!(saved.login, "modbot");
    assert_eq!(saved.user_id, BOT_ID);
}

// ---- (a) EventSub -----------------------------------------------------------------------------

#[tokio::test]
async fn eventsub_subscribes_and_maps_chat_dropping_the_bots_own() {
    let mut h = harness().await;
    let (p, mut ws, mut rx) = h.connected().await;

    let subs = h.mock.calls_to("POST", "/helix/eventsub/subscriptions");
    assert_eq!(subs.len(), 1);
    assert_eq!(
        subs[0].json(),
        json!({"type": "channel.chat.message", "version": "1",
               "condition": {"broadcaster_user_id": BROADCASTER_ID, "user_id": BOT_ID},
               "transport": {"method": "websocket", "session_id": "S1"}})
    );
    assert_eq!(subs[0].auth, format!("Bearer {ACCESS}"));
    let users = h.mock.calls_to("GET", "/helix/users");
    assert_eq!(get(&users[0].query, "login"), Some("chan"));

    // The bot's own line is dropped.
    send(
        &mut ws,
        notification(
            "n-0",
            chat_event(BOT_ID, "ModBot", "m-0", "behave", json!([])),
        ),
    )
    .await;
    let mod_event = chat_event(
        "200",
        "SeniorMod",
        "m-1",
        "calm down",
        json!([{"set_id": "moderator", "id": "1", "info": ""}]),
    );
    send(&mut ws, notification("n-1", mod_event.clone())).await;
    // A duplicate delivery of the same notification is dropped.
    send(&mut ws, notification("n-1", mod_event)).await;
    send(
        &mut ws,
        notification(
            "n-2",
            chat_event(
                "300",
                "TierTwo",
                "m-2",
                "!vote a",
                json!([{"set_id": "subscriber", "id": "2012", "info": "14"}]),
            ),
        ),
    )
    .await;

    let m1 = recv(&mut rx).await;
    assert_eq!(m1.id, "m-1");
    assert_eq!(m1.text, "calm down");
    assert_eq!(m1.viewer.id, "200");
    assert_eq!(m1.viewer.name, "SeniorMod");
    assert!(m1.viewer.is_mod);
    assert_eq!(m1.viewer.sub_tier, None);
    assert_eq!(m1.viewer.rank, shadowrunr_types::Rank::Thrall);
    assert_eq!(m1.viewer.platform, shadowrunr_types::Platform::Twitch);
    assert!(m1.command.is_none());
    assert!(m1.ts_ms > 0);

    let m2 = recv(&mut rx).await;
    assert_eq!(m2.id, "m-2");
    assert!(!m2.viewer.is_mod);
    assert_eq!(m2.viewer.sub_tier, Some(2));
    assert!(matches!(m2.command, Some(Command::Vote { .. })));

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        rx.try_recv().is_err(),
        "the bot's own or a duplicate message got through"
    );
    drop(p);
}

#[test]
fn badges_map_to_mod_and_tier() {
    use super::eventsub::map_chat;
    let ev = |badges: Value| chat_event("5", "V", "m", "hi", badges);
    let b = map_chat(
        &ev(json!([{"set_id": "broadcaster", "id": "1", "info": ""}])),
        BOT_ID,
        1,
    )
    .expect("msg");
    assert!(b.viewer.is_mod);
    let f = map_chat(
        &ev(json!([{"set_id": "founder", "id": "0", "info": "3"}])),
        BOT_ID,
        1,
    )
    .expect("msg");
    assert_eq!(f.viewer.sub_tier, Some(1));
    assert!(!f.viewer.is_mod);
    let t3 = map_chat(
        &ev(json!([{"set_id": "subscriber", "id": "3006", "info": "7"}])),
        BOT_ID,
        1,
    )
    .expect("msg");
    assert_eq!(t3.viewer.sub_tier, Some(3));
    let vip = map_chat(&ev(json!([{"set_id": "vip", "id": "1"}])), BOT_ID, 1).expect("msg");
    assert!(!vip.viewer.is_mod);
    assert_eq!(vip.viewer.sub_tier, None);
    assert!(
        map_chat(&ev(json!([])), "5", 1).is_none(),
        "self not dropped"
    );
}

#[tokio::test]
async fn session_reconnect_moves_sockets_without_resubscribing() {
    let mut h = harness().await;
    let (p, mut old, mut rx) = h.connected().await;

    send(
        &mut old,
        reconnect_frame(&format!("{}?reconnect=1", h.ws_url)),
    )
    .await;
    let mut new = h.accept().await;
    // The old socket still delivers until the new one says welcome.
    send(
        &mut old,
        notification(
            "n-a",
            chat_event("400", "Early", "m-a", "still here", json!([])),
        ),
    )
    .await;
    assert_eq!(recv(&mut rx).await.id, "m-a");

    send(&mut new, welcome("S2", None)).await;
    // The client closes the old socket once the new one is welcomed.
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match old.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the old socket was not closed");

    send(
        &mut new,
        notification(
            "n-b",
            chat_event("401", "Later", "m-b", "on the new socket", json!([])),
        ),
    )
    .await;
    assert_eq!(recv(&mut rx).await.id, "m-b");
    assert_eq!(
        h.mock
            .calls_to("POST", "/helix/eventsub/subscriptions")
            .len(),
        1,
        "a session_reconnect must not resubscribe"
    );
    assert_eq!(p.status().state, PlatformState::Connected);
}

#[tokio::test]
async fn a_dropped_socket_reconnects_and_resubscribes() {
    let mut h = harness().await;
    let (p, old, mut rx) = h.connected().await;
    drop(old);
    let mut ws = h.accept().await;
    send(&mut ws, welcome("S9", None)).await;
    let mock = h.mock.clone();
    until("resubscribed", move || {
        mock.calls_to("POST", "/helix/eventsub/subscriptions").len() == 2
    })
    .await;
    let subs = h.mock.calls_to("POST", "/helix/eventsub/subscriptions");
    assert_eq!(subs[1].json()["transport"]["session_id"], "S9");
    send(
        &mut ws,
        notification(
            "n-z",
            chat_event("402", "Back", "m-z", "hello again", json!([])),
        ),
    )
    .await;
    assert_eq!(recv(&mut rx).await.id, "m-z");
    assert_eq!(p.status().state, PlatformState::Connected);
}

// ---- (a) Helix actions ------------------------------------------------------------------------

#[tokio::test]
async fn actions_hit_the_right_method_path_query_and_body() {
    let mut h = harness().await;
    let (p, _ws, _rx) = h.connected().await;

    p.delete("msg-1").await.expect("delete");
    p.timeout("u-2", 600, "spam").await.expect("timeout");
    p.ban("u-3", "slurs").await.expect("ban");
    p.unban("u-3").await.expect("unban");
    let long: String = "é".repeat(600);
    p.say(&long).await.expect("say");

    let base = |c: &Call| {
        assert_eq!(c.auth, format!("Bearer {ACCESS}"));
        assert_eq!(get(&c.query, "broadcaster_id"), Some(BROADCASTER_ID));
        assert_eq!(get(&c.query, "moderator_id"), Some(BOT_ID));
    };
    let del = h.mock.calls_to("DELETE", "/helix/moderation/chat");
    assert_eq!(del.len(), 1);
    base(&del[0]);
    assert_eq!(get(&del[0].query, "message_id"), Some("msg-1"));

    let bans = h.mock.calls_to("POST", "/helix/moderation/bans");
    assert_eq!(bans.len(), 2);
    base(&bans[0]);
    assert_eq!(
        bans[0].json(),
        json!({"data": {"user_id": "u-2", "duration": 600, "reason": "spam"}})
    );
    base(&bans[1]);
    assert_eq!(
        bans[1].json(),
        json!({"data": {"user_id": "u-3", "reason": "slurs"}})
    );

    let unban = h.mock.calls_to("DELETE", "/helix/moderation/bans");
    assert_eq!(unban.len(), 1);
    base(&unban[0]);
    assert_eq!(get(&unban[0].query, "user_id"), Some("u-3"));

    let say = h.mock.calls_to("POST", "/helix/chat/messages");
    assert_eq!(say.len(), 1);
    assert_eq!(say[0].auth, format!("Bearer {ACCESS}"));
    let body = say[0].json();
    assert_eq!(body["broadcaster_id"], BROADCASTER_ID);
    assert_eq!(body["sender_id"], BOT_ID);
    assert_eq!(
        body["message"].as_str().map(|m| m.chars().count()),
        Some(500)
    );

    // An empty message id would clear the whole chat on Twitch: refused before any call.
    assert!(p.delete("").await.is_err());
    assert_eq!(h.mock.calls_to("DELETE", "/helix/moderation/chat").len(), 1);
}

#[tokio::test]
async fn a_401_refreshes_once_then_retries() {
    let mut h = harness().await;
    let (p, _ws, _rx) = h.connected().await;
    *h.mock.reject.lock().expect("lock") = Some(ACCESS.into());

    p.ban("u-9", "spam").await.expect("ban after refresh");

    let bans = h.mock.calls_to("POST", "/helix/moderation/bans");
    assert_eq!(bans.len(), 2);
    assert_eq!(bans[0].auth, format!("Bearer {ACCESS}"));
    assert_eq!(bans[1].auth, format!("Bearer {ACCESS2}"));
    let refresh: Vec<_> = h
        .mock
        .calls_to("POST", "/oauth2/token")
        .into_iter()
        .filter(|c| get(&c.form(), "grant_type") == Some("refresh_token"))
        .collect();
    assert_eq!(refresh.len(), 1);
    assert!(get(&refresh[0].form(), "client_secret").is_none());

    let saved = h.store.load().expect("load").expect("entry");
    assert_eq!(saved.access_token.expose(), ACCESS2);
    assert_eq!(saved.refresh_token.expose(), REFRESH2);
    assert_eq!(saved.login, "modbot");
}

#[tokio::test]
async fn a_429_fails_fast_until_the_reset() {
    let mut h = harness().await;
    let (p, _ws, _rx) = h.connected().await;
    h.mock.rate_limit.store(true, Ordering::SeqCst);
    let e = p.delete("m-1").await.expect_err("429");
    assert!(e.0.contains("rate limited"), "{e}");
    let before = h.mock.calls().len();
    let e2 = p.say("hello").await.expect_err("still limited");
    assert!(e2.0.contains("rate limited"), "{e2}");
    assert_eq!(
        h.mock.calls().len(),
        before,
        "a call went out before the reset"
    );
}

// ---- (b) no token in Debug or errors --------------------------------------------------------

#[tokio::test]
async fn tokens_never_reach_debug_or_errors() {
    let mut h = harness().await;
    let (p, _ws, _rx) = h.connected().await;

    assert_no_secret(&format!("{p:?}"));
    let cred = h.store.load().expect("load").expect("entry");
    assert_no_secret(&format!("{cred:?}"));
    assert_no_secret(&format!("{:?}", p.status()));

    // A server that echoes the token back in its error message.
    h.mock.echo_403.store(true, Ordering::SeqCst);
    let e = p.say("hello").await.expect_err("403");
    assert!(e.0.contains("HTTP 403"), "{e}");
    assert_no_secret(&format!("{e:?}"));
    assert_no_secret(&e.to_string());
    h.mock.echo_403.store(false, Ordering::SeqCst);

    // A dead login: 401, then a refresh Twitch refuses.
    *h.mock.reject.lock().expect("lock") = Some(ACCESS.into());
    {
        let store = h.store.clone();
        let mut c = store.load().expect("load").expect("entry");
        c.refresh_token = Secret::new("tok-REFRESH-revoked-fictional");
        store.save(&c).expect("save");
        p.set_cred(Some(c));
    }
    let e = p.ban("u-1", "x").await.expect_err("refresh refused");
    assert!(e.0.contains("twitch.connect"), "{e}");
    assert_no_secret(&format!("{e:?}"));
    assert!(!format!("{e:?}").contains("tok-REFRESH-revoked"));
    assert_no_secret(&format!("{p:?}"));
    assert!(!format!("{p:?}").contains("tok-REFRESH-revoked"));
    assert_eq!(p.status().state, PlatformState::Error);
}

// ---- (c) disconnect -------------------------------------------------------------------------

#[tokio::test]
async fn disconnect_deletes_the_credential_and_goes_disconnected() {
    let mut h = harness().await;
    let (p, mut ws, _rx) = h.connected().await;
    assert!(h.store.raw().is_some());

    p.disconnect().await;

    assert!(h.store.raw().is_none(), "the stored credential survived");
    let st = p.status();
    assert_eq!(st.state, PlatformState::Disconnected);
    assert_eq!(st.bot_login, None);
    let revoke = h.mock.calls_to("POST", "/oauth2/revoke");
    assert_eq!(revoke.len(), 1);
    assert_eq!(get(&revoke[0].form(), "token"), Some(ACCESS));
    assert!(p.say("hi").await.is_err());
    // The session socket is gone.
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "the socket stayed open");
}

#[tokio::test]
async fn status_names_a_missing_client_id() {
    let p = TwitchPlatform::with_endpoints(
        TwitchConfig {
            client_id: String::new(),
            channel: "chan".into(),
        },
        Endpoints::default(),
        Arc::new(MemoryStore::new()),
    );
    let (tx, _rx) = mpsc::channel(1);
    p.start(tx);
    let st = p.status();
    assert_eq!(st.state, PlatformState::Disconnected);
    assert_eq!(
        st.detail.as_deref(),
        Some("no [twitch] client_id configured")
    );
    assert_eq!(st.channel.as_deref(), Some("chan"));
    assert!(p.begin_device_login().await.is_err());
}

#[test]
fn truncation_keeps_char_boundaries() {
    assert_eq!(super::truncate_chars("héllo", 2), "hé");
    assert_eq!(super::truncate_chars("hi", 500), "hi");
}
