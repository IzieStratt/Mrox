//! Settings from the environment (the defaults match `test.ts`), and the message catalogue from `config.yaml`
//! with the built-in fallbacks and the `{placeholder}` renderer (`t()` in `test.ts`).
use crate::js;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const RETENTION_MS: f64 = 2.0 * 24.0 * 60.0 * 60.0 * 1000.0; // keep resolved confessions 2 days
pub const HISTORY_SCAN_LIMIT: usize = 5; // pages of 200 when searching for a confession post
pub const CLEANUP_INTERVAL_SECS: u64 = 60 * 60; // hourly
pub const PRIVACY_POLICY_URL: &str = "https://hackclub.enterprise.slack.com/docs/T0266FRGM/F0C9ZFGE6U9";
pub const SLACK_API_BASE: &str = "https://slack.com/api";

/// Runtime settings. Field comments give the env var and the `test.ts` default.
#[derive(Clone, Debug)]
pub struct Settings {
    /// `RESTRICT_TO_USER` (anything but the exact string "false" restricts). Default true.
    pub restrict_to_user: bool,
    /// `ALLOWED_USER_ID`. No default: unset means nobody may use /mrox (set it in .env). See README and PARITY.md.
    pub allowed_user_id: String,
    /// `CHANNEL_CONFESSIONS`. Default `CNMU9L92Q`.
    pub channel_confessions: String,
    /// `CHANNEL_CONFESSION_LOGS`. Default `C08R8LE6DDG`.
    pub channel_confession_logs: String,
    /// `CHANNEL_META`. Default `C0188CY57PZ`.
    pub channel_meta: String,
    /// `OWL_BOT_ID`. Default `B0C6JPG4W0G`.
    pub owl_bot_id: String,
    /// `OWL_USER_ID`. Default `U0C5Q89C2P4`.
    pub owl_user_id: String,
    /// `LOGS_CHANNEL_NAME` (used in the search query). Default `confessions-log`.
    pub logs_channel_name: String,
    /// `SLACK_USER_TOKEN`: optional user token with search:read. Without it the status search is skipped.
    pub user_token: Option<String>,
    /// `WORKSPACE_DOMAIN`. Default `hackclub.enterprise.slack.com`.
    pub workspace_domain: String,
    /// `DATA_FILE`. Default `<cwd>/confessions.json`.
    pub data_file: PathBuf,
    /// `CONFIG_FILE`. Default `<cwd>/config.yaml`.
    pub config_file: PathBuf,
    /// `VERIFY_ID_IN_STATUS` (exactly "true" enables). Default false.
    pub verify_id_in_status: bool,
    /// `DEBUG` (exactly "true" enables stdout logging). Default false.
    pub debug: bool,
    /// `SLACK_BOT_TOKEN`.
    pub bot_token: Option<String>,
    /// `SLACK_APP_TOKEN`.
    pub app_token: Option<String>,
    /// `MROX_SLACK_API_BASE`: test hook only (mock server for measurement). Default `https://slack.com/api`.
    pub api_base: String,
    /// `REASON_QUIET_MS`: a rejection's DM waits this long after the reviewer's last thread reply. Default 90000.
    pub reason_quiet_ms: f64,
    /// `REASON_MAX_WAIT_MS`: a rejection's DM goes out at most this long after the rejection. Default 600000.
    pub reason_max_wait_ms: f64,
}

impl Settings {
    /// Reads the environment. `cwd` is `process.cwd()` for the two file defaults.
    pub fn from_env(cwd: &Path) -> Settings {
        let env = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_string());
        let env_opt = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        Settings {
            restrict_to_user: std::env::var("RESTRICT_TO_USER").map_or(true, |v| v != "false"),
            allowed_user_id: env("ALLOWED_USER_ID", ""),
            channel_confessions: env("CHANNEL_CONFESSIONS", "CNMU9L92Q"),
            channel_confession_logs: env("CHANNEL_CONFESSION_LOGS", "C08R8LE6DDG"),
            channel_meta: env("CHANNEL_META", "C0188CY57PZ"),
            owl_bot_id: env("OWL_BOT_ID", "B0C6JPG4W0G"),
            owl_user_id: env("OWL_USER_ID", "U0C5Q89C2P4"),
            logs_channel_name: env("LOGS_CHANNEL_NAME", "confessions-log"),
            user_token: env_opt("SLACK_USER_TOKEN"),
            workspace_domain: env("WORKSPACE_DOMAIN", "hackclub.enterprise.slack.com"),
            data_file: PathBuf::from(env("DATA_FILE", &cwd.join("confessions.json").to_string_lossy())),
            config_file: PathBuf::from(env("CONFIG_FILE", &cwd.join("config.yaml").to_string_lossy())),
            verify_id_in_status: std::env::var("VERIFY_ID_IN_STATUS").is_ok_and(|v| v == "true"),
            debug: std::env::var("DEBUG").is_ok_and(|v| v == "true"),
            bot_token: std::env::var("SLACK_BOT_TOKEN").ok(),
            app_token: std::env::var("SLACK_APP_TOKEN").ok(),
            api_base: env_opt("MROX_SLACK_API_BASE").unwrap_or_else(|| SLACK_API_BASE.to_string()),
            // `Number(process.env.X ?? default)`: a set value is converted as a number (so "" is 0, "abc" is NaN).
            reason_quiet_ms: std::env::var("REASON_QUIET_MS").map_or(90_000.0, |v| js::string_to_number(&v)),
            reason_max_wait_ms: std::env::var("REASON_MAX_WAIT_MS").map_or(600_000.0, |v| js::string_to_number(&v)),
        }
    }

    /// Settings with the `test.ts` defaults and the given files, independent of the process environment.
    pub fn defaults(data_file: PathBuf, config_file: PathBuf) -> Settings {
        Settings {
            restrict_to_user: true,
            allowed_user_id: "U00000TEST".into(),
            channel_confessions: "CNMU9L92Q".into(),
            channel_confession_logs: "C08R8LE6DDG".into(),
            channel_meta: "C0188CY57PZ".into(),
            owl_bot_id: "B0C6JPG4W0G".into(),
            owl_user_id: "U0C5Q89C2P4".into(),
            logs_channel_name: "confessions-log".into(),
            user_token: None,
            workspace_domain: "hackclub.enterprise.slack.com".into(),
            data_file,
            config_file,
            verify_id_in_status: false,
            debug: false,
            bot_token: Some("xoxb-test".into()),
            app_token: Some("xapp-test".into()),
            api_base: SLACK_API_BASE.into(),
            reason_quiet_ms: 90_000.0,
            reason_max_wait_ms: 600_000.0,
        }
    }
}

/// Built-in message fallbacks (`FALLBACK_MESSAGES` in `test.ts`), used for any key config.yaml does not set.
pub fn fallback_messages() -> Map<String, Value> {
    let mut m = Map::new();
    let pairs: [(&str, &str); 11] = [
        ("not_allowed", "You're not allowed to use this."),
        ("slash_help", "Welcome to Mrox! To start tracking a confession please run: `/mrox {id}`, or paste it into the box on my App Home."),
        ("dm_refuse", "sorry! for your security I can't accept ids via dms, please use /mrox or the app home :D"),
        ("not_yours", "Confession #{confessionID} is already being tracked by someone else."),
        ("already_tracked", "Confession #{confessionID} is {status}."),
        ("now_tracking", "Now tracking confession #{confessionID}. I'll DM you when its status changes."),
        ("untracked", "Stopped tracking confession #{confessionID}."),
        ("untrack_missing", "Confession #{confessionID} isn't being tracked."),
        ("untrack_below", "Stopped tracking {count} confession(s) below #{threshold}."),
        ("approved", ":yay: Your confession #{confessionID} got approved for <#{channel}>!{link}"),
        ("meta_approved", ":meta: Your confession #{confessionID} got approved for <#{channel}>!{link}"),
    ];
    for (k, v) in pairs {
        m.insert(k.into(), json!(v));
    }
    m.insert("unapproved".into(), json!(":uuh: Your confession #{confessionID} was unapproved and the thread has been nuked."));
    m.insert("rejected".into(), json!(":neocat_sad: Your confession #{confessionID} was denied.{reason}{link}"));
    m.insert("stupid_lines".into(), json!(STUPID_FALLBACK));
    m
}

/// `STUPID_FALLBACK` in `test.ts`.
pub const STUPID_FALLBACK: [&str; 2] = ["that confession doesn't exist... yet :eyes:", "whoa there, that's not supposed to happen. neat."];

/// Built-in App Home fallbacks (`FALLBACK_HOME` in `test.ts`).
pub fn fallback_home() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("header".into(), json!("*Mrox* :neocat_glasses:"));
    m
}

/// The loaded message catalogue: `CONFIG.messages` and `CONFIG.home`.
#[derive(Clone, Debug)]
pub struct Messages {
    pub messages: Map<String, Value>,
    pub home: Map<String, Value>,
}

impl Messages {
    /// `loadConfig()`: reads the YAML file, merging over the built-in fallbacks. A missing or unparsable file
    /// warns on stderr and uses the fallbacks alone.
    pub fn load(path: &Path) -> Messages {
        match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| Messages::from_yaml(&t)) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Could not load {}, using built-in messages: {e}", path.display());
                Messages::fallback()
            }
        }
    }

    /// Only the built-in fallbacks.
    pub fn fallback() -> Messages {
        Messages { messages: fallback_messages(), home: fallback_home() }
    }

    /// Parses config.yaml text. A document that is not a mapping, or has no `messages`/`home` mapping, contributes
    /// nothing (the JS spread of a non-object adds no usable keys).
    pub fn from_yaml(text: &str) -> Result<Messages, String> {
        let docs = yaml_rust2::YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
        let raw = docs.first().map(yaml_to_json).unwrap_or(Value::Null);
        let mut out = Messages::fallback();
        if let Some(Value::Object(m)) = raw.get("messages") {
            out.messages.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        if let Some(Value::Object(h)) = raw.get("home") {
            out.home.extend(h.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        Ok(out)
    }

    /// `t(key, vars)`: the first element when the entry is a list, `""` for a missing or null entry, then every
    /// `{name}` whose name is in `vars` is replaced (single pass; unknown placeholders stay as written).
    /// A non-string template is an error, as `template.replace` throws in JS.
    pub fn t(&self, key: &str, vars: &[(&str, String)]) -> Result<String, String> {
        let template: &Value = match self.messages.get(key) {
            Some(Value::Array(a)) => match a.first() {
                None | Some(Value::Null) => return Ok(String::new()),
                Some(v) => v,
            },
            None | Some(Value::Null) => return Ok(String::new()),
            Some(v) => v,
        };
        let Value::String(template) = template else {
            return Err(format!("template.replace is not a function (key {key})"));
        };
        Ok(render(template, vars))
    }

    /// `stupidLines()`: the configured list when non-empty, else the built-in list.
    pub fn stupid_lines(&self) -> Vec<Value> {
        match self.messages.get("stupid_lines") {
            Some(Value::Array(a)) if !a.is_empty() => a.clone(),
            _ => STUPID_FALLBACK.iter().map(|s| json!(s)).collect(),
        }
    }

    /// A home-view text: the config value when set (any JSON value, as in JS), else the default.
    pub fn home_value(&self, key: &str, default: &str) -> Value {
        match self.home.get(key) {
            None | Some(Value::Null) => json!(default),
            Some(v) => v.clone(),
        }
    }
}

/// Replaces `{name}` (name is `\w+`) with `vars[name]` when present, in one left-to-right pass.
pub fn render(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name_len = after.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
        if name_len > 0 && after.as_bytes().get(name_len) == Some(&b'}') {
            let name = &after[..name_len];
            match vars.iter().find(|(k, _)| *k == name) {
                Some((_, v)) => out.push_str(v),
                None => out.push_str(&rest[open..open + 2 + name_len]),
            }
            rest = &after[name_len + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// YAML to JSON for the config shapes the bot reads. Scalars that JSON cannot hold become null.
pub fn yaml_to_json(y: &yaml_rust2::Yaml) -> Value {
    use yaml_rust2::Yaml;
    match y {
        Yaml::Real(s) => s.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map_or(Value::Null, Value::Number),
        Yaml::Integer(i) => json!(i),
        Yaml::String(s) => Value::String(s.clone()),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Array(a) => Value::Array(a.iter().map(yaml_to_json).collect()),
        Yaml::Hash(h) => Value::Object(
            h.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Yaml::String(s) => s.clone(),
                        other => js::js_string(Some(&yaml_to_json(other))),
                    };
                    (key, yaml_to_json(v))
                })
                .collect(),
        ),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_config() -> Messages {
        Messages::from_yaml(include_str!("../../config.yaml")).unwrap()
    }

    #[test]
    fn renders_placeholders_once_and_keeps_unknown_ones() {
        assert_eq!(render("a {x} {y} {z}", &[("x", "{y}".into()), ("y", "Y".into())]), "a {y} Y {z}");
        assert_eq!(render("{ {a} {a_1} {a-b} }", &[("a", "1".into())]), "{ 1 {a_1} {a-b} }");
        assert_eq!(render("}{", &[]), "}{");
    }

    #[test]
    fn config_yaml_messages_win_over_fallbacks() {
        let m = real_config();
        assert!(m.t("not_allowed", &[]).unwrap().starts_with("sorgy u cant do this, dm <@"));
        assert_eq!(
            m.t("rejected", &[("confessionID", "40101".into()), ("reason", "".into()), ("link", "".into())]).unwrap(),
            ":neocat_sad: Your confession #40101 was denied. Reason (will be blank if none was provided): "
        );
        assert_eq!(
            m.t("now_tracking", &[("confessionID", "1".into())]).unwrap(),
            "Now tracking confession #1. I'll DM you when its status changes. :neocat:"
        );
        assert_eq!(
            m.home_value("header", "x"),
            json!("*Mrox* :neocat_glasses:\nPaste your confession ID below and press Enter, and I'll DM you when its status changes.")
        );
        assert_eq!(m.stupid_lines().len(), 5);
    }

    #[test]
    fn fallbacks_apply_when_keys_are_missing() {
        let m = Messages::from_yaml("messages:\n  not_allowed: \"nope\"\n").unwrap();
        assert_eq!(m.t("not_allowed", &[]).unwrap(), "nope");
        assert_eq!(
            m.t("rejected", &[("confessionID", "7".into()), ("reason", "\n> x".into()), ("link", " <u|v>".into())]).unwrap(),
            ":neocat_sad: Your confession #7 was denied.\n> x <u|v>"
        );
        assert_eq!(m.stupid_lines().len(), 2);
        assert_eq!(m.home_value("track_label", "Track a confession"), json!("Track a confession"));
    }

    #[test]
    fn list_entries_use_the_first_item_and_null_is_empty() {
        let m = Messages::from_yaml("messages:\n  a:\n    - first\n    - second\n  b: ~\n  c: []\n").unwrap();
        assert_eq!(m.t("a", &[]).unwrap(), "first");
        assert_eq!(m.t("b", &[]).unwrap(), "");
        assert_eq!(m.t("c", &[]).unwrap(), "");
        assert_eq!(m.t("missing", &[]).unwrap(), "");
    }

    #[test]
    fn non_string_templates_are_errors() {
        let m = Messages::from_yaml("messages:\n  n: 5\n").unwrap();
        assert!(m.t("n", &[]).is_err());
    }

    #[test]
    fn unreadable_config_falls_back_to_builtins() {
        let m = Messages::load(Path::new("/nonexistent/mrox-config.yaml"));
        assert_eq!(m.t("untracked", &[("confessionID", "9".into())]).unwrap(), "Stopped tracking confession #9.");
    }
}
