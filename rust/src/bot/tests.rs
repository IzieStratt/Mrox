//! Handler tests against the fake Slack. The config is the repo's real config.yaml, so the message text under test is
//! the text the bot actually sends.
use super::*;
use crate::config::Settings;
use crate::js::{iso_from_ms, system_ms, Clock};
use crate::testkit::FakeSlack;
use std::time::Duration;
use serde_json::json;
use std::sync::Arc;
use std::fs;
use std::path::PathBuf;

const LOGS: &str = "C08R8LE6DDG";
const CONFESSIONS: &str = "CNMU9L92Q";
const META: &str = "C0188CY57PZ";
const OWL: &str = "B0C6JPG4W0G";
const ALLOWED: &str = "U00000TEST";
const DOMAIN: &str = "hackclub.enterprise.slack.com";

struct Harness {
    bot: Arc<Bot<Arc<FakeSlack>>>,
    fake: Arc<FakeSlack>,
    dir: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn repo_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("config.yaml")
}

/// A bot with an empty data file (or `data` written first) and the given settings tweaks.
fn harness(data: &str, tweak: impl FnOnce(&mut Settings)) -> Harness {
    let dir = std::env::temp_dir().join(format!("mrox-bot-{}-{}", std::process::id(), fastrand::u64(..)));
    fs::create_dir_all(&dir).unwrap();
    let data_file = dir.join("confessions.json");
    fs::write(&data_file, data).unwrap();
    let mut cfg = Settings::defaults(data_file.clone(), repo_config());
    tweak(&mut cfg);
    let msgs = Messages::load(&cfg.config_file);
    let fake = FakeSlack::new();
    let bot = Bot::with_clock(cfg, msgs, Store::new(&data_file), fake.clone(), Clock::tokio());
    Harness { bot, fake, dir }
}

fn read_data(h: &Harness) -> Value {
    serde_json::from_str(&fs::read_to_string(h.bot.store.path()).unwrap()).unwrap()
}

fn entry(id: &str, user: &str, resolved: bool) -> String {
    format!(r#"{{"id":"{id}","userId":"{user}","resolved":{resolved},"date":"{}"}}"#, iso_from_ms(system_ms()))
}

fn data(entries: &[String]) -> String {
    format!("[{}]", entries.join(","))
}

fn command(text: &str, user: &str) -> Value {
    json!({"command": "/mrox", "text": text, "user_id": user, "response_url": "https://hooks.example/resp"})
}

fn message(fields: Value) -> Value {
    let mut ev = json!({"type": "message", "ts": "1700000500.000200"});
    for (k, v) in fields.as_object().unwrap() {
        ev[k] = v.clone();
    }
    ev
}

fn owl_status(text: &str) -> Value {
    message(json!({"channel": LOGS, "bot_id": OWL, "text": text}))
}

/// Moves the paused clock forward and lets the timers and their tasks run.
async fn advance(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

fn published_blocks(fake: &FakeSlack) -> Vec<Value> {
    fake.calls_to("views.publish").last().unwrap()["view"]["blocks"].as_array().unwrap().clone()
}

// ---------------------------------------------------------------------------------------------------------------
// /mrox

#[tokio::test(start_paused = true)]
async fn slash_command_tracks_and_reports_status() {
    let h = harness("[]", |_| {});
    h.bot.dispatch(Dispatch::Command(command("40101", ALLOWED))).await;
    let r = h.fake.responded();
    assert_eq!(r[0].0, "https://hooks.example/resp");
    assert_eq!(r[0].1, json!({"text": "Now tracking confession #40101. I'll DM you when its status changes. :neocat:"}));
    let stored = read_data(&h);
    assert_eq!(stored[0]["id"], "40101");
    assert_eq!(stored[0]["userId"], ALLOWED);
    assert_eq!(stored[0]["resolved"], false);
    assert!(stored[0]["date"].as_str().unwrap().ends_with('Z'));

    // tracking again: the status word
    h.bot.dispatch(Dispatch::Command(command("#40101", ALLOWED))).await;
    assert_eq!(h.fake.responded()[1].1, json!({"text": "Confession #40101 is still being tracked."}));
}

#[tokio::test(start_paused = true)]
async fn resolved_status_is_reported_with_its_kind() {
    let h = harness(&data(&[r#"{"id":"7","userId":"U1","resolved":true,"date":"2026-01-01T00:00:00.000Z","status":"meta"}"#.into()]), |c| c.restrict_to_user = false);
    h.bot.dispatch(Dispatch::Command(command("7", "U2"))).await;
    assert_eq!(h.fake.responded()[0].1["text"], "Confession #7 is already being tracked by someone else.");
    h.bot.dispatch(Dispatch::Command(command("7", "U1"))).await;
    assert_eq!(h.fake.responded()[1].1["text"], "Confession #7 is resolved (meta).");
}

#[tokio::test(start_paused = true)]
async fn other_users_confession_is_not_revealed() {
    let h = harness(&data(&[entry("8", "U1", false)]), |c| c.restrict_to_user = false);
    h.bot.dispatch(Dispatch::Command(command("8", "U2"))).await;
    assert_eq!(h.fake.responded()[0].1["text"], "Confession #8 is already being tracked by someone else.");
    assert_eq!(read_data(&h).as_array().unwrap().len(), 1, "nothing is added");
}

#[tokio::test(start_paused = true)]
async fn restricted_user_gets_not_allowed_and_nothing_is_written() {
    let h = harness("[]", |_| {});
    h.bot.dispatch(Dispatch::Command(command("40101", "U_OTHER"))).await;
    assert_eq!(h.fake.responded()[0].1["text"], json!(h.bot.msgs.t("not_allowed", &[]).unwrap()));
    assert_eq!(read_data(&h), json!([]));
}

#[tokio::test(start_paused = true)]
async fn help_text_for_empty_and_bad_input() {
    let h = harness("[]", |_| {});
    let help = "to track a confession use `/mrox {id}` (or paste the id into the box on my app home) and ill dm u when its approved, use `/mrox untrack {id}` to untrack somthing";
    for text in ["", "abc", "untrack", "untrack nope", "12 34"] {
        h.bot.dispatch(Dispatch::Command(command(text, ALLOWED))).await;
    }
    for r in h.fake.responded() {
        assert_eq!(r.1["text"], help);
    }
}

#[tokio::test(start_paused = true)]
async fn untrack_commands() {
    let h = harness(&data(&[entry("40101", ALLOWED, false), entry("40103", ALLOWED, false), entry("50000", ALLOWED, false)]), |_| {});
    h.bot.dispatch(Dispatch::Command(command("untrack 40101", ALLOWED))).await;
    h.bot.dispatch(Dispatch::Command(command("untrack #40101", ALLOWED))).await;
    h.bot.dispatch(Dispatch::Command(command("UNTRACK below 50001", ALLOWED))).await;
    let texts: Vec<Value> = h.fake.responded().into_iter().map(|r| r.1["text"].clone()).collect();
    assert_eq!(
        texts,
        [
            json!("Stopped tracking confession #40101."),
            json!("Confession #40101 isn't being tracked."),
            json!("Stopped tracking 2 confession(s) below #50001."),
        ]
    );
    // "untrack below 50001" also takes 50000 (it is below 50001), so only the other user's entries could remain
    assert_eq!(read_data(&h), json!([]));
}

#[tokio::test(start_paused = true)]
async fn untrack_below_all_form_and_threshold_text() {
    let h = harness(&data(&[entry("10", ALLOWED, false), entry("20", "U_OTHER", false)]), |c| c.restrict_to_user = false);
    // The JS regex is unanchored, so "below 0" inside "below 0x" still matches: threshold 0, nothing removed.
    h.bot.dispatch(Dispatch::Command(command("untrack all below 0x", ALLOWED))).await;
    assert_eq!(h.fake.responded()[0].1["text"], "Stopped tracking 0 confession(s) below #0.");
    h.bot.dispatch(Dispatch::Command(command("untrack all below 100", ALLOWED))).await;
    assert_eq!(h.fake.responded()[1].1["text"], "Stopped tracking 1 confession(s) below #100.");
    assert_eq!(read_data(&h).as_array().unwrap().len(), 1, "the other user's entry stays");
}

// ---------------------------------------------------------------------------------------------------------------
// App Home

#[tokio::test(start_paused = true)]
async fn app_home_publishes_the_input_and_only_unresolved_entries() {
    let entries: Vec<String> = (0..25).map(|i| entry(&format!("{}", 40000 + i), ALLOWED, false)).chain([entry("1", ALLOWED, true), entry("2", "U_OTHER", false)]).collect();
    let h = harness(&data(&entries), |_| {});
    h.bot.dispatch(Dispatch::HomeOpened(json!({"type": "app_home_opened", "user": ALLOWED}))).await;
    let call = h.fake.calls_to("views.publish");
    assert_eq!(call.len(), 1);
    assert_eq!(call[0]["user_id"], ALLOWED);
    let view = &call[0]["view"];
    assert_eq!(view["type"], "home");
    let blocks = view["blocks"].as_array().unwrap();
    assert_eq!(blocks[0], json!({"type": "section", "text": {"type": "mrkdwn", "text": "*Mrox* :neocat_glasses:\nPaste your confession ID below and press Enter, and I'll DM you when its status changes."}}));
    let input = &blocks[1];
    assert_eq!(input["type"], "input");
    assert!(input["block_id"].as_str().unwrap().starts_with("track_input_"));
    assert_eq!(input["dispatch_action"], true);
    assert_eq!(input["label"], json!({"type": "plain_text", "text": "Track a confession"}));
    assert_eq!(
        input["element"],
        json!({
            "type": "plain_text_input",
            "action_id": "track_confession_input",
            "placeholder": {"type": "plain_text", "text": "Paste a confession ID, e.g. 40289"},
            "dispatch_action_config": {"trigger_actions_on": ["on_enter_pressed"]}
        })
    );
    // no notice block; divider, heading, 20 entries, divider, privacy context
    assert_eq!(blocks[2], json!({"type": "divider"}));
    assert_eq!(blocks[3], json!({"type": "section", "text": {"type": "mrkdwn", "text": "*Being tracked*"}}));
    assert_eq!(blocks.len(), 4 + 20 + 2);
    assert_eq!(blocks[4]["text"]["text"], "#40000");
    assert_eq!(
        blocks[4]["accessory"],
        json!({"type": "button", "text": {"type": "plain_text", "text": "Untrack"}, "action_id": "untrack_confession", "value": "40000"})
    );
    assert_eq!(blocks[23]["text"]["text"], "#40019");
    assert_eq!(blocks[24], json!({"type": "divider"}));
    assert_eq!(
        blocks[25]["elements"][0]["text"],
        "<https://hackclub.enterprise.slack.com/docs/T0266FRGM/F0C9ZFGE6U9|Privacy policy>"
    );
}

#[tokio::test(start_paused = true)]
async fn app_home_without_a_user_does_nothing_and_failures_are_logged_only() {
    let h = harness("[]", |_| {});
    h.bot.dispatch(Dispatch::HomeOpened(json!({"type": "app_home_opened"}))).await;
    assert!(h.fake.calls().is_empty());
    h.fake.fail("views.publish", "invalid_arguments");
    h.bot.dispatch(Dispatch::HomeOpened(json!({"type": "app_home_opened", "user": ALLOWED}))).await;
    assert_eq!(h.fake.calls_to("views.publish").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn pasting_an_id_into_the_box_publishes_a_notice() {
    let h = harness("[]", |_| {});
    let body = json!({"user": {"id": ALLOWED}});
    h.bot.dispatch(Dispatch::TrackInput { body: body.clone(), action: json!({"action_id": "track_confession_input", "value": "  40101 \n"}) }).await;
    let blocks = published_blocks(&h.fake);
    assert_eq!(blocks[2], json!({"type": "context", "elements": [{"type": "mrkdwn", "text": "Now tracking confession #40101. I'll DM you when its status changes. :neocat:"}]}));
    assert_eq!(blocks[3], json!({"type": "divider"}));
    assert_eq!(blocks[4]["text"]["text"], "*Being tracked*");
    assert_eq!(blocks[5]["text"]["text"], "#40101");

    // an empty value publishes nothing
    let before = h.fake.calls_to("views.publish").len();
    h.bot.dispatch(Dispatch::TrackInput { body: body.clone(), action: json!({"value": "   "}) }).await;
    assert_eq!(h.fake.calls_to("views.publish").len(), before);

    // a non-allowed user gets the refusal notice
    h.bot.dispatch(Dispatch::TrackInput { body: json!({"user": {"id": "U_OTHER"}}), action: json!({"value": "1"}) }).await;
    assert_eq!(published_blocks(&h.fake)[2]["elements"][0]["text"], json!(h.bot.msgs.t("not_allowed", &[]).unwrap()));
}

#[tokio::test(start_paused = true)]
async fn untrack_button_removes_and_republishes() {
    let h = harness(&data(&[entry("123", ALLOWED, false)]), |_| {});
    h.bot.dispatch(Dispatch::Untrack { body: json!({"user": {"id": ALLOWED}}), action: json!({"action_id": "untrack_confession", "value": "123"}) }).await;
    let blocks = published_blocks(&h.fake);
    assert_eq!(blocks[2]["elements"][0]["text"], "Stopped tracking confession #123.");
    assert_eq!(read_data(&h), json!([]));
    // a button with no value sends "untrack " which is the help text
    h.bot.dispatch(Dispatch::Untrack { body: json!({"user": {"id": ALLOWED}}), action: json!({"action_id": "untrack_confession"}) }).await;
    assert!(published_blocks(&h.fake)[2]["elements"][0]["text"].as_str().unwrap().starts_with("to track a confession"));
}

// ---------------------------------------------------------------------------------------------------------------
// Messages

#[tokio::test(start_paused = true)]
async fn dm_to_the_bot_gets_the_refusal_and_bot_dms_are_ignored() {
    let h = harness("[]", |_| {});
    h.bot.dispatch(Dispatch::Message(json!({"type": "message", "channel": "D0123", "channel_type": "im", "user": "U5", "text": "40101"}))).await;
    let posts = h.fake.calls_to("chat.postMessage");
    assert_eq!(posts, vec![json!({"channel": "U5", "text": "sorry! for your security I can't accept ids via dms, please use /mrox or the app home :D"})]);
    h.bot.dispatch(Dispatch::Message(json!({"type": "message", "channel": "D0123", "bot_id": "B1", "text": "hi"}))).await;
    h.bot.dispatch(Dispatch::Message(json!({"type": "message", "channel": "D0123", "subtype": "bot_message", "text": "hi"}))).await;
    assert_eq!(h.fake.calls_to("chat.postMessage").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn non_bot_messages_in_the_logs_channel_are_not_trusted() {
    let h = harness(&data(&[entry("40101", "U1", false)]), |_| {});
    h.bot.dispatch(Dispatch::Message(message(json!({"channel": LOGS, "user": "U_HUMAN", "text": "40101: approved to <#C0188CY57PZ>"})))).await;
    h.bot.dispatch(Dispatch::Message(message(json!({"channel": LOGS, "bot_id": "B_OTHER", "text": "40101 approved"})))).await;
    assert!(h.fake.calls().is_empty());
    assert_eq!(read_data(&h)[0]["resolved"], false);
}

#[tokio::test(start_paused = true)]
async fn approved_status_dms_with_the_confessions_link() {
    let h = harness(&data(&[entry("40101", "U1", false)]), |_| {});
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [{"text": "40104: other", "ts": "1.1"}, {"text": "40101: hello world", "ts": "1700000000.000100"}]}));
    h.bot.dispatch(Dispatch::Message(owl_status("Confession #40101 was approved to <#CNMU9L92Q>"))).await;
    let hist = h.fake.calls_to("conversations.history");
    assert_eq!(hist, vec![json!({"channel": CONFESSIONS, "limit": 200})]);
    let posts = h.fake.calls_to("chat.postMessage");
    assert_eq!(
        posts,
        vec![json!({
            "channel": "U1",
            "text": format!(":yay: Your confession #40101 got approved for <#{CONFESSIONS}>! <https://{DOMAIN}/archives/{CONFESSIONS}/p1700000000000100|View it here>")
        })]
    );
    let stored = read_data(&h);
    assert_eq!(stored[0]["resolved"], true);
    assert_eq!(stored[0]["status"], "approved");
    assert_eq!(stored[0]["postTs"], "1700000000.000100");
    // key order: Object.assign appends postTs (resolvePostTs runs before setResolved), then status
    let text = fs::read_to_string(h.bot.store.path()).unwrap();
    assert!(text.find("\"postTs\"").unwrap() < text.find("\"status\"").unwrap());
}

#[tokio::test(start_paused = true)]
async fn meta_approval_links_the_meta_channel_and_searches_confessions_then_meta() {
    let h = harness(&data(&[entry("40105", "U1", false)]), |_| {});
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [], "response_metadata": {"next_cursor": ""}}));
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [{"text": "40105: meta post", "ts": "1700.5"}]}));
    h.bot.dispatch(Dispatch::Message(owl_status(&format!("40105 approved to <#{META}>")))).await;
    let hist = h.fake.calls_to("conversations.history");
    assert_eq!(hist[0]["channel"], CONFESSIONS);
    assert_eq!(hist[1]["channel"], META);
    assert_eq!(
        h.fake.posted_texts(),
        [format!(":meta: Your confession #40105 got approved for <#{META}>! <https://{DOMAIN}/archives/{META}/p17005|View it here>")]
    );
    assert_eq!(read_data(&h)[0]["status"], "meta");
}

#[tokio::test(start_paused = true)]
async fn unapproved_nukes_and_does_not_search() {
    let h = harness(&data(&[entry("40106", "U1", false)]), |_| {});
    h.bot.dispatch(Dispatch::Message(owl_status("40106 unapproved"))).await;
    assert!(h.fake.calls_to("conversations.history").is_empty());
    assert_eq!(h.fake.posted_texts(), [":uuh: Your confession #40106 was unapproved and the thread has been nuked."]);
    assert_eq!(read_data(&h)[0]["status"], "unapproved");
}

#[tokio::test(start_paused = true)]
async fn rejection_quotes_the_first_human_reply_in_the_thread() {
    let h = harness(&data(&[entry("40107", "U1", false)]), |_| {});
    h.fake.reply(
        "conversations.replies",
        json!({"ok": true, "messages": [
            {"text": "40107 denied", "ts": "1700000500.000200"},
            {"text": "bot note", "bot_id": "B9"},
            {"text": "   ", "user": "U_H"},
            {"text": "  too spicy  ", "user": "U_H"}
        ]}),
    );
    h.bot.dispatch(Dispatch::Message(owl_status("40107 denied"))).await;
    assert!(h.fake.posted_texts().is_empty(), "the DM waits for the reviewer");
    advance(90_000).await;
    assert_eq!(
        h.fake.posted_texts(),
        [":neocat_sad: Your confession #40107 was denied. Reason (will be blank if none was provided): \n> too spicy <https://hackclub.enterprise.slack.com/archives/C08R8LE6DDG/p1700000500000200|View the thread>"]
    );
    assert_eq!(h.fake.calls_to("conversations.replies")[0], json!({"channel": LOGS, "ts": "1700000500.000200"}));
    assert_eq!(read_data(&h)[0]["status"], "rejected");
}

#[tokio::test(start_paused = true)]
async fn rejection_without_a_reason_leaves_the_placeholder_empty() {
    let h = harness(&data(&[entry("40108", "U1", false)]), |_| {});
    h.bot.dispatch(Dispatch::Message(owl_status("40108 rejected"))).await;
    advance(90_000).await;
    assert_eq!(h.fake.posted_texts(), [":neocat_sad: Your confession #40108 was denied. Reason (will be blank if none was provided):  <https://hackclub.enterprise.slack.com/archives/C08R8LE6DDG/p1700000500000200|View the thread>"]);
}

#[tokio::test(start_paused = true)]
async fn a_status_for_a_resolved_confession_gets_a_stupid_line_in_its_thread() {
    let h = harness(&data(&[entry("40109", "U1", true)]), |_| {});
    h.bot.dispatch(Dispatch::Message(owl_status("40109 approved to <#CNMU9L92Q>"))).await;
    let posts = h.fake.calls_to("chat.postMessage");
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0]["channel"], LOGS);
    assert_eq!(posts[0]["thread_ts"], "1700000500.000200");
    let line = posts[0]["text"].as_str().unwrap();
    let lines = ["that confession doesn't exist... yet :eyes:", "whoa there, that's not supposed to happen. neat.", "you've broken reality. congrats i guess? :neocat_confused:", "this confession is in a superposition of approved and denied :hyperbongocat:", "error 404: common sense not found"];
    assert!(lines.contains(&line), "{line}");
}

#[tokio::test(start_paused = true)]
async fn messages_without_a_known_id_or_kind_do_nothing() {
    let h = harness(&data(&[entry("40110", "U1", false)]), |_| {});
    h.bot.dispatch(Dispatch::Message(owl_status("hello there"))).await;
    h.bot.dispatch(Dispatch::Message(owl_status("40111 approved to <#CNMU9L92Q>"))).await;
    h.bot.dispatch(Dispatch::Message(owl_status(""))).await;
    assert!(h.fake.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn message_changed_edits_are_read_from_the_inner_message() {
    let h = harness(&data(&[entry("40112", "U1", false)]), |_| {});
    let edit = json!({
        "type": "message", "subtype": "message_changed", "channel": LOGS, "ts": "1700.9",
        "message": {"text": "40112 unapproved", "bot_id": OWL, "ts": "1700.1"}
    });
    h.bot.dispatch(Dispatch::Message(edit)).await;
    assert_eq!(h.fake.posted_texts(), [":uuh: Your confession #40112 was unapproved and the thread has been nuked."]);
}

#[tokio::test(start_paused = true)]
async fn verify_id_in_status_skips_messages_without_the_id() {
    let h = harness(&data(&[entry("40113", "U1", false)]), |c| c.verify_id_in_status = true);
    h.bot.dispatch(Dispatch::Message(owl_status("40113 unapproved"))).await;
    assert!(h.fake.calls().is_empty(), "no #40113 in the text");
    h.bot.dispatch(Dispatch::Message(owl_status("#40113 unapproved"))).await;
    assert_eq!(h.fake.posted_texts().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn history_is_scanned_for_at_most_five_pages_per_channel_then_the_link_is_dropped() {
    let h = harness(&data(&[entry("40114", "U1", false)]), |_| {});
    for _ in 0..12 {
        h.fake.reply("conversations.history", json!({"ok": true, "messages": [], "response_metadata": {"next_cursor": "more"}}));
    }
    h.bot.dispatch(Dispatch::Message(owl_status("40114 approved to <#CNMU9L92Q>"))).await;
    let hist = h.fake.calls_to("conversations.history");
    assert_eq!(hist.len(), 10, "5 pages in #confessions, then 5 in #meta");
    assert_eq!(hist[1]["cursor"], "more");
    assert_eq!(hist[5]["channel"], META);
    assert_eq!(h.fake.posted_texts(), [format!(":yay: Your confession #40114 got approved for <#{CONFESSIONS}>!")]);
}

#[tokio::test(start_paused = true)]
async fn a_failing_history_channel_is_skipped_and_the_other_is_still_searched() {
    let h = harness(&data(&[entry("40115", "U1", false)]), |_| {});
    h.fake.fail("conversations.history", "channel_not_found");
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [{"text": "40115: meta", "ts": "1.2"}]}));
    h.bot.dispatch(Dispatch::Message(owl_status("40115 approved to <#CNMU9L92Q>"))).await;
    assert_eq!(h.fake.calls_to("conversations.history").len(), 2);
    assert!(h.fake.posted_texts()[0].contains("https://hackclub.enterprise.slack.com/archives/CNMU9L92Q/p12"));
}

#[tokio::test(start_paused = true)]
async fn a_cached_post_ts_is_used_without_searching() {
    let h = harness(&data(&[r#"{"id":"40116","userId":"U1","resolved":false,"date":"2026-01-01T00:00:00.000Z","postTs":"1700.25"}"#.into()]), |_| {});
    h.bot.dispatch(Dispatch::Message(owl_status("40116 approved to <#CNMU9L92Q>"))).await;
    assert!(h.fake.calls_to("conversations.history").is_empty());
    assert_eq!(h.fake.posted_texts(), [format!(":yay: Your confession #40116 got approved for <#{CONFESSIONS}>! <https://{DOMAIN}/archives/{CONFESSIONS}/p170025|View it here>")]);
}

#[tokio::test(start_paused = true)]
async fn a_failing_dm_is_logged_and_the_status_is_still_saved() {
    let h = harness(&data(&[entry("40117", "U1", false)]), |_| {});
    h.fake.fail("chat.postMessage", "user_not_found");
    h.bot.dispatch(Dispatch::Message(owl_status("40117 unapproved"))).await;
    assert_eq!(read_data(&h)[0]["status"], "unapproved");
}

// ---------------------------------------------------------------------------------------------------------------
// Parsing

#[test]
fn extract_id_prefers_a_hash_then_two_digits() {
    assert_eq!(extract_id("Confession # 40101 was approved"), Some("40101".into()));
    assert_eq!(extract_id("status 40101: approved"), Some("40101".into()));
    assert_eq!(extract_id("a 7 b 12 c"), Some("12".into()));
    assert_eq!(extract_id("#7 then 40101"), Some("7".into()));
    assert_eq!(extract_id("no digits"), None);
    assert_eq!(extract_id("one 5 digit"), None);
}

#[test]
fn classify_orders_meta_then_unapproved_then_approved_then_denied() {
    assert_eq!(classify("40 approved to <#C0188CY57PZ>", META), Kind::Meta);
    assert_eq!(classify("40 unapproved", META), Kind::Unapproved);
    assert_eq!(classify("40 unapproved to <#C0188CY57PZ>", META), Kind::Meta, "meta wins over unapproved");
    assert_eq!(classify("40 APPROVED to <#CNMU9L92Q>", META), Kind::Approved);
    assert_eq!(classify("40 Rejected", META), Kind::Rejected);
    assert_eq!(classify("40 DENIED", META), Kind::Rejected);
    assert_eq!(classify("40 pending", META), Kind::None);
    assert_eq!(classify("40 approved to <#C0188CY57PZ>", "C_OTHER"), Kind::Approved, "meta is judged by the configured channel");
}

#[test]
fn parse_untrack_forms() {
    assert_eq!(parse_untrack("untrack 1"), Some(Untrack { id: Some("1".into()), below: None }));
    assert_eq!(parse_untrack("  Untrack   #1  "), Some(Untrack { id: Some("1".into()), below: None }));
    assert_eq!(parse_untrack("untrack below # 3"), Some(Untrack { id: None, below: Some(3.0) }));
    assert_eq!(parse_untrack("untrack all below 99"), Some(Untrack { id: None, below: Some(99.0) }));
    assert_eq!(parse_untrack("untrack banana"), Some(Untrack::default()));
    assert_eq!(parse_untrack("untrack"), None);
    assert_eq!(parse_untrack("untrack 1\n2"), None, "a line break ends the command, as '.' does in JS");
}

#[test]
fn message_links_drop_only_the_first_dot() {
    assert_eq!(message_link(DOMAIN, META, "1700.000100.5"), format!("https://{DOMAIN}/archives/{META}/p1700000100.5"));
}

#[tokio::test(start_paused = true)]
async fn cleanup_expires_only_old_resolved_entries() {
    let old = r#"{"id":"1","userId":"U","resolved":true,"date":"2020-01-01T00:00:00.000Z"}"#;
    let h = harness(&data(&[old.into(), entry("2", "U", true)]), |_| {});
    h.bot.cleanup().unwrap();
    let ids: Vec<Value> = read_data(&h).as_array().unwrap().iter().map(|c| c["id"].clone()).collect();
    assert_eq!(ids, [json!("2")]);
}

// ---------------------------------------------------------------------------------------------------------------
// Denial reasons, the quiet period, unrejected, resume, and the status lookback

fn reply_msg(thread_ts: &str, ts: &str, user: &str, text: &str) -> Value {
    message(json!({"channel": LOGS, "user": user, "thread_ts": thread_ts, "ts": ts, "text": text}))
}

const DENIED_TS: &str = "1700000500.000200";
const EXPECTED_REASON_LINES: &str = ":neocat_sad: Your confession #40118 was denied. Reason (will be blank if none was provided): \n> too spicy\n> and cringe <https://hackclub.enterprise.slack.com/archives/C08R8LE6DDG/p1700000500000200|View the thread>";

#[tokio::test(start_paused = true)]
async fn reviewer_reason_waits_for_the_quiet_period_after_the_last_reviewer_reply() {
    let h = harness(&data(&[entry("40118", "U1", false)]), |_| {});
    h.fake.reply(
        "conversations.replies",
        json!({"ok": true, "messages": [
            {"text": "40118 denied", "ts": DENIED_TS},
            {"text": "too spicy", "user": "U_REV", "ts": "1700000520.000100"},
            {"text": "and cringe", "user": "U_REV", "ts": "1700000530.000100"},
            {"text": "lol", "user": "U_OTHER", "ts": "1700000535.000100"}
        ]}),
    );
    h.bot.dispatch(Dispatch::Message(owl_status("40118 denied"))).await;
    advance(89_000).await;
    h.bot.dispatch(Dispatch::Message(reply_msg(DENIED_TS, "1700000520.000100", "U_REV", "too spicy"))).await;
    advance(89_999).await;
    assert!(h.fake.posted_texts().is_empty(), "the reply pushed the DM back by 90 s");
    advance(1).await;
    assert_eq!(h.fake.posted_texts(), [EXPECTED_REASON_LINES], "only the reviewer's lines, in order");
    // the reply that rescheduled it did not write the reason twice
    assert_eq!(h.fake.calls_to("conversations.replies").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn repeated_replies_cannot_hold_a_rejection_past_ten_minutes() {
    let h = harness(&data(&[entry("40118", "U1", false)]), |_| {});
    h.fake.reply(
        "conversations.replies",
        json!({"ok": true, "messages": [{"text": "40118 denied"}, {"text": "too spicy", "user": "U_REV"}, {"text": "and cringe", "user": "U_REV"}]}),
    );
    h.bot.dispatch(Dispatch::Message(owl_status("40118 denied"))).await;
    // a reviewer reply every 80 s keeps pushing the DM back, but the 10-minute cap wins
    for _ in 0..7 {
        advance(80_000).await;
        h.bot.dispatch(Dispatch::Message(reply_msg(DENIED_TS, "1700000600.000100", "U_REV", "still"))).await;
    }
    // 7 * 80 s = 560 s; the reply at 560 s would give 650 s, so the cap sets 600 s
    assert!(h.fake.posted_texts().is_empty());
    advance(39_999).await;
    assert!(h.fake.posted_texts().is_empty(), "not before the cap");
    advance(1).await;
    assert_eq!(h.fake.posted_texts().len(), 1, "at 600 s after the rejection");
}

#[tokio::test(start_paused = true)]
async fn unrejected_cancels_a_pending_rejection_and_clears_the_marker() {
    let h = harness(&data(&[entry("40119", "U1", false)]), |_| {});
    h.bot.dispatch(Dispatch::Message(owl_status("40119 denied"))).await;
    assert!(fs::read_to_string(h.bot.store.path()).unwrap().contains("\"pendingReject\""));
    h.bot.dispatch(Dispatch::Message(owl_status("40119 unrejected"))).await;
    let text = fs::read_to_string(h.bot.store.path()).unwrap();
    assert!(!text.contains("pendingReject"), "the key is removed, as JSON.stringify drops undefined");
    advance(700_000).await;
    assert!(h.fake.posted_texts().is_empty());
    assert_eq!(read_data(&h)[0]["resolved"], false);
}

#[tokio::test(start_paused = true)]
async fn unrejected_without_a_pending_rejection_changes_nothing() {
    let h = harness(&data(&[entry("40120", "U1", false)]), |_| {});
    let before = fs::read_to_string(h.bot.store.path()).unwrap();
    h.bot.dispatch(Dispatch::Message(owl_status("40120 unrejected"))).await;
    assert_eq!(fs::read_to_string(h.bot.store.path()).unwrap(), before);
    assert!(h.fake.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_pending_rejection_survives_a_restart() {
    let at = iso_from_ms(system_ms());
    let entry_text = format!(r#"{{"id":"40121","userId":"U1","resolved":false,"date":"{at}","pendingReject":{{"statusTs":"{DENIED_TS}","at":"{at}"}}}}"#);
    let h = harness(&data(&[entry_text]), |_| {});
    h.fake.reply("conversations.replies", json!({"ok": true, "messages": [{"text": "40121 denied"}, {"text": "later", "user": "U_REV"}]}));
    h.bot.resume_pending_rejections().unwrap();
    advance(89_999).await;
    assert!(h.fake.posted_texts().is_empty());
    advance(1).await;
    assert_eq!(h.fake.posted_texts(), [":neocat_sad: Your confession #40121 was denied. Reason (will be blank if none was provided): \n> later <https://hackclub.enterprise.slack.com/archives/C08R8LE6DDG/p1700000500000200|View the thread>"]);
}

#[tokio::test(start_paused = true)]
async fn a_resolved_entry_with_a_stale_marker_is_not_resumed() {
    let at = iso_from_ms(system_ms());
    let entry_text = format!(r#"{{"id":"40122","userId":"U1","resolved":true,"date":"{at}","status":"approved","pendingReject":{{"statusTs":"{DENIED_TS}","at":"{at}"}}}}"#);
    let h = harness(&data(&[entry_text]), |_| {});
    h.bot.resume_pending_rejections().unwrap();
    advance(700_000).await;
    assert!(h.fake.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_reply_from_a_bot_or_the_root_itself_is_not_a_reviewer_reply() {
    let h = harness(&data(&[entry("40120", "U1", false)]), |_| {});
    h.fake.reply("conversations.replies", json!({"ok": true, "messages": [{"text": "40120 denied"}]}));
    h.bot.dispatch(Dispatch::Message(owl_status("40120 denied"))).await;
    // the root: thread_ts equals ts, so it is an ordinary (non-Owl) message and is ignored
    h.bot.dispatch(Dispatch::Message(message(json!({"channel": LOGS, "user": "U_REV", "thread_ts": DENIED_TS, "ts": DENIED_TS, "text": "hi"})))).await;
    // a bot reply in the thread is not a reviewer reply either
    h.bot.dispatch(Dispatch::Message(message(json!({"channel": LOGS, "bot_id": "B_BOT", "thread_ts": DENIED_TS, "ts": "1700000510.000100", "text": "hi"})))).await;
    advance(90_000).await;
    assert_eq!(h.fake.posted_texts().len(), 1, "the DM went out on the original schedule");
}

#[tokio::test(start_paused = true)]
async fn an_approval_that_lands_first_keeps_the_timer_from_sending_a_denial() {
    let h = harness(&data(&[entry("40122", "U1", false)]), |_| {});
    h.bot.dispatch(Dispatch::Message(owl_status("40122 denied"))).await;
    h.bot.dispatch(Dispatch::Message(owl_status("40122 approved to <#CNMU9L92Q>"))).await;
    let approved = h.fake.posted_texts();
    assert_eq!(approved.len(), 1);
    assert!(approved[0].starts_with(":yay: "));
    advance(90_000).await;
    assert_eq!(h.fake.posted_texts(), approved, "the timer sees a resolved entry and does nothing");
}

#[tokio::test(start_paused = true)]
async fn a_failed_rejection_dm_still_resolves_the_entry() {
    let h = harness(&data(&[entry("40123", "U1", false)]), |_| {});
    h.fake.fail("chat.postMessage", "user_not_found");
    h.bot.dispatch(Dispatch::Message(owl_status("40123 denied"))).await;
    advance(90_000).await;
    let stored = read_data(&h);
    assert_eq!(stored[0]["status"], "rejected");
    assert_eq!(stored[0]["resolved"], true);
    assert!(stored[0].get("pendingReject").is_none());
}

#[tokio::test(start_paused = true)]
async fn pending_marker_key_order_matches_object_assign_and_stringify() {
    let h = harness(&data(&[entry("40124", "U1", false)]), |_| {});
    h.fake.reply("conversations.replies", json!({"ok": true, "messages": []}));
    h.bot.dispatch(Dispatch::Message(owl_status("40124 denied"))).await;
    let pending = fs::read_to_string(h.bot.store.path()).unwrap();
    let order = |text: &str, keys: &[&str]| -> Vec<usize> { keys.iter().map(|k| text.find(k).unwrap_or_else(|| panic!("{k} missing in {text}"))).collect() };
    let at = order(&pending, &["\"id\"", "\"userId\"", "\"resolved\"", "\"date\"", "\"pendingReject\"", "\"statusTs\"", "\"at\""]);
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{pending}");
    advance(90_000).await;
    let done = fs::read_to_string(h.bot.store.path()).unwrap();
    assert!(!done.contains("pendingReject"));
    let order_done = order(&done, &["\"id\"", "\"userId\"", "\"resolved\"", "\"date\"", "\"status\""]);
    assert!(order_done.windows(2).all(|w| w[0] < w[1]), "{done}");
}

#[tokio::test(start_paused = true)]
async fn lookback_applies_a_status_posted_before_tracking_started() {
    let h = harness("[]", |_| {});
    // first the logs history (the lookback), then #confessions (findPostTs for the link)
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [
        {"bot_id": OWL, "text": "40125 approved to <#CNMU9L92Q>", "ts": "1700000900.000100"}
    ]}));
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [{"text": "40125: hi", "ts": "1700000000.000300"}]}));
    h.bot.dispatch(Dispatch::Command(command("40125", ALLOWED))).await;
    let hist = h.fake.calls_to("conversations.history");
    assert_eq!(hist[0]["channel"], LOGS, "the lookback reads the logs first");
    assert_eq!(hist[1]["channel"], CONFESSIONS);
    assert_eq!(h.fake.responded()[0].1["text"], "Now tracking confession #40125. I'll DM you when its status changes. :neocat:");
    assert_eq!(
        h.fake.posted_texts(),
        [format!(":yay: Your confession #40125 got approved for <#{CONFESSIONS}>! <https://{DOMAIN}/archives/{CONFESSIONS}/p1700000000000300|View it here>")]
    );
}

#[tokio::test(start_paused = true)]
async fn lookback_takes_a_denial_and_waits_for_the_reason() {
    let h = harness("[]", |_| {});
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [{"bot_id": OWL, "text": "40127 denied", "ts": "1700000700.000100"}]}));
    h.fake.reply("conversations.replies", json!({"ok": true, "messages": [{"text": "40127 denied"}, {"text": "nope", "user": "U_REV"}]}));
    h.bot.dispatch(Dispatch::Command(command("40127", ALLOWED))).await;
    assert!(h.fake.posted_texts().is_empty());
    advance(90_000).await;
    assert_eq!(h.fake.calls_to("conversations.replies")[0]["ts"], "1700000700.000100");
    assert_eq!(h.fake.posted_texts(), [":neocat_sad: Your confession #40127 was denied. Reason (will be blank if none was provided): \n> nope <https://hackclub.enterprise.slack.com/archives/C08R8LE6DDG/p1700000700000100|View the thread>"]);
}

#[tokio::test(start_paused = true)]
async fn lookback_ignores_non_owl_and_other_ids_and_stops_at_the_first_match() {
    let h = harness("[]", |_| {});
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [
        {"user": "U_H", "text": "40128 approved to <#CNMU9L92Q>", "ts": "1700000950.000100"},
        {"bot_id": OWL, "text": "40129 approved to <#CNMU9L92Q>", "ts": "1700000940.000100"},
        {"bot_id": OWL, "text": "40128 pending", "ts": "1700000930.000100"},
        {"bot_id": OWL, "text": "40128 unapproved", "ts": "1700000920.000100"}
    ]}));
    h.bot.dispatch(Dispatch::Command(command("40128", ALLOWED))).await;
    assert_eq!(h.fake.calls_to("conversations.history").len(), 1, "the first Owl status for the id is the current one; 'pending' is not a status");
    assert_eq!(read_data(&h)[0]["status"], "unapproved");
    assert!(h.fake.posted_texts()[0].starts_with(":uuh: "));
}

#[tokio::test(start_paused = true)]
async fn lookback_does_not_touch_an_entry_that_is_already_resolved() {
    let h = harness(&data(&[entry("40130", "U1", true)]), |_| {});
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [{"bot_id": OWL, "text": "40130 denied", "ts": "1.1"}]}));
    h.bot.lookback_status("40130").await;
    assert!(h.fake.posted_texts().is_empty());
    assert_eq!(h.fake.calls_to("conversations.history").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn startup_looks_up_every_open_entry_and_skips_resolved_and_pending_ones() {
    let at = iso_from_ms(system_ms());
    let pending = format!(r#"{{"id":"40203","userId":"U1","resolved":false,"date":"{at}","pendingReject":{{"statusTs":"{DENIED_TS}","at":"{at}"}}}}"#);
    let h = harness(&data(&[entry("40201", "U1", false), entry("40202", "U1", true), pending]), |_| {});
    h.fake.reply("conversations.history", json!({"ok": true, "messages": [{"bot_id": OWL, "text": "40201 unapproved", "ts": "1700000900.000100"}]}));
    h.fake.reply("conversations.replies", json!({"ok": true, "messages": [{"text": "40203 denied"}, {"text": "later", "user": "U_REV"}]}));
    h.bot.start_up().unwrap();
    tokio::time::advance(Duration::from_millis(1)).await;
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    assert_eq!(h.fake.calls_to("conversations.history").len(), 1, "only the open entry is looked up");
    assert!(h.fake.posted_texts()[0].starts_with(":uuh: "), "the open entry got its status");
    advance(89_998).await;
    assert_eq!(h.fake.posted_texts().len(), 1, "the pending rejection is still waiting");
    advance(1).await;
    assert_eq!(h.fake.posted_texts().len(), 2);
}
