//! The Socket Mode transport: `apps.connections.open` for a one-shot wss URL, then a tokio-tungstenite connection
//! over rustls. Copied from the music-stalker Rust port (`src/slack/ws.rs`) and trimmed to mrox's needs.
use super::socket::{Connection, Transport};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub struct WsTransport {
    pub client: reqwest::Client,
    pub api_base: String,
    pub app_token: String,
}

pub struct WsConnection {
    stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

/// `apps.connections.open` with the app-level token; returns the wss URL.
pub async fn open_connection_url(client: &reqwest::Client, base: &str, app_token: &str) -> Result<String, String> {
    let resp = client
        .post(format!("{base}/apps.connections.open"))
        .bearer_auth(app_token)
        .send()
        .await
        .map_err(|e| format!("apps.connections.open: {e}"))?;
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if v.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        v.get("url").and_then(serde_json::Value::as_str).map(str::to_owned).ok_or_else(|| "apps.connections.open: no url".into())
    } else {
        Err(format!("apps.connections.open: {}", v.get("error").and_then(serde_json::Value::as_str).unwrap_or("unknown_error")))
    }
}

impl Transport for WsTransport {
    type Conn = WsConnection;

    async fn connect(&self) -> Result<WsConnection, String> {
        let url = open_connection_url(&self.client, &self.api_base, &self.app_token).await?;
        let (stream, _) = tokio_tungstenite::connect_async(url).await.map_err(|e| e.to_string())?;
        Ok(WsConnection { stream })
    }
}

impl Connection for WsConnection {
    async fn recv(&mut self) -> Option<Result<String, String>> {
        loop {
            match self.stream.next().await? {
                Ok(Message::Text(t)) => return Some(Ok(t.to_string())),
                Ok(Message::Ping(p)) => {
                    if let Err(e) = self.stream.send(Message::Pong(p)).await {
                        return Some(Err(e.to_string()));
                    }
                }
                Ok(Message::Close(_)) => return None,
                Ok(_) => {}
                Err(e) => return Some(Err(e.to_string())),
            }
        }
    }

    async fn send(&mut self, text: String) -> Result<(), String> {
        self.stream.send(Message::Text(text.into())).await.map_err(|e| e.to_string())
    }
}
