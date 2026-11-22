//! The listeners of `test.ts`: `/mrox`, the message listener, App Home and its two actions, the status flow
//! (`findPostTs`, `resolvePostTs`, `getDenialReason`, `stupidThing`), the deferred rejection DM with its timers,
//! and the status lookback when tracking starts. Every Slack call goes through [`Slack::call`] with the same
//! method and parameter names as the TypeScript code.
use crate::config::{Messages, Settings, HISTORY_SCAN_LIMIT, PRIVACY_POLICY_URL, RETENTION_MS};
use crate::js::{self, date_ms, iso_from_ms, js_min, js_string, str_eq, timeout_delay_ms, truthy, Clock};
use crate::slack::socket::Dispatch;
use crate::slack::Slack;
use crate::store::Store;
use regex_lite::Regex;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::task::JoinHandle;

/// `Bot` holds the settings, the message catalogue, the data file, the Slack client and the clock.
pub struct Bot<S: Slack> {
    pub cfg: Settings,
    pub msgs: Messages,
    pub store: Store,
    pub slack: S,
    pub clock: Clock,
    /// Handle to ourselves, for the timers (`setTimeout` callbacks outlive the event that set them).
    me: Weak<Bot<S>>,
    /// `rejectionTimers`: the pending rejection DM per confession id.
    rejection_timers: Mutex<HashMap<String, JoinHandle<()>>>,
}

/// The reply to a command or action, plus the id whose status lookback must run alongside the reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub lookback: Option<String>,
}

impl Reply {
    fn plain(text: String) -> Reply {
        Reply { text, lookback: None }
    }
}

/// The status kinds `classify` returns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Meta,
    Approved,
    Rejected,
    Unrejected,
    Unapproved,
    None,
}

/// `classify`: the order of the checks matters (meta, then unapproved, unrejected, approved, rejected/denied).
pub fn classify(info: &str, channel_meta: &str) -> Kind {
    let lower = info.to_lowercase();
    // Owl writes "approved to <#CHANNEL>"; the meta channel means a meta approval.
    if lower.contains("approved") && info.contains(&format!("<#{channel_meta}")) {
        return Kind::Meta;
    }
    if lower.contains("unapproved") {
        return Kind::Unapproved;
    }
    if lower.contains("unrejected") {
        return Kind::Unrejected;
    }
    if lower.contains("approved") {
        return Kind::Approved;
    }
    if lower.contains("rejected") || lower.contains("denied") {
        return Kind::Rejected;
    }
    Kind::None
}

/// `extractId`: the first `#<digits>` (with optional spaces), else the first run of two or more digits.
pub fn extract_id(text: &str) -> Option<String> {
    static HASH_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"#[{}]*([0-9]+)", js::WS)).unwrap());
    static ANY_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([0-9]{2,})").unwrap());
    HASH_ID.captures(text).or_else(|| ANY_ID.captures(text)).map(|c| c[1].to_string())
}

/// What `parseUntrack` returns: `below` for "untrack [all] below N", `id` for "untrack N". Both None is `{}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Untrack {
    pub id: Option<String>,
    pub below: Option<f64>,
}

/// `parseUntrack`: `None` when the text is not an untrack command at all.
pub fn parse_untrack(text: &str) -> Option<Untrack> {
    static CMD: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^[Uu][Nn][Tt][Rr][Aa][Cc][Kk][{}]+([^\n\r\x{{2028}}\x{{2029}}]+)$", js::WS)).unwrap());
    static BELOW: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"(?:[Aa][Ll][Ll][{ws}]+)?[Bb][Ee][Ll][Oo][Ww][{ws}]*#?[{ws}]*([0-9]+)", ws = js::WS)).unwrap());
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^#?[{}]*([0-9]+)$", js::WS)).unwrap());
    let m = CMD.captures(js::trim(text))?;
    let rest = js::trim(&m[1]);
    if let Some(b) = BELOW.captures(rest) {
        return Some(Untrack { id: None, below: Some(js::string_to_number(&b[1])) });
    }
    if let Some(i) = ID.captures(rest) {
        return Some(Untrack { id: Some(i[1].to_string()), below: None });
    }
    Some(Untrack::default())
}

/// `statusWord`: the wording used by "already tracked".
fn status_word(post: &Value) -> String {
    if truthy(post.get("resolved")) {
        let status = match post.get("status") {
            None | Some(Value::Null) => "unknown".to_string(),
            Some(v) => js_string(Some(v)),
        };
        format!("resolved ({status})")
    } else {
        "still being tracked".into()
    }
}

/// `messageLink`: the permalink for a message in the workspace.
pub fn message_link(domain: &str, channel: &str, ts: &str) -> String {
    format!("https://{domain}/archives/{channel}/p{}", ts.replacen('.', "", 1))
}

enum Scan {
    Found(Option<String>),
    Missing,
}

impl<S: Slack> Bot<S> {
    /// A bot on the system clock.
    pub fn new(cfg: Settings, msgs: Messages, store: Store, slack: S) -> Arc<Bot<S>> {
        Bot::with_clock(cfg, msgs, store, slack, Clock::System)
    }

    pub fn with_clock(cfg: Settings, msgs: Messages, store: Store, slack: S, clock: Clock) -> Arc<Bot<S>> {
        Arc::new_cyclic(|me| Bot { cfg, msgs, store, slack, clock, me: me.clone(), rejection_timers: Mutex::new(HashMap::new()) })
    }

    fn log(&self, msg: impl FnOnce() -> String) {
        if self.cfg.debug {
            println!("{}", msg());
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.slack.call(method, params).await.map_err(|e| e.to_string())
    }

    fn is_allowed(&self, user: &str) -> bool {
        !self.cfg.restrict_to_user || user == self.cfg.allowed_user_id
    }

    /// `handleInput`: runs a "/mrox"-style input (an id, or an untrack command) for a user and returns the reply.
    pub fn handle_input(&self, input: &str, user: &str) -> Result<Reply, String> {
        if let Some(un) = parse_untrack(input) {
            if let Some(n) = un.below {
                let count = self.store.untrack_below(n, user)?;
                let text = self.msgs.t("untrack_below", &[("count", count.to_string()), ("threshold", js::number_to_string(n))])?;
                return Ok(Reply::plain(text));
            }
            if let Some(id) = un.id {
                let vars = [("confessionID", id.clone())];
                let key = if self.store.untrack(&id, user)? { "untracked" } else { "untrack_missing" };
                return Ok(Reply::plain(self.msgs.t(key, &vars)?));
            }
            return Ok(Reply::plain(self.msgs.t("slash_help", &[])?));
        }
        static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^#?[{}]*([0-9]+)$", js::WS)).unwrap());
        match ID.captures(js::trim(input)) {
            Some(c) => self.track_confession(&c[1], user),
            None => Ok(Reply::plain(self.msgs.t("slash_help", &[])?)),
        }
    }

    /// `trackConfession`: tracks an id for a user, or explains that it belongs to someone else or is already
    /// tracked. A new entry also starts the status lookback.
    pub fn track_confession(&self, id: &str, user: &str) -> Result<Reply, String> {
        let vars = [("confessionID", id.to_string())];
        if let Some(existing) = self.store.find(id)? {
            // Never reveal someone else's confession status.
            if !str_eq(existing.get("userId"), user) {
                return Ok(Reply::plain(self.msgs.t("not_yours", &vars)?));
            }
            let status = status_word(&existing);
            return Ok(Reply::plain(self.msgs.t("already_tracked", &[("confessionID", id.to_string()), ("status", status)])?));
        }
        let date = iso_from_ms(self.clock.now_ms());
        self.store.push(json!({"id": id, "userId": user, "resolved": false, "date": date}))?;
        let text = self.msgs.t("now_tracking", &vars)?;
        Ok(Reply { text, lookback: Some(id.to_string()) })
    }

    /// `classify` with this bot's meta channel.
    pub fn classify(&self, info: &str) -> Kind {
        classify(info, &self.cfg.channel_meta)
    }

    /// `messageLink` with this bot's workspace domain.
    pub fn link(&self, channel: &str, ts: &str) -> String {
        message_link(&self.cfg.workspace_domain, channel, ts)
    }

    /// `dm`: DMs a user; a failure is logged, not raised.
    async fn dm(&self, user: &str, text: &str) {
        match self.call("chat.postMessage", json!({"channel": user, "text": text})).await {
            Ok(res) => self.log(|| format!("DM sent to {user}: {}", js_string(res.get("ts")))),
            Err(e) => eprintln!("Failed to DM {user}: {e}"),
        }
    }

    /// Runs `work` while `lookback` (if any) runs alongside it. The lookback is polled first, so its first Slack
    /// call is issued before the reply's, as in the TypeScript (which starts the lookback without awaiting it).
    async fn alongside<T>(&self, lookback: Option<String>, work: impl Future<Output = T>) -> T {
        match lookback {
            Some(id) => tokio::join!(self.lookback_status(&id), work).1,
            None => work.await,
        }
    }

    /// `findPostTs`: searches #confessions, then #meta, for the Owl post that starts with `<id>:`.
    /// Up to [`HISTORY_SCAN_LIMIT`] pages of 200 per channel. A failing channel is logged and skipped.
    pub async fn find_post_ts(&self, id: &str) -> Option<String> {
        for channel in [self.cfg.channel_confessions.clone(), self.cfg.channel_meta.clone()] {
            match self.scan_history(&channel, id).await {
                Ok(Scan::Found(ts)) => return ts,
                Ok(Scan::Missing) => {}
                Err(e) => eprintln!("History scan failed for {channel}: {e}"),
            }
        }
        None
    }

    async fn scan_history(&self, channel: &str, id: &str) -> Result<Scan, String> {
        let prefix = format!("{id}:");
        let mut cursor: Option<Value> = None;
        for _page in 0..HISTORY_SCAN_LIMIT {
            let mut params = json!({"channel": channel, "limit": 200});
            if let Some(c) = &cursor {
                params["cursor"] = c.clone();
            }
            let res = self.call("conversations.history", params).await?;
            for m in res.get("messages").and_then(Value::as_array).into_iter().flatten() {
                if m.get("text").and_then(Value::as_str).is_some_and(|t| t.starts_with(&prefix)) {
                    return Ok(Scan::Found(m.get("ts").and_then(Value::as_str).map(str::to_string)));
                }
            }
            let next = res.get("response_metadata").and_then(|m| m.get("next_cursor")).cloned();
            if !truthy(next.as_ref()) {
                break;
            }
            cursor = next;
        }
        Ok(Scan::Missing)
    }

    /// `resolvePostTs`: the cached `postTs`, or a search whose result is cached on the entry.
    pub async fn resolve_post_ts(&self, post: &Value) -> Result<Option<String>, String> {
        if truthy(post.get("postTs")) {
            return match post.get("postTs").and_then(Value::as_str) {
                Some(ts) => Ok(Some(ts.to_string())),
                None => Err("ts.replace is not a function".into()),
            };
        }
        let id = js_string(post.get("id"));
        let ts = self.find_post_ts(&id).await;
        if let Some(ts) = &ts {
            self.store.update(&id, &[("postTs", Some(json!(ts)))])?;
        }
        Ok(ts)
    }

    /// `getDenialReason`: the reviewer's replies in the status message's thread. The reviewer is the first human
    /// who replied (the root is skipped), so other people chatting in the thread are not quoted. Lines are joined
    /// with "\n> ".
    pub async fn get_denial_reason(&self, status_ts: Option<&str>) -> Option<String> {
        let mut params = json!({"channel": self.cfg.channel_confession_logs});
        if let Some(ts) = status_ts {
            params["ts"] = json!(ts);
        }
        match self.call("conversations.replies", params).await {
            Ok(res) => {
                let human: Vec<&Value> = res
                    .get("messages")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .skip(1)
                    .filter(|m| !truthy(m.get("bot_id")) && truthy(m.get("user")) && m.get("text").and_then(Value::as_str).is_some_and(|t| !js::trim(t).is_empty()))
                    .collect();
                let reviewer = human.first()?.get("user");
                let lines: Vec<&str> = human
                    .iter()
                    .filter(|m| m.get("user") == reviewer)
                    .filter_map(|m| m.get("text").and_then(Value::as_str).map(js::trim))
                    .collect();
                Some(lines.join("\n> "))
            }
            Err(e) => {
                eprintln!("Could not fetch denial reason: {e}");
                None
            }
        }
    }

    /// `stupidThing`: posts one random line into the logs thread for a status that should not happen.
    async fn stupid_thing(&self, channel: &str, thread_ts: Option<&str>) {
        let lines = self.msgs.stupid_lines();
        let line = lines[fastrand::usize(..lines.len())].clone();
        let mut params = json!({"channel": channel, "text": line});
        if let Some(ts) = thread_ts {
            params["thread_ts"] = json!(ts);
        }
        if let Err(e) = self.call("chat.postMessage", params).await {
            eprintln!("Failed to do the stupid thing: {e}");
        }
    }

    /// `checkStatus`: acts on one Owl status message for a tracked confession.
    pub async fn check_status(&self, information: &str, post: &Value, status_ts: Option<&str>) -> Result<(), String> {
        let id = js_string(post.get("id"));
        let logs = self.cfg.channel_confession_logs.clone();
        // Optionally require the message to reference the confession id before acting.
        if self.cfg.verify_id_in_status && !information.contains(&format!("#{id}")) {
            self.log(|| format!("Status doesn't reference confession {id}, skipping"));
            return Ok(());
        }
        let kind = self.classify(information);
        if kind == Kind::None {
            return Ok(());
        }
        // Easter egg: a status arriving for something already resolved shouldn't happen.
        if truthy(post.get("resolved")) {
            self.stupid_thing(&logs, status_ts).await;
            return Ok(());
        }
        let user = js_string(post.get("userId"));
        match kind {
            Kind::Meta => {
                let meta = self.cfg.channel_meta.clone();
                let ts = self.resolve_post_ts(post).await?;
                self.store.set_resolved(&id, "meta")?;
                let link = ts.map(|ts| format!(" <{}|View it here>", self.link(&meta, &ts))).unwrap_or_default();
                let text = self.msgs.t("meta_approved", &[("confessionID", id.clone()), ("channel", meta), ("link", link)])?;
                self.dm(&user, &text).await;
            }
            Kind::Unapproved => {
                self.store.set_resolved(&id, "unapproved")?;
                let text = self.msgs.t("unapproved", &[("confessionID", id.clone())])?;
                self.dm(&user, &text).await;
            }
            Kind::Approved => {
                let confessions = self.cfg.channel_confessions.clone();
                let ts = self.resolve_post_ts(post).await?;
                self.store.set_resolved(&id, "approved")?;
                let link = ts.map(|ts| format!(" <{}|View it here>", self.link(&confessions, &ts))).unwrap_or_default();
                let text = self.msgs.t("approved", &[("confessionID", id.clone()), ("channel", confessions), ("link", link)])?;
                self.dm(&user, &text).await;
            }
            Kind::Rejected => {
                // Wait for the reviewer's reason; send_rejection fires from a timer.
                let mut pending = Map::new();
                if let Some(ts) = status_ts {
                    pending.insert("statusTs".into(), json!(ts));
                }
                pending.insert("at".into(), json!(iso_from_ms(self.clock.now_ms())));
                self.store.update(&id, &[("pendingReject", Some(Value::Object(pending)))])?;
                self.schedule_rejection(&id, self.clock.now_ms() + self.cfg.reason_quiet_ms)?;
            }
            Kind::Unrejected => {
                // A rejection was undone before we told anyone: drop it and keep tracking.
                if truthy(post.get("pendingReject")) {
                    self.cancel_rejection(&id);
                    self.store.update(&id, &[("pendingReject", None)])?;
                }
            }
            Kind::None => {}
        }
        Ok(())
    }

    /// `scheduleRejection(id, when)`: (re)arms the rejection DM for `when`, never later than the max wait after the
    /// rejection. Does nothing when the entry has no pending rejection.
    pub fn schedule_rejection(&self, id: &str, when: f64) -> Result<(), String> {
        let Some(post) = self.store.find(id)? else { return Ok(()) };
        let Some(pending) = post.get("pendingReject").filter(|p| truthy(Some(p))) else { return Ok(()) };
        let deadline = date_ms(pending.get("at")) + self.cfg.reason_max_wait_ms;
        self.cancel_rejection(id);
        let delay = timeout_delay_ms(js_min(when, deadline) - self.clock.now_ms());
        let Some(bot) = self.me.upgrade() else { return Ok(()) };
        let owned = id.to_string();
        // The sleep's deadline is fixed here, when the timer is set (as setTimeout's is), not when the task first runs.
        let sleep = tokio::time::sleep(Duration::from_millis(delay));
        let handle = tokio::spawn(async move {
            sleep.await;
            if let Err(e) = bot.send_rejection(&owned).await {
                eprintln!("Unhandled rejection: {e}");
            }
        });
        self.rejection_timers.lock().unwrap().insert(id.to_string(), handle);
        Ok(())
    }

    /// `cancelRejection(id)`.
    pub fn cancel_rejection(&self, id: &str) {
        if let Some(timer) = self.rejection_timers.lock().unwrap().remove(id) {
            timer.abort();
        }
    }

    /// `sendRejection(id)`: when the timer fires, gets the reviewer's reason, marks the entry rejected and DMs.
    async fn send_rejection(&self, id: &str) -> Result<(), String> {
        self.rejection_timers.lock().unwrap().remove(id);
        let Some(post) = self.store.find(id)? else { return Ok(()) };
        let Some(pending) = post.get("pendingReject").filter(|p| truthy(Some(p))) else { return Ok(()) };
        if truthy(post.get("resolved")) {
            return Ok(());
        }
        let status_ts = pending.get("statusTs").filter(|v| !v.is_null()).map(|v| js_string(Some(v)));
        let reason = self.get_denial_reason(status_ts.as_deref()).await;
        self.store.update(
            id,
            &[("resolved", Some(json!(true))), ("status", Some(json!("rejected"))), ("pendingReject", None)],
        )?;
        let reason = reason.map(|r| format!("\n> {r}")).unwrap_or_default();
        // The link to the rejection thread. A missing statusTs throws in JS (`ts.replace`) after the update.
        let ts = pending.get("statusTs").and_then(Value::as_str).ok_or("ts.replace is not a function")?;
        let link = format!(" <{}|View the thread>", self.link(&self.cfg.channel_confession_logs, ts));
        let text = self.msgs.t("rejected", &[("confessionID", js_string(post.get("id"))), ("reason", reason), ("link", link)])?;
        self.dm(&js_string(post.get("userId")), &text).await;
        Ok(())
    }

    /// `onLogsThreadReply(threadTs)`: a human reply under a pending rejection pushes its DM back by the quiet period.
    pub fn on_logs_thread_reply(&self, thread_ts: &Value) -> Result<(), String> {
        if let Some(post) = self.store.find_pending(thread_ts)? {
            let when = self.clock.now_ms() + self.cfg.reason_quiet_ms;
            self.schedule_rejection(&js_string(post.get("id")), when)?;
        }
        Ok(())
    }

    /// `resumePendingRejections()`: after a restart, re-arms every unresolved pending rejection.
    pub fn resume_pending_rejections(&self) -> Result<(), String> {
        for c in self.store.list()? {
            if truthy(c.get("pendingReject")) && !truthy(c.get("resolved")) {
                let when = self.clock.now_ms() + self.cfg.reason_quiet_ms;
                self.schedule_rejection(&js_string(c.get("id")), when)?;
            }
        }
        Ok(())
    }

    /// `lookbackStatus(id)`: after a new track, applies the newest Owl status for the id from the logs history.
    /// Never raises; a failure is logged.
    pub async fn lookback_status(&self, id: &str) {
        if let Err(e) = self.lookback(id).await {
            eprintln!("Status lookback failed for {id}: {e}");
        }
    }

    /// `searchStatus(id)`: looks the id up in the logs with `search.messages` (user token). True when the search
    /// answered: a status was found and applied. Without a user token, or on any failure, it is false (and a
    /// failure is logged), so the caller falls back to the history scan.
    async fn search_status(&self, id: &str) -> bool {
        if self.cfg.user_token.is_none() {
            return false;
        }
        match self.search_status_inner(id).await {
            Ok(found) => found,
            Err(e) => {
                eprintln!("Status search failed for {id}, scanning history instead: {e}");
                false
            }
        }
    }

    async fn search_status_inner(&self, id: &str) -> Result<bool, String> {
        let query = format!("\"#{id}\" in:#{}", self.cfg.logs_channel_name);
        let params = json!({"query": query, "sort": "timestamp", "sort_dir": "desc", "count": 20});
        let res = self.slack.call_user("search.messages", params).await.map_err(|e| e.to_string())?;
        let logs = self.cfg.channel_confession_logs.as_str();
        let matches = res.get("messages").and_then(|m| m.get("matches")).and_then(Value::as_array);
        // No hit may just be search lag, so only a positive result counts.
        let Some(hit) = matches.into_iter().flatten().find(|m| {
            str_eq(m.get("channel").and_then(|c| c.get("id")), logs)
                && (str_eq(m.get("bot_id"), &self.cfg.owl_bot_id) || str_eq(m.get("user"), &self.cfg.owl_user_id))
                && m.get("text").and_then(Value::as_str).is_some_and(|t| extract_id(t).as_deref() == Some(id) && self.classify(t) != Kind::None)
        }) else {
            return Ok(false);
        };
        let text = hit.get("text").and_then(Value::as_str).unwrap_or_default().to_string();
        let ts = hit.get("ts").filter(|v| !v.is_null()).map(|v| js_string(Some(v)));
        if let Some(post) = self.store.find(id)? {
            if !truthy(post.get("resolved")) && !truthy(post.get("pendingReject")) {
                self.check_status(&text, &post, ts.as_deref()).await?;
            }
        }
        Ok(true)
    }

    async fn lookback(&self, id: &str) -> Result<(), String> {
        if self.search_status(id).await {
            return Ok(());
        }
        let logs = self.cfg.channel_confession_logs.clone();
        let mut cursor: Option<Value> = None;
        for _page in 0..HISTORY_SCAN_LIMIT {
            let mut params = json!({"channel": logs, "limit": 200});
            if let Some(c) = &cursor {
                params["cursor"] = c.clone();
            }
            let res = self.call("conversations.history", params).await?;
            for m in res.get("messages").and_then(Value::as_array).into_iter().flatten() {
                // history is newest first, so the first match is the current status
                let Some(text) = m.get("text").and_then(Value::as_str) else { continue };
                if !str_eq(m.get("bot_id"), &self.cfg.owl_bot_id) || !truthy(m.get("ts")) {
                    continue;
                }
                if extract_id(text).as_deref() != Some(id) || self.classify(text) == Kind::None {
                    continue;
                }
                if let Some(post) = self.store.find(id)? {
                    if !truthy(post.get("resolved")) && !truthy(post.get("pendingReject")) {
                        let ts = js_string(m.get("ts"));
                        self.check_status(text, &post, Some(&ts)).await?;
                    }
                }
                return Ok(());
            }
            let next = res.get("response_metadata").and_then(|m| m.get("next_cursor")).cloned();
            if !truthy(next.as_ref()) {
                break;
            }
            cursor = next;
        }
        Ok(())
    }

    /// The `app.event("message")` listener.
    pub async fn on_message(&self, event: &Value) -> Result<(), String> {
        let subtype = event.get("subtype");
        // Edits are handled by unwrapping the changed message; other subtypes are ignored, bot messages are kept.
        let raw: Value = if str_eq(subtype, "message_changed") {
            match event.get("message") {
                Some(m) if !m.is_null() => m.clone(),
                _ => event.clone(),
            }
        } else if truthy(subtype) && !str_eq(subtype, "bot_message") {
            return Ok(());
        } else {
            event.clone()
        };

        let text = match raw.get("text") {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        // message_changed nests the message, so the channel lives on the outer event.
        let channel_val = event.get("channel").filter(|v| !v.is_null()).or_else(|| raw.get("channel").filter(|v| !v.is_null()));
        let channel = channel_val.and_then(Value::as_str);
        let bot_id = raw.get("bot_id");
        let logs = self.cfg.channel_confession_logs.as_str();

        if channel == Some(logs) {
            self.log(|| format!("[msg] channel={logs} bot={} text=\"{text}\"", js_string(bot_id)));
        }

        // DMs: never accept ids here (they would sit in DM history); point to /mrox or the App Home.
        let is_dm = str_eq(raw.get("channel_type"), "im") || channel.is_some_and(|c| c.starts_with('D'));
        if is_dm {
            if truthy(bot_id) || str_eq(raw.get("subtype"), "bot_message") || !truthy(raw.get("user")) {
                return Ok(());
            }
            let user = js_string(raw.get("user"));
            let text = self.msgs.t("dm_refuse", &[])?;
            self.dm(&user, &text).await;
            return Ok(());
        }

        let Some(channel) = channel else { return Ok(()) };
        if channel != logs {
            return Ok(());
        }
        // Reviewer replies under a rejection carry the reason.
        let thread_ts = raw.get("thread_ts");
        if truthy(thread_ts) && thread_ts != raw.get("ts") && !truthy(bot_id) {
            return self.on_logs_thread_reply(thread_ts.unwrap_or(&Value::Null));
        }
        if text.is_empty() {
            return Ok(());
        }
        // Only trust status messages from Owl, not anyone who can type in the channel.
        if !str_eq(bot_id, &self.cfg.owl_bot_id) {
            self.log(|| format!("Ignoring non-Owl message in logs (bot={})", js_string(bot_id)));
            return Ok(());
        }
        let Some(id) = extract_id(&text) else { return Ok(()) };
        let Some(post) = self.store.find(&id)? else {
            self.log(|| format!("No tracked confession for id {id}"));
            return Ok(());
        };
        let status_ts = raw.get("ts").filter(|v| !v.is_null()).map(|v| js_string(Some(v)));
        self.check_status(&text, &post, status_ts.as_deref()).await
    }

    /// `homeView`: the App Home blocks for a user, with an optional notice under the input.
    pub fn home_view(&self, user: &str, notice: Option<&str>) -> Result<Value, String> {
        let tracked = self.store.tracked_by(user)?;
        let mut blocks = vec![
            json!({"type": "section", "text": {"type": "mrkdwn", "text": self.msgs.home_value("header", "*Mrox* :neocat_glasses:")}}),
            json!({
                // dispatch_action fires on Enter, so no command is needed.
                "type": "input",
                // A fresh id each publish clears the box.
                "block_id": format!("track_input_{}", self.clock.now_ms() as i64),
                "dispatch_action": true,
                "label": {"type": "plain_text", "text": self.msgs.home_value("track_label", "Track a confession")},
                "element": {
                    "type": "plain_text_input",
                    "action_id": "track_confession_input",
                    "placeholder": {"type": "plain_text", "text": self.msgs.home_value("track_placeholder", "Paste a confession ID, e.g. 40289")},
                    "dispatch_action_config": {"trigger_actions_on": ["on_enter_pressed"]}
                }
            }),
        ];
        if let Some(n) = notice {
            blocks.push(json!({"type": "context", "elements": [{"type": "mrkdwn", "text": n}]}));
        }
        if !tracked.is_empty() {
            blocks.push(json!({"type": "divider"}));
            blocks.push(json!({"type": "section", "text": {"type": "mrkdwn", "text": "*Being tracked*"}}));
            // Home views allow 100 blocks; cap the list well under that.
            for c in tracked.iter().take(20) {
                let mut button = json!({
                    "type": "section",
                    "text": {"type": "mrkdwn", "text": format!("#{}", js_string(c.get("id")))},
                    "accessory": {
                        "type": "button",
                        "text": {"type": "plain_text", "text": "Untrack"},
                        "action_id": "untrack_confession",
                    }
                });
                if let Some(id) = c.get("id") {
                    button["accessory"]["value"] = id.clone();
                }
                blocks.push(button);
            }
        }
        blocks.push(json!({"type": "divider"}));
        blocks.push(json!({"type": "context", "elements": [{"type": "mrkdwn", "text": format!("<{PRIVACY_POLICY_URL}|Privacy policy>")}]}));
        Ok(json!({"type": "home", "blocks": blocks}))
    }

    async fn publish_home(&self, user: &str, notice: Option<&str>) -> Result<(), String> {
        let view = self.home_view(user, notice)?;
        self.call("views.publish", json!({"user_id": user, "view": view})).await.map(|_| ())
    }

    /// `app.event("app_home_opened")`.
    pub async fn on_home_opened(&self, event: &Value) {
        if !truthy(event.get("user")) {
            return;
        }
        let user = js_string(event.get("user"));
        if let Err(e) = self.publish_home(&user, None).await {
            eprintln!("Failed to publish home view: {e}");
        }
    }

    async fn respond(&self, url: &str, text: &str) -> Result<(), String> {
        self.slack.respond(url, json!({"text": text})).await.map_err(|e| e.to_string())
    }

    /// `app.command("/mrox")`: the command is acked by the socket layer before this runs.
    pub async fn on_command(&self, payload: &Value) -> Result<(), String> {
        let user = payload.get("user_id").and_then(Value::as_str).unwrap_or("");
        let text = payload.get("text").and_then(Value::as_str).unwrap_or("");
        let url = payload.get("response_url").and_then(Value::as_str).unwrap_or("");
        if !self.is_allowed(user) {
            let msg = self.msgs.t("not_allowed", &[])?;
            return self.respond(url, &msg).await;
        }
        let Reply { text: reply, lookback } = self.handle_input(text, user)?;
        self.alongside(lookback, self.respond(url, &reply)).await
    }

    fn body_user(body: &Value) -> Result<&str, String> {
        body.get("user").and_then(|u| u.get("id")).and_then(Value::as_str).ok_or_else(|| "Cannot read properties of undefined (reading 'id')".into())
    }

    /// `app.action("track_confession_input")`: a value pasted and entered in the App Home box.
    pub async fn on_track_input(&self, body: &Value, action: &Value) -> Result<(), String> {
        let user = Self::body_user(body)?;
        let value = match action.get("value") {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => js::trim(s).to_string(),
            Some(_) => return Err("value.trim is not a function".into()),
        };
        if value.is_empty() {
            return Ok(());
        }
        let Reply { text, lookback } = if self.is_allowed(user) {
            self.handle_input(&value, user)?
        } else {
            Reply::plain(self.msgs.t("not_allowed", &[])?)
        };
        self.alongside(lookback, self.publish_home(user, Some(&text))).await
    }

    /// `app.action("untrack_confession")`: an Untrack button in the App Home list.
    pub async fn on_untrack(&self, body: &Value, action: &Value) -> Result<(), String> {
        let user = Self::body_user(body)?;
        let id = match action.get("value") {
            None | Some(Value::Null) => String::new(),
            Some(v) => js_string(Some(v)),
        };
        let Reply { text, lookback } = if self.is_allowed(user) {
            self.handle_input(&format!("untrack {id}"), user)?
        } else {
            Reply::plain(self.msgs.t("not_allowed", &[])?)
        };
        self.alongside(lookback, self.publish_home(user, Some(&text))).await
    }

    /// Runs one listener; errors are logged, as Bolt does for an uncaught listener error.
    pub async fn dispatch(&self, d: Dispatch) {
        let result = match d {
            Dispatch::Message(ev) => {
                if let Err(e) = self.on_message(&ev).await {
                    eprintln!("Error handling message event: {e}");
                }
                return;
            }
            Dispatch::HomeOpened(ev) => {
                self.on_home_opened(&ev).await;
                return;
            }
            Dispatch::Command(p) => self.on_command(&p).await.map_err(|e| format!("/mrox listener: {e}")),
            Dispatch::TrackInput { body, action } => self.on_track_input(&body, &action).await.map_err(|e| format!("track_confession_input listener: {e}")),
            Dispatch::Untrack { body, action } => self.on_untrack(&body, &action).await.map_err(|e| format!("untrack_confession listener: {e}")),
        };
        if let Err(e) = result {
            eprintln!("{e}");
        }
    }

    /// The startup and hourly cleanup with the 2-day retention window.
    pub fn cleanup(&self) -> Result<(), String> {
        self.store.cleanup(RETENTION_MS, self.clock.now_ms())
    }

    /// `main()` after the data-file check: cleanup, the pending rejections, then the status lookbacks.
    pub fn start_up(&self) -> Result<(), String> {
        self.cleanup()?;
        self.resume_pending_rejections()?;
        self.lookback_open_entries()
    }

    /// Catches statuses posted while the bot was down: every entry that is unresolved and has no pending rejection
    /// gets a status lookback. The lookbacks all start at once (each runs as its own task).
    pub fn lookback_open_entries(&self) -> Result<(), String> {
        for c in self.store.list()? {
            if truthy(c.get("resolved")) || truthy(c.get("pendingReject")) {
                continue;
            }
            let Some(bot) = self.me.upgrade() else { continue };
            let id = js_string(c.get("id"));
            tokio::spawn(async move { bot.lookback_status(&id).await });
        }
        Ok(())
    }
}

#[cfg(test)]
mod parity;
#[cfg(test)]
mod tests;
