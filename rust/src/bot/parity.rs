//! Replays `rust/tests/parity/scenarios.json` through the Rust bot and compares every step with what the real
//! `test.ts` produced under Node (see `rust/tests/parity/gen.mts`). A step's text is the JSON of its Web API calls,
//! its `response_url` posts and the data file's exact bytes.
use super::*;
use crate::config::Settings;
use crate::js::Clock;
use crate::testkit::FakeSlack;
use regex_lite::Regex;
use std::fs;
use std::path::PathBuf;

const FIXTURE: &str = include_str!("../../tests/parity/scenarios.json");

/// `calls` grouped by method, each method's calls in order, methods in sorted order (gen.mts `canon`). Calls of
/// different methods that the TypeScript starts concurrently may interleave differently, so only the order within a
/// method is compared.
fn canon(calls: Vec<(String, Value)>) -> Value {
    let mut by: std::collections::BTreeMap<String, Vec<Value>> = std::collections::BTreeMap::new();
    for (m, p) in calls {
        by.entry(m).or_default().push(p);
    }
    Value::Object(by.into_iter().map(|(k, v)| (k, Value::Array(v))).collect())
}

/// Lets spawned timer tasks and their Slack calls run to completion (the fake never blocks).
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

/// The same normalisation as `normalize()` in gen.mts: ISO timestamps, the block id, and the random stupid lines.
fn normalize(text: &str, stupid: &[String]) -> String {
    static ISO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z").unwrap());
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"track_input_[0-9]+").unwrap());
    static MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<@U[A-Z0-9]+>").unwrap());
    let text = MENTION.replace_all(text, "<@USER>");
    let mut out = BLOCK.replace_all(&ISO.replace_all(&text, "<ISO>"), "track_input_<N>").into_owned();
    for line in stupid {
        out = out.split(line.as_str()).collect::<Vec<_>>().join("<STUPID>");
    }
    out
}

async fn run_scenario(sc: &Value, stupid: &[String]) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("mrox-parity-{}-{}", std::process::id(), fastrand::u64(..)));
    fs::create_dir_all(&dir).unwrap();
    let data_file = dir.join("confessions.json");
    fs::write(&data_file, sc["data"].as_str().unwrap()).unwrap();
    let config = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("config.yaml");

    let mut cfg = Settings::defaults(data_file.clone(), config.clone());
    cfg.restrict_to_user = sc["restrict"].as_bool().unwrap_or(true);
    cfg.verify_id_in_status = sc["verify"].as_bool().unwrap_or(false);
    cfg.user_token = sc["userToken"].as_bool().unwrap_or(false).then(|| "xoxp-test".to_string());
    let fake = FakeSlack::new();
    for (method, list) in sc["replies"].as_object().into_iter().flatten() {
        for r in list.as_array().into_iter().flatten() {
            match r.get("__error").and_then(Value::as_str) {
                Some(code) => fake.fail(method, code),
                None => fake.reply(method, r.clone()),
            }
        }
    }
    let msgs = Messages::load(&config);
    let bot = Bot::with_clock(cfg, msgs, Store::new(&data_file), fake.clone(), Clock::tokio());

    let mut out = Vec::new();
    for step in sc["steps"].as_array().unwrap() {
        let kind = step["kind"].as_str().unwrap();
        let d = match kind {
            "advance" => {
                tokio::time::advance(std::time::Duration::from_millis(step["ms"].as_u64().unwrap())).await;
                None
            }
            "resume" => {
                let _ = bot.resume_pending_rejections();
                None
            }
            "startup" => {
                let _ = bot.start_up();
                None
            }
            "command" => Some(Dispatch::Command(step["payload"].clone())),
            "message" => Some(Dispatch::Message(step["event"].clone())),
            "home" => Some(Dispatch::HomeOpened(step["event"].clone())),
            "track" => Some(Dispatch::TrackInput { body: step["body"].clone(), action: step["action"].clone() }),
            "untrack" => Some(Dispatch::Untrack { body: step["body"].clone(), action: step["action"].clone() }),
            _ => None,
        };
        match (kind, d) {
            (_, Some(d)) => bot.dispatch(d).await,
            ("cleanup", None) => {
                let _ = bot.cleanup();
            }
            _ => {}
        }
        settle().await;
        let responded: Vec<Value> = fake.take_responded().into_iter().map(|(u, b)| json!([u, b])).collect();
        let data = fs::read_to_string(&data_file).unwrap();
        let text = serde_json::to_string(&json!({"calls": canon(fake.take_calls()), "responded": responded, "data": data})).unwrap();
        out.push(normalize(&text, stupid));
    }
    let _ = fs::remove_dir_all(&dir);
    out
}

#[tokio::test(start_paused = true)]
async fn replays_every_typescript_scenario_identically() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let stupid: Vec<String> = fixture["stupid"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    let mut failures = Vec::new();
    let mut steps = 0;
    for sc in fixture["scenarios"].as_array().unwrap() {
        let name = sc["name"].as_str().unwrap();
        let expected: Vec<&str> = sc["expected"].as_array().unwrap().iter().map(|e| e.as_str().unwrap()).collect();
        let got = run_scenario(sc, &stupid).await;
        assert_eq!(got.len(), expected.len(), "{name}: step count");
        for (i, (g, e)) in got.iter().zip(&expected).enumerate() {
            steps += 1;
            if g != e {
                failures.push(format!("[{name}] step {i}\n  rust: {g}\n  ts:   {e}"));
            }
        }
    }
    assert!(steps >= 90, "the fixture should cover the scenarios ({steps} steps)");
    assert!(failures.is_empty(), "{} of {steps} steps differ:\n{}", failures.len(), failures.join("\n"));
}
