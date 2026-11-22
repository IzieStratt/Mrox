//! JavaScript semantics the TypeScript bot relies on: `trim()`, `\s`, truthiness, `Number()`, `String()`,
//! `new Date(x).getTime()` and `new Date().toISOString()`.
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use regex_lite::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// JS `\s` plus line terminators, as the body of a regex character class (`[...]`).
pub const WS: &str = r"\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";

/// One character of JS whitespace (what `String.prototype.trim` strips). Note U+FEFF counts, U+0085 does not.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `s.trim()`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// JS truthiness of a JSON value; a missing key (`None`) is `undefined`, which is falsy.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `x === "..."`: true only for a JSON string equal to `s`.
pub fn str_eq(v: Option<&Value>, s: &str) -> bool {
    matches!(v, Some(Value::String(x)) if x == s)
}

/// `Number(x)` for a JSON value (`None` is `undefined`, which gives NaN).
pub fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => string_to_number(s),
        Some(_) => f64::NAN,
    }
}

/// `Number("...")` for a string: decimal, Infinity, and 0x/0o/0b integers; anything else is NaN.
pub fn string_to_number(s: &str) -> f64 {
    static DEC: OnceLock<Regex> = OnceLock::new();
    static RADIX: OnceLock<Regex> = OnceLock::new();
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let dec = DEC.get_or_init(|| Regex::new(r"^[+-]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$").unwrap());
    if dec.is_match(t) {
        return t.parse::<f64>().unwrap_or(f64::NAN);
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let radix = RADIX.get_or_init(|| Regex::new(r"^0([xXoObB])([0-9a-fA-F]+)$").unwrap());
    if let Some(c) = radix.captures(t) {
        let base = match &c[1] {
            "x" | "X" => 16,
            "o" | "O" => 8,
            _ => 2,
        };
        return u128::from_str_radix(&c[2], base).map_or(f64::NAN, |n| n as f64);
    }
    f64::NAN
}

/// `String(n)` for a JS number.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into();
    }
    let a = n.abs();
    if !(1e-6..1e21).contains(&a) {
        // JS switches to exponent form outside this range; Rust's `{:e}` gives the same shortest digits.
        let s = format!("{n:e}");
        let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let exp: i32 = exp.parse().unwrap_or(0);
        return format!("{mant}e{}{}", if exp < 0 { "-" } else { "+" }, exp.abs());
    }
    format!("{n}")
}

/// `String(x)` / template-literal coercion of a JSON value. `None` is `undefined`.
pub fn js_string(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().map(|x| if x.is_null() { String::new() } else { js_string(Some(x)) }).collect::<Vec<_>>().join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `new Date(x).getTime()`; NaN when JS would give an invalid date.
pub fn date_ms(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => parse_date(s),
        Some(_) => f64::NAN,
    }
}

/// `Date.parse` for the ISO 8601 forms JS accepts: date-only is UTC, date-time without an offset is local time.
/// The legacy non-ISO fallbacks V8 accepts are not replicated; the bot only ever writes `toISOString()` output.
pub fn parse_date(s: &str) -> f64 {
    static ISO: OnceLock<Regex> = OnceLock::new();
    let re = ISO.get_or_init(|| {
        Regex::new(r"^([+-][0-9]{6}|[0-9]{4})(?:-([0-9]{2})(?:-([0-9]{2}))?)?(?:T([0-9]{2}):([0-9]{2})(?::([0-9]{2})(?:\.([0-9]+))?)?(Z|[+-][0-9]{2}:[0-9]{2})?)?$").unwrap()
    });
    let Some(c) = re.captures(s) else { return f64::NAN };
    let num = |i: usize, default: u32| -> Option<u32> { c.get(i).map_or(Some(default), |m| m.as_str().parse().ok()) };
    let (Some(month), Some(day)) = (num(2, 1), num(3, 1)) else { return f64::NAN };
    let Ok(year) = c[1].parse::<i32>() else { return f64::NAN };
    let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else { return f64::NAN };
    let has_time = c.get(4).is_some();
    let (Some(h), Some(mi), Some(sec)) = (num(4, 0), num(5, 0), num(6, 0)) else { return f64::NAN };
    let ms: u32 = c.get(7).map_or(0, |m| {
        let digits: String = m.as_str().chars().chain("000".chars()).take(3).collect();
        digits.parse().unwrap_or(0)
    });
    let Some(time) = NaiveTime::from_hms_milli_opt(h, mi, sec, ms) else { return f64::NAN };
    let naive = NaiveDateTime::new(date, if has_time { time } else { NaiveTime::MIN });
    let utc_ms = match c.get(8).map(|m| m.as_str()) {
        Some("Z") => naive.and_utc().timestamp_millis(),
        Some(off) => {
            let sign: i64 = if off.starts_with('-') { -1 } else { 1 };
            let secs = sign * (off[1..3].parse::<i64>().unwrap_or(0) * 3600 + off[4..6].parse::<i64>().unwrap_or(0) * 60);
            naive.and_utc().timestamp_millis() - secs * 1000
        }
        None if has_time => match chrono::Local.from_local_datetime(&naive).earliest() {
            Some(local) => local.timestamp_millis(),
            None => return f64::NAN,
        },
        None => naive.and_utc().timestamp_millis(),
    };
    utc_ms as f64
}

/// `Date.now()` on the system clock, floored to whole milliseconds.
pub fn system_ms() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64
}

/// `new Date(ms).toISOString()`, e.g. `2026-11-21T20:00:00.000Z`.
pub fn iso_from_ms(ms: f64) -> String {
    let dt = chrono::DateTime::from_timestamp_millis(ms as i64).unwrap_or_default();
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// The bot's wall clock (`Date.now()`, `new Date()`). Production reads the system clock. Tests use a tokio clock,
/// which follows the runtime's (possibly paused) time, so the rejection timers and their deadlines stay in step.
#[derive(Clone, Copy, Debug)]
pub enum Clock {
    System,
    Tokio { wall_ms: i64, origin: tokio::time::Instant },
}

impl Clock {
    /// A tokio-backed clock starting now (call it inside the runtime whose time it should follow).
    pub fn tokio() -> Clock {
        Clock::Tokio { wall_ms: system_ms() as i64, origin: tokio::time::Instant::now() }
    }

    /// Whole milliseconds since the epoch.
    pub fn now_ms(&self) -> f64 {
        match *self {
            Clock::System => system_ms(),
            Clock::Tokio { wall_ms, origin } => (wall_ms as u128 + origin.elapsed().as_millis()) as f64,
        }
    }
}

/// `Math.min(a, b)`: NaN if either is NaN.
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// The delay `setTimeout(fn, raw)` actually uses in Node: `Math.max(0, raw)`, but anything below 1 ms, NaN, or
/// above the 2^31-1 ms limit becomes 1 ms.
pub fn timeout_delay_ms(raw: f64) -> u64 {
    if raw.is_nan() {
        return 1;
    }
    let d = raw.max(0.0);
    if !(1.0..=2_147_483_647.0).contains(&d) {
        return 1;
    }
    d as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trims_like_js() {
        assert_eq!(trim("\u{feff} \t12\n\u{3000}"), "12");
        // U+0085 (NEL) is not JS whitespace, so it survives trim().
        assert_eq!(trim("\u{85}x"), "\u{85}x");
    }

    #[test]
    fn truthiness() {
        assert!(!truthy(None));
        assert!(!truthy(Some(&json!(null))));
        assert!(!truthy(Some(&json!(0))));
        assert!(!truthy(Some(&json!(""))));
        assert!(truthy(Some(&json!("0"))));
        assert!(truthy(Some(&json!([]))));
        assert!(truthy(Some(&json!(true))));
    }

    #[test]
    fn number_strings_match_js() {
        assert_eq!(number_to_string(3.0), "3");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(string_to_number(" 42 "), 42.0);
        assert_eq!(string_to_number(""), 0.0);
        assert!(string_to_number("4x").is_nan());
        assert_eq!(string_to_number("0x1f"), 31.0);
    }

    #[test]
    fn dates_match_js() {
        // Date.parse("2026-11-21T20:00:00.000Z") = 1795291200000
        assert_eq!(parse_date("2026-11-21T20:00:00.000Z"), 1_795_291_200_000.0);
        assert_eq!(parse_date("2026-11-21T20:00:00.000+01:00"), 1_795_287_600_000.0);
        assert_eq!(parse_date("2026-11-21"), 1_795_219_200_000.0);
        assert!(parse_date("not a date").is_nan());
        assert!(parse_date("2026-13-01T00:00:00.000Z").is_nan());
        assert_eq!(date_ms(Some(&json!(null))), 0.0);
        assert!(date_ms(None).is_nan());
    }

    #[test]
    fn iso_strings_round_trip_through_date_parsing() {
        let s = iso_from_ms(1_795_291_200_123.0);
        assert_eq!(s, "2026-11-21T20:00:00.123Z");
        assert_eq!(parse_date(&s), 1_795_291_200_123.0);
    }

    #[test]
    fn node_timeout_clamping() {
        assert_eq!(timeout_delay_ms(f64::NAN), 1);
        assert_eq!(timeout_delay_ms(-5.0), 1);
        assert_eq!(timeout_delay_ms(0.0), 1);
        assert_eq!(timeout_delay_ms(90_000.7), 90_000);
        assert_eq!(timeout_delay_ms(3_000_000_000.0), 1);
        assert!(js_min(1.0, f64::NAN).is_nan());
    }

    #[tokio::test(start_paused = true)]
    async fn tokio_clock_follows_paused_time() {
        let clock = Clock::tokio();
        let before = clock.now_ms();
        tokio::time::advance(std::time::Duration::from_millis(1_500)).await;
        assert_eq!(clock.now_ms() - before, 1_500.0);
    }
}
