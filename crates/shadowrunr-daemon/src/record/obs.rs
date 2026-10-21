//! A small obs-websocket v5 client: what the recorder needs, nothing more.
//!
//! - **Handshake.** The server sends `Hello` (op 0). When it asks for authentication, the client
//!   answers with `Identify` (op 1) carrying [`auth_string`]: `base64(sha256(base64(sha256(
//!   password + salt)) + challenge))`. The server confirms with `Identified` (op 2), or closes
//!   with 4009 when the password is wrong.
//! - **Requests.** One at a time: `Request` (op 6) with a fresh `requestId`, answered by
//!   `RequestResponse` (op 7) with the same id. A response whose `requestStatus.result` is false
//!   is an [`ObsError::Request`] with OBS's code and comment.
//! - **Events** (op 5) arrive only for the subscriptions asked for in `Identify`; they are kept
//!   in arrival order for [`Obs::wait_event`].
//!
//! The password is never logged or put in an error: errors name what failed, not what was sent.

use std::collections::VecDeque;
use std::time::Duration;

use base64::Engine as _;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// The RPC version this client speaks.
pub const RPC_VERSION: u64 = 1;
/// The JSON subprotocol.
pub const SUBPROTOCOL: &str = "obswebsocket.json";
/// `eventSubscriptions` bit for output events (`RecordStateChanged`).
pub const EVENTS_OUTPUTS: u64 = 1 << 6;

/// Request status codes the recorder tells apart.
pub mod code {
    pub const SUCCESS: i64 = 100;
    pub const OUTPUT_RUNNING: i64 = 500;
    pub const OUTPUT_NOT_RUNNING: i64 = 501;
    pub const RESOURCE_NOT_FOUND: i64 = 600;
    pub const RESOURCE_ALREADY_EXISTS: i64 = 601;
}

/// The close code OBS sends when the password is wrong.
const AUTH_FAILED: u16 = 4009;

/// How long the connection and the handshake may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long one request may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub enum ObsError {
    /// Nothing answered at the address (OBS not running, or its websocket server off).
    Unreachable(String),
    /// OBS wants a password and none was given, or refused the one given.
    Auth(String),
    /// Something that is not obs-websocket v5 answered, or the connection dropped.
    Protocol(String),
    /// OBS refused a request.
    Request {
        request: String,
        code: i64,
        comment: String,
    },
    Timeout(String),
}

impl ObsError {
    /// The request status code, for a refused request.
    pub fn code(&self) -> Option<i64> {
        match self {
            Self::Request { code, .. } => Some(*code),
            _ => None,
        }
    }
}

impl std::fmt::Display for ObsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(m) => write!(f, "OBS is not reachable: {m}"),
            Self::Auth(m) => write!(f, "OBS refused the connection: {m}"),
            Self::Protocol(m) => write!(f, "OBS websocket: {m}"),
            Self::Request {
                request,
                code,
                comment,
            } => {
                write!(f, "OBS refused {request} (code {code})")?;
                if !comment.is_empty() {
                    write!(f, ": {comment}")?;
                }
                Ok(())
            }
            Self::Timeout(m) => write!(f, "OBS did not answer in time: {m}"),
        }
    }
}

impl std::error::Error for ObsError {}

/// The `Identify` authentication string for `password` and the `Hello`'s salt and challenge.
pub fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let secret = b64.encode(Sha256::digest(format!("{password}{salt}").as_bytes()));
    b64.encode(Sha256::digest(format!("{secret}{challenge}").as_bytes()))
}

/// The `Identify` message answering `hello`.
pub fn identify(hello: &Value, password: Option<&str>, events: u64) -> Result<Value, ObsError> {
    let rpc = hello["rpcVersion"].as_u64().unwrap_or(RPC_VERSION);
    if rpc < RPC_VERSION {
        return Err(ObsError::Protocol(format!(
            "the server speaks rpc version {rpc}; this client needs {RPC_VERSION}"
        )));
    }
    let mut d = json!({ "rpcVersion": RPC_VERSION, "eventSubscriptions": events });
    if let Some(auth) = hello.get("authentication") {
        let (Some(salt), Some(challenge)) = (auth["salt"].as_str(), auth["challenge"].as_str())
        else {
            return Err(ObsError::Protocol(
                "a Hello with a malformed challenge".into(),
            ));
        };
        let Some(password) = password else {
            return Err(ObsError::Auth(
                "it asks for a password and none is configured".into(),
            ));
        };
        d["authentication"] = Value::String(auth_string(password, salt, challenge));
    }
    Ok(json!({ "op": 1, "d": d }))
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// One identified connection.
pub struct Obs {
    ws: Socket,
    next_id: u64,
    events: VecDeque<Value>,
    /// The `Hello`'s `obsWebSocketVersion`.
    pub websocket_version: String,
    /// Whether the server asked for a password.
    pub authenticated: bool,
}

impl Obs {
    /// Connect to `url` (`ws://host:port`), identify, and subscribe to `events`.
    pub async fn connect(url: &str, password: Option<&str>, events: u64) -> Result<Self, ObsError> {
        tokio::time::timeout(CONNECT_TIMEOUT, Self::handshake(url, password, events))
            .await
            .map_err(|_| {
                ObsError::Timeout(format!(
                    "no handshake within {} s at {url}",
                    CONNECT_TIMEOUT.as_secs()
                ))
            })?
    }

    async fn handshake(url: &str, password: Option<&str>, events: u64) -> Result<Self, ObsError> {
        let mut req = url
            .into_client_request()
            .map_err(|e| ObsError::Unreachable(format!("{url}: {e}")))?;
        req.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_static(SUBPROTOCOL),
        );
        let (ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| ObsError::Unreachable(format!("{url}: {e}")))?;
        let mut obs = Self {
            ws,
            next_id: 0,
            events: VecDeque::new(),
            websocket_version: String::new(),
            authenticated: false,
        };
        let hello = obs.read_op(0).await?;
        obs.websocket_version = hello["obsWebSocketVersion"]
            .as_str()
            .unwrap_or("?")
            .to_string();
        obs.authenticated = hello.get("authentication").is_some();
        let msg = identify(&hello, password, events)?;
        obs.send(&msg).await?;
        obs.read_op(2).await?;
        Ok(obs)
    }

    async fn send(&mut self, v: &Value) -> Result<(), ObsError> {
        self.ws
            .send(Message::Text(v.to_string().into()))
            .await
            .map_err(|e| ObsError::Protocol(format!("send: {e}")))
    }

    /// The next message's `{op, d}`. A close is an error naming why.
    async fn next(&mut self) -> Result<(u64, Value), ObsError> {
        loop {
            let msg = match self.ws.next().await {
                Some(Ok(m)) => m,
                Some(Err(e)) => return Err(ObsError::Protocol(format!("read: {e}"))),
                None => return Err(ObsError::Protocol("the connection closed".into())),
            };
            let text = match msg {
                Message::Text(t) => t.to_string(),
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Close(frame) => {
                    let (code, reason) = frame
                        .map(|f| (u16::from(f.code), f.reason.to_string()))
                        .unwrap_or((u16::from(CloseCode::Normal), String::new()));
                    if code == AUTH_FAILED {
                        return Err(ObsError::Auth("the password was not accepted".into()));
                    }
                    return Err(ObsError::Protocol(format!(
                        "closed by OBS (code {code}{})",
                        if reason.is_empty() {
                            String::new()
                        } else {
                            format!(": {reason}")
                        }
                    )));
                }
                _ => continue,
            };
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| ObsError::Protocol(format!("not JSON: {e}")))?;
            let Some(op) = v["op"].as_u64() else {
                return Err(ObsError::Protocol("a message without op".into()));
            };
            let d = v.get("d").cloned().unwrap_or(Value::Null);
            return Ok((op, d));
        }
    }

    async fn read_op(&mut self, want: u64) -> Result<Value, ObsError> {
        loop {
            let (op, d) = self.next().await?;
            if op == want {
                return Ok(d);
            }
            if op == 5 {
                self.events.push_back(d);
            }
        }
    }

    /// Send one request and wait for its response's data (`{}` when it has none).
    pub async fn request(&mut self, request: &str, data: Value) -> Result<Value, ObsError> {
        tokio::time::timeout(REQUEST_TIMEOUT, self.request_now(request, data))
            .await
            .map_err(|_| ObsError::Timeout(request.to_string()))?
    }

    async fn request_now(&mut self, request: &str, data: Value) -> Result<Value, ObsError> {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let mut d = json!({ "requestType": request, "requestId": id });
        if !data.is_null() {
            d["requestData"] = data;
        }
        self.send(&json!({ "op": 6, "d": d })).await?;
        loop {
            let resp = self.read_op(7).await?;
            if resp["requestId"].as_str() != Some(id.as_str()) {
                continue;
            }
            let status = &resp["requestStatus"];
            if status["result"].as_bool() == Some(true) {
                return Ok(resp.get("responseData").cloned().unwrap_or(json!({})));
            }
            return Err(ObsError::Request {
                request: request.to_string(),
                code: status["code"].as_i64().unwrap_or(0),
                comment: status["comment"].as_str().unwrap_or("").to_string(),
            });
        }
    }

    /// The first event of `event_type` whose data passes `pred`, kept or still to come,
    /// within `timeout`. Earlier events of other kinds are dropped.
    pub async fn wait_event(
        &mut self,
        event_type: &str,
        pred: impl Fn(&Value) -> bool,
        timeout: Duration,
    ) -> Result<Value, ObsError> {
        let found =
            |e: &Value| e["eventType"].as_str() == Some(event_type) && pred(&e["eventData"]);
        let by = tokio::time::Instant::now() + timeout;
        loop {
            while let Some(e) = self.events.pop_front() {
                if found(&e) {
                    return Ok(e["eventData"].clone());
                }
            }
            match tokio::time::timeout_at(by, self.next()).await {
                Err(_) => {
                    return Err(ObsError::Timeout(format!(
                        "no {event_type} within {} s",
                        timeout.as_secs()
                    )))
                }
                Ok(r) => {
                    let (op, d) = r?;
                    if op == 5 && found(&d) {
                        return Ok(d["eventData"].clone());
                    }
                }
            }
        }
    }

    /// Close the connection politely.
    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

#[cfg(test)]
#[allow(clippy::result_large_err)] // the handshake callback's error type is tungstenite's
pub(crate) mod fake {
    //! A fake obs-websocket server: one connection per test, scripted by a closure over the
    //! client's requests.

    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;

    pub const SALT: &str = "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=";
    pub const CHALLENGE: &str = "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=";

    /// What the fake saw and answered.
    #[derive(Debug, Default, Clone)]
    pub struct Seen {
        pub subprotocol: Option<String>,
        pub identify: Option<Value>,
        pub requests: Vec<Value>,
    }

    /// Serve one connection on a free port; `answer` maps each request to
    /// `(status code, responseData, events to send after it)`. Returns the URL and what the
    /// server saw once the client is gone.
    pub async fn serve<F>(
        password: Option<&'static str>,
        answer: F,
    ) -> (String, tokio::task::JoinHandle<Seen>)
    where
        F: Fn(&str, &Value) -> (i64, Value, Vec<Value>) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut seen = Seen::default();
            let (tcp, _) = listener.accept().await.unwrap();
            let proto = std::sync::Arc::new(std::sync::Mutex::new(None));
            let p2 = proto.clone();
            let mut ws = tokio_tungstenite::accept_hdr_async(
                tcp,
                move |req: &Request, mut resp: Response| {
                    let asked = req
                        .headers()
                        .get("Sec-WebSocket-Protocol")
                        .and_then(|v| v.to_str().ok())
                        .map(String::from);
                    if asked.as_deref() == Some(SUBPROTOCOL) {
                        resp.headers_mut().insert(
                            "Sec-WebSocket-Protocol",
                            HeaderValue::from_static(SUBPROTOCOL),
                        );
                    }
                    *p2.lock().unwrap() = asked;
                    Ok(resp)
                },
            )
            .await
            .unwrap();
            seen.subprotocol = proto.lock().unwrap().clone();
            let mut hello = json!({"obsWebSocketVersion": "5.9.9", "rpcVersion": 1});
            if password.is_some() {
                hello["authentication"] = json!({"challenge": CHALLENGE, "salt": SALT});
            }
            let send = |v: Value| Message::Text(v.to_string().into());
            ws.send(send(json!({"op": 0, "d": hello}))).await.unwrap();
            while let Some(Ok(msg)) = ws.next().await {
                let Message::Text(t) = msg else { continue };
                let v: Value = serde_json::from_str(&t).unwrap();
                match v["op"].as_u64() {
                    Some(1) => {
                        seen.identify = Some(v["d"].clone());
                        let ok = match password {
                            None => true,
                            Some(p) => {
                                v["d"]["authentication"].as_str()
                                    == Some(auth_string(p, SALT, CHALLENGE).as_str())
                            }
                        };
                        if !ok {
                            let _ = ws
                                .send(Message::Close(Some(CloseFrame {
                                    code: CloseCode::from(AUTH_FAILED),
                                    reason: "Authentication failed.".into(),
                                })))
                                .await;
                            break;
                        }
                        ws.send(send(json!({"op": 2, "d": {"negotiatedRpcVersion": 1}})))
                            .await
                            .unwrap();
                    }
                    Some(6) => {
                        let d = v["d"].clone();
                        seen.requests.push(d.clone());
                        let ty = d["requestType"].as_str().unwrap_or("");
                        let data = d.get("requestData").cloned().unwrap_or(Value::Null);
                        let (code, resp, events) = answer(ty, &data);
                        // An unrelated event first: the client must skip it.
                        ws.send(send(json!({"op": 5, "d": {"eventType": "CurrentSceneTransitionChanged", "eventIntent": 1, "eventData": {}}})))
                            .await
                            .unwrap();
                        let mut r = json!({
                            "requestType": ty,
                            "requestId": d["requestId"],
                            "requestStatus": {"result": code == code::SUCCESS, "code": code},
                        });
                        if code != code::SUCCESS {
                            r["requestStatus"]["comment"] = json!("scripted refusal");
                        } else if !resp.is_null() {
                            r["responseData"] = resp;
                        }
                        ws.send(send(json!({"op": 7, "d": r}))).await.unwrap();
                        for e in events {
                            ws.send(send(json!({"op": 5, "d": e}))).await.unwrap();
                        }
                    }
                    _ => {}
                }
            }
            seen
        });
        (url, task)
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{serve, CHALLENGE, SALT};
    use super::*;

    #[test]
    fn the_auth_string_is_the_v5_construction() {
        // Computed independently: base64(sha256(base64(sha256(password + salt)) + challenge)).
        assert_eq!(
            auth_string("supersecretpassword", SALT, CHALLENGE),
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
        );
        let hello =
            json!({"rpcVersion": 1, "authentication": {"salt": SALT, "challenge": CHALLENGE}});
        let id = identify(&hello, Some("supersecretpassword"), EVENTS_OUTPUTS).unwrap();
        assert_eq!(id["op"], 1);
        assert_eq!(id["d"]["eventSubscriptions"], EVENTS_OUTPUTS);
        assert_eq!(
            id["d"]["authentication"],
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
        );
        // No auth asked: none sent.
        let open = identify(&json!({"rpcVersion": 1}), Some("x"), 0).unwrap();
        assert!(open["d"].get("authentication").is_none());
        // Auth asked, no password: refused before anything is sent.
        assert!(matches!(identify(&hello, None, 0), Err(ObsError::Auth(_))));
    }

    #[tokio::test]
    async fn requests_are_framed_by_id_and_refusals_carry_obs_code() {
        let (url, server) = serve(Some("pw"), |ty, data| match ty {
            "GetVersion" => (
                code::SUCCESS,
                json!({"obsVersion": "32.2.2", "obsWebSocketVersion": "5.9.9"}),
                vec![],
            ),
            "CreateScene" if data["sceneName"] == "Taken" => {
                (code::RESOURCE_ALREADY_EXISTS, Value::Null, vec![])
            }
            "StartRecord" => (
                code::SUCCESS,
                Value::Null,
                vec![json!({"eventType": "RecordStateChanged", "eventIntent": 64,
                    "eventData": {"outputActive": true, "outputState": "OBS_WEBSOCKET_OUTPUT_STARTED"}})],
            ),
            _ => (code::SUCCESS, Value::Null, vec![]),
        })
        .await;
        let mut obs = Obs::connect(&url, Some("pw"), EVENTS_OUTPUTS)
            .await
            .unwrap();
        assert!(obs.authenticated);
        assert_eq!(obs.websocket_version, "5.9.9");
        let v = obs.request("GetVersion", Value::Null).await.unwrap();
        assert_eq!(v["obsVersion"], "32.2.2");
        let err = obs
            .request("CreateScene", json!({"sceneName": "Taken"}))
            .await
            .unwrap_err();
        assert_eq!(err.code(), Some(code::RESOURCE_ALREADY_EXISTS));
        assert!(err.to_string().contains("CreateScene"), "{err}");
        assert_eq!(
            obs.request("StartRecord", Value::Null).await.unwrap(),
            json!({})
        );
        let started = obs
            .wait_event(
                "RecordStateChanged",
                |d| d["outputState"] == "OBS_WEBSOCKET_OUTPUT_STARTED",
                Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert_eq!(started["outputActive"], true);
        assert!(matches!(
            obs.wait_event("RecordStateChanged", |_| true, Duration::from_millis(50))
                .await,
            Err(ObsError::Timeout(_))
        ));
        obs.close().await;
        let seen = server.await.unwrap();
        assert_eq!(seen.subprotocol.as_deref(), Some(SUBPROTOCOL));
        assert_eq!(seen.identify.unwrap()["eventSubscriptions"], EVENTS_OUTPUTS);
        let ids: Vec<&str> = seen
            .requests
            .iter()
            .map(|r| r["requestId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["1", "2", "3"]);
        assert!(seen.requests[0].get("requestData").is_none());
        assert_eq!(seen.requests[1]["requestData"]["sceneName"], "Taken");
    }

    #[tokio::test]
    async fn a_wrong_password_is_an_auth_error_that_never_names_it() {
        let (url, server) = serve(Some("right"), |_, _| (code::SUCCESS, Value::Null, vec![])).await;
        let err = Obs::connect(&url, Some("wrong-secret"), 0)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ObsError::Auth(_)), "{err:?}");
        assert!(!err.to_string().contains("wrong-secret"));
        server.await.unwrap();

        let (url, server) = serve(Some("right"), |_, _| (code::SUCCESS, Value::Null, vec![])).await;
        let err = Obs::connect(&url, None, 0).await.err().unwrap();
        assert!(err.to_string().contains("none is configured"), "{err}");
        drop(server);
    }

    #[tokio::test]
    async fn nothing_listening_is_unreachable() {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let err = Obs::connect(&format!("ws://127.0.0.1:{port}"), None, 0)
            .await
            .err()
            .unwrap();
        assert!(matches!(err, ObsError::Unreachable(_)), "{err:?}");
    }
}
