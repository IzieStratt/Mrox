//! The real Web API client: one shared rustls `reqwest` client, JSON bodies, and the retry policy of
//! `@slack/web-api` (10 retries; `Retry-After` on 429, exponential backoff on 5xx and transport errors).
use super::{Slack, SlackError};
use serde_json::Value;
use std::time::Duration;

const MAX_RETRIES: u32 = 10;

pub struct SlackClient {
    http: reqwest::Client,
    base: String,
    bot_token: String,
    user_token: Option<String>,
}

impl SlackClient {
    pub fn new(http: reqwest::Client, base: impl Into<String>, bot_token: impl Into<String>, user_token: Option<String>) -> SlackClient {
        SlackClient { http, base: base.into().trim_end_matches('/').to_string(), bot_token: bot_token.into(), user_token }
    }

    /// `minTimeout` 1s doubling per attempt, with jitter, as web-api's default retry config.
    fn backoff(attempt: u32) -> Duration {
        Duration::from_millis(1000u64 << attempt.min(6)) + Duration::from_millis(fastrand::u64(0..1000))
    }

    /// One Web API call with `token`, retried on 429, 5xx and transport errors. `ok: false` is an error.
    async fn send(&self, token: &str, method: &str, params: Value) -> Result<Value, SlackError> {
        let url = format!("{}/{method}", self.base);
        let mut attempt = 0;
        loop {
            let resp = match self.http.post(&url).bearer_auth(token).json(&params).send().await {
                Ok(r) => r,
                Err(e) => {
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(Self::backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(SlackError::Http(format!("{method}: {e}")));
                }
            };
            let status = resp.status();
            if status.as_u16() == 429 || status.is_server_error() {
                if attempt < MAX_RETRIES {
                    let retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
                    let wait = match retry_after {
                        Some(secs) if status.as_u16() == 429 => Duration::from_secs(secs),
                        _ => Self::backoff(attempt),
                    };
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                    continue;
                }
                return Err(SlackError::Http(format!("{method}: HTTP {status}")));
            }
            let text = resp.text().await.map_err(|e| SlackError::Http(format!("{method}: {e}")))?;
            let v: Value = serde_json::from_str(&text).map_err(|_| SlackError::Http(format!("{method}: bad response (HTTP {status})")))?;
            if v.get("ok").and_then(Value::as_bool) == Some(false) {
                let error = v.get("error").and_then(Value::as_str).unwrap_or("unknown_error").to_string();
                return Err(SlackError::Api(error));
            }
            return Ok(v);
        }
    }
}

impl Slack for SlackClient {
    async fn call(&self, method: &str, params: Value) -> Result<Value, SlackError> {
        self.send(&self.bot_token, method, params).await
    }

    async fn call_user(&self, method: &str, params: Value) -> Result<Value, SlackError> {
        match &self.user_token {
            Some(token) => self.send(token, method, params).await,
            None => Err(SlackError::Api("not_authed".into())),
        }
    }

    async fn respond(&self, url: &str, body: Value) -> Result<(), SlackError> {
        let resp = self.http.post(url).json(&body).send().await.map_err(|e| SlackError::Http(format!("response_url: {e}")))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(SlackError::Http(format!("response_url: HTTP {}", resp.status())))
        }
    }
}
