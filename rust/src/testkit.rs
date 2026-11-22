//! A fake Slack for tests: records every Web API call and `response_url` post, answers queued replies FIFO per
//! method, and answers `{"ok": true}` when nothing is queued.
use crate::slack::{Slack, SlackError};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct FakeSlack {
    calls: Mutex<Vec<(String, Value)>>,
    responses: Mutex<Vec<(String, Value)>>,
    queued: Mutex<HashMap<String, VecDeque<Result<Value, SlackError>>>>,
}

impl FakeSlack {
    pub fn new() -> Arc<FakeSlack> {
        Arc::new(FakeSlack::default())
    }

    /// Queues one successful reply for `method`.
    pub fn reply(&self, method: &str, response: Value) {
        self.queued.lock().unwrap().entry(method.into()).or_default().push_back(Ok(response));
    }

    /// Queues one failure for `method`.
    pub fn fail(&self, method: &str, error: &str) {
        self.queued.lock().unwrap().entry(method.into()).or_default().push_back(Err(SlackError::Api(error.into())));
    }

    /// Every call as (method, params), in order.
    pub fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }

    /// The params of every call to `method`, in order.
    pub fn calls_to(&self, method: &str) -> Vec<Value> {
        self.calls().into_iter().filter(|(m, _)| m == method).map(|(_, p)| p).collect()
    }

    /// The text of every `chat.postMessage` (DMs and stupid lines), in order.
    pub fn posted_texts(&self) -> Vec<String> {
        self.calls_to("chat.postMessage").iter().map(|p| p["text"].as_str().unwrap_or_default().to_string()).collect()
    }

    /// Every `response_url` post, as (url, body).
    pub fn responded(&self) -> Vec<(String, Value)> {
        self.responses.lock().unwrap().clone()
    }

    /// Returns and clears the calls recorded since the last take.
    pub fn take_calls(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    /// Returns and clears the `response_url` posts recorded since the last take.
    pub fn take_responded(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.responses.lock().unwrap())
    }
}

impl Slack for FakeSlack {
    async fn call(&self, method: &str, params: Value) -> Result<Value, SlackError> {
        self.calls.lock().unwrap().push((method.to_string(), params));
        let queued = self.queued.lock().unwrap().get_mut(method).and_then(VecDeque::pop_front);
        queued.unwrap_or_else(|| Ok(json!({"ok": true})))
    }

    async fn call_user(&self, method: &str, params: Value) -> Result<Value, SlackError> {
        // Recorded and answered like a bot call: the method name identifies the call in the parity fixture.
        self.call(method, params).await
    }

    async fn respond(&self, url: &str, body: Value) -> Result<(), SlackError> {
        self.responses.lock().unwrap().push((url.to_string(), body));
        Ok(())
    }
}
