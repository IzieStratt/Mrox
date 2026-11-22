//! Slack access: the [`Slack`] trait the bot is written against, the real client, and the Socket Mode transport.
pub mod client;
pub mod socket;
pub mod ws;

pub use client::SlackClient;

use serde_json::Value;
use std::future::Future;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum SlackError {
    /// `{"ok": false, "error": ...}` from the Web API.
    Api(String),
    /// Transport failure, HTTP failure or an unparsable body.
    Http(String),
}

impl std::fmt::Display for SlackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlackError::Api(e) => write!(f, "An API error occurred: {e}"),
            SlackError::Http(e) => write!(f, "{e}"),
        }
    }
}

/// What the bot needs from Slack: Web API methods (bot token) and `response_url` posts. The futures are `Send`
/// because handlers run as spawned tasks.
pub trait Slack: Send + Sync + 'static {
    /// `POST {base}/{method}` with the bot token and a JSON body; returns the whole response object.
    fn call(&self, method: &str, params: Value) -> impl Future<Output = Result<Value, SlackError>> + Send;
    /// The same call with the optional user token (`SLACK_USER_TOKEN`), for `search.messages`.
    fn call_user(&self, method: &str, params: Value) -> impl Future<Output = Result<Value, SlackError>> + Send;
    /// POSTs a JSON body to a slash command's `response_url`.
    fn respond(&self, url: &str, body: Value) -> impl Future<Output = Result<(), SlackError>> + Send;
}

impl<T: Slack> Slack for Arc<T> {
    fn call(&self, method: &str, params: Value) -> impl Future<Output = Result<Value, SlackError>> + Send {
        (**self).call(method, params)
    }
    fn call_user(&self, method: &str, params: Value) -> impl Future<Output = Result<Value, SlackError>> + Send {
        (**self).call_user(method, params)
    }
    fn respond(&self, url: &str, body: Value) -> impl Future<Output = Result<(), SlackError>> + Send {
        (**self).respond(url, body)
    }
}
