//! Socket Mode: envelope parsing, acks (sent before handling, as Bolt does), routing to the bot's listeners, and
//! the reconnect loop. The wire sits behind [`Transport`]/[`Connection`] so the loop is tested without a network.
use serde_json::{json, Value};
use std::time::Duration;

/// Seconds to wait after a failed connect or a socket error before retrying.
pub const RETRY_DELAY: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq)]
pub enum Envelope {
    Hello,
    Disconnect { reason: String },
    EventsApi { id: String, payload: Value },
    SlashCommand { id: String, payload: Value },
    Interactive { id: String, payload: Value },
    Other,
}

impl Envelope {
    pub fn parse(text: &str) -> Result<Envelope, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("bad envelope: {e}"))?;
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
        let id = v.get("envelope_id").and_then(Value::as_str).unwrap_or("").to_string();
        let payload = v.get("payload").cloned().unwrap_or(Value::Null);
        Ok(match ty {
            "hello" => Envelope::Hello,
            "disconnect" => Envelope::Disconnect { reason: v.get("reason").and_then(Value::as_str).unwrap_or("").to_string() },
            "events_api" => Envelope::EventsApi { id, payload },
            "slash_commands" => Envelope::SlashCommand { id, payload },
            "interactive" => Envelope::Interactive { id, payload },
            _ => Envelope::Other,
        })
    }

    /// The ack to send right away: every event, command and interaction envelope is acknowledged.
    pub fn ack(&self) -> Option<String> {
        match self {
            Envelope::EventsApi { id, .. } | Envelope::SlashCommand { id, .. } | Envelope::Interactive { id, .. } => Some(json!({"envelope_id": id}).to_string()),
            _ => None,
        }
    }
}

/// What the bot should do with an acknowledged envelope (the listeners of `test.ts`).
#[derive(Clone, Debug, PartialEq)]
pub enum Dispatch {
    /// `app.event("message")`: the event object.
    Message(Value),
    /// `app.event("app_home_opened")`: the event object.
    HomeOpened(Value),
    /// `app.command("/mrox")`: the slash command payload.
    Command(Value),
    /// `app.action("track_confession_input")`: the interaction body and its action.
    TrackInput { body: Value, action: Value },
    /// `app.action("untrack_confession")`: the interaction body and its action.
    Untrack { body: Value, action: Value },
}

/// Maps an envelope to a listener. Anything else (other events, commands, action ids) is acked and ignored.
pub fn route(env: &Envelope) -> Option<Dispatch> {
    match env {
        Envelope::EventsApi { payload, .. } => {
            let event = payload.get("event")?;
            match event.get("type").and_then(Value::as_str)? {
                "message" => Some(Dispatch::Message(event.clone())),
                "app_home_opened" => Some(Dispatch::HomeOpened(event.clone())),
                _ => None,
            }
        }
        Envelope::SlashCommand { payload, .. } => {
            (payload.get("command").and_then(Value::as_str) == Some("/mrox")).then(|| Dispatch::Command(payload.clone()))
        }
        Envelope::Interactive { payload, .. } => {
            if payload.get("type").and_then(Value::as_str) != Some("block_actions") {
                return None;
            }
            let action = payload.get("actions").and_then(Value::as_array).and_then(|a| a.first())?.clone();
            match action.get("action_id").and_then(Value::as_str)? {
                "track_confession_input" => Some(Dispatch::TrackInput { body: payload.clone(), action }),
                "untrack_confession" => Some(Dispatch::Untrack { body: payload.clone(), action }),
                _ => None,
            }
        }
        _ => None,
    }
}

#[allow(async_fn_in_trait)]
pub trait Connection {
    /// Next text frame: None when the peer closed, Err on a socket error.
    async fn recv(&mut self) -> Option<Result<String, String>>;
    async fn send(&mut self, text: String) -> Result<(), String>;
}

#[allow(async_fn_in_trait)]
pub trait Transport {
    type Conn: Connection;
    async fn connect(&self) -> Result<Self::Conn, String>;
}

/// Receives acknowledged envelopes. Implementations must not block (the bot spawns its handlers).
pub trait Sink {
    fn dispatch(&self, env: Envelope);
}

/// One connection: ack each envelope, then hand it on. Returns Ok when the peer closes or Slack asks for a
/// reconnect (`disconnect`, except the `warning` heads-up), Err on a socket error.
pub async fn session<C: Connection, K: Sink>(mut conn: C, sink: &K) -> Result<(), String> {
    while let Some(frame) = conn.recv().await {
        let text = frame?;
        let env = match Envelope::parse(&text) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[socket-mode] {e}");
                continue;
            }
        };
        match &env {
            Envelope::Hello | Envelope::Other => continue,
            Envelope::Disconnect { reason } if reason == "warning" => continue,
            Envelope::Disconnect { .. } => return Ok(()),
            _ => {}
        }
        if let Some(ack) = env.ack() {
            conn.send(ack).await?;
        }
        sink.dispatch(env);
    }
    Ok(())
}

/// Connects once (the caller treats that failure as fatal, like `app.start()`), then serves forever: a session
/// that ends normally reconnects at once, a socket error or failed connect waits [`RETRY_DELAY`].
pub async fn serve<T: Transport, K: Sink>(transport: &T, sink: &K, mut conn: T::Conn) {
    loop {
        if let Err(e) = session(conn, sink).await {
            eprintln!("Socket Mode error: {e}");
            tokio::time::sleep(RETRY_DELAY).await;
        }
        conn = loop {
            match transport.connect().await {
                Ok(c) => break c,
                Err(e) => {
                    eprintln!("Socket Mode error: {e}");
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct FakeConn {
        incoming: VecDeque<Option<Result<String, String>>>,
        sent: Arc<Mutex<Vec<String>>>,
    }
    impl Connection for FakeConn {
        async fn recv(&mut self) -> Option<Result<String, String>> {
            self.incoming.pop_front().flatten()
        }
        async fn send(&mut self, text: String) -> Result<(), String> {
            self.sent.lock().unwrap().push(text);
            Ok(())
        }
    }

    #[derive(Default)]
    struct Seen(Mutex<Vec<Envelope>>);
    impl Sink for Seen {
        fn dispatch(&self, e: Envelope) {
            self.0.lock().unwrap().push(e);
        }
    }

    const HELLO: &str = r#"{"type":"hello","num_connections":1}"#;
    const EVENT: &str = r#"{"type":"events_api","envelope_id":"e1","payload":{"event":{"type":"message"}}}"#;

    fn frames(list: &[&str]) -> FakeConn {
        FakeConn {
            incoming: list.iter().map(|f| Some(Ok(f.to_string()))).collect(),
            sent: Arc::default(),
        }
    }

    #[tokio::test]
    async fn acks_before_dispatch_and_stops_on_refresh() {
        let conn = frames(&[
            HELLO,
            EVENT,
            r#"{"type":"disconnect","reason":"warning"}"#,
            r#"{"type":"slash_commands","envelope_id":"s1","payload":{"command":"/mrox"}}"#,
            r#"{"type":"disconnect","reason":"refresh_requested"}"#,
            EVENT,
        ]);
        let sent = conn.sent.clone();
        let seen = Seen::default();
        session(conn, &seen).await.unwrap();
        assert_eq!(sent.lock().unwrap().as_slice(), [r#"{"envelope_id":"e1"}"#, r#"{"envelope_id":"s1"}"#]);
        assert_eq!(seen.0.lock().unwrap().len(), 2, "the event after refresh_requested is never read");
    }

    #[test]
    fn envelope_parsing_and_acks() {
        assert_eq!(Envelope::parse(HELLO).unwrap(), Envelope::Hello);
        assert_eq!(Envelope::parse(EVENT).unwrap().ack().unwrap(), r#"{"envelope_id":"e1"}"#);
        assert!(Envelope::Hello.ack().is_none());
        assert!(Envelope::parse("not json").is_err());
    }

    #[test]
    fn routes_only_the_listeners_mrox_has() {
        let msg = Envelope::parse(EVENT).unwrap();
        assert!(matches!(route(&msg), Some(Dispatch::Message(_))));
        let home = Envelope::parse(r#"{"type":"events_api","envelope_id":"h","payload":{"event":{"type":"app_home_opened","user":"U1"}}}"#).unwrap();
        assert!(matches!(route(&home), Some(Dispatch::HomeOpened(_))));
        let other = Envelope::parse(r#"{"type":"events_api","envelope_id":"o","payload":{"event":{"type":"reaction_added"}}}"#).unwrap();
        assert!(route(&other).is_none());
        let cmd = Envelope::parse(r#"{"type":"slash_commands","envelope_id":"c","payload":{"command":"/other"}}"#).unwrap();
        assert!(route(&cmd).is_none());
        let act = Envelope::parse(r#"{"type":"interactive","envelope_id":"a","payload":{"type":"block_actions","actions":[{"action_id":"untrack_confession","value":"1"}]}}"#).unwrap();
        assert!(matches!(route(&act), Some(Dispatch::Untrack { .. })));
        let view = Envelope::parse(r#"{"type":"interactive","envelope_id":"v","payload":{"type":"view_submission"}}"#).unwrap();
        assert!(route(&view).is_none());
    }

    struct Scripted {
        plans: Mutex<VecDeque<Result<Vec<&'static str>, String>>>,
        connects: Mutex<usize>,
    }
    impl Transport for Scripted {
        type Conn = FakeConn;
        async fn connect(&self) -> Result<FakeConn, String> {
            *self.connects.lock().unwrap() += 1;
            match self.plans.lock().unwrap().pop_front().unwrap_or(Err("no more".into())) {
                Ok(f) => Ok(frames(&f)),
                Err(e) => Err(e),
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reconnects_after_a_failed_connect_with_a_ten_second_wait() {
        let t = Scripted { plans: Mutex::new(VecDeque::from([Err("boom".into()), Ok(vec![HELLO, EVENT])])), connects: Mutex::new(0) };
        let seen = Seen::default();
        let start = tokio::time::Instant::now();
        // serve() never returns; run it until the scripted plans are used up, then stop.
        let _ = tokio::time::timeout(Duration::from_secs(30), serve(&t, &seen, frames(&[HELLO]))).await;
        assert!(*t.connects.lock().unwrap() >= 2);
        assert!(start.elapsed() >= RETRY_DELAY);
        assert_eq!(seen.0.lock().unwrap().len(), 1);
    }
}
