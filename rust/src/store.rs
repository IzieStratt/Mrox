//! `confessions.json`: the same file, format and mutation order as the TypeScript helpers
//! (`readConfessions`, `writeJsonAtomic`, `updateConfession`, `untrackConfession`, `untrackBelow`, `cleanup`).
//! Entries stay as JSON values so unknown keys survive a round trip and key order matches `Object.assign`.
use crate::js::{date_ms, str_eq, to_number, truthy};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Store {
        Store { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `readJson(DATA_FILE, [])`: a missing, unreadable or unparsable file reads as an empty list.
    fn read_raw(&self) -> Value {
        fs::read_to_string(&self.path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!([]))
    }

    /// `readConfessions()`. A file that parses to something other than an array makes JS throw on `.find`/`.filter`,
    /// so that is an error here too.
    pub fn list(&self) -> Result<Vec<Value>, String> {
        match self.read_raw() {
            Value::Array(a) => Ok(a),
            other => Err(format!("confessions.json is not a list (found {})", crate::js::js_string(Some(&other)))),
        }
    }

    /// `writeJsonAtomic`: `JSON.stringify(value, null, 2)` to `<file>.<pid>.tmp`, then rename over the file.
    pub fn write_list(&self, list: &[Value]) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&Value::Array(list.to_vec())).map_err(|e| e.to_string())?;
        let tmp = PathBuf::from(format!("{}.{}.tmp", self.path.display(), std::process::id()));
        fs::write(&tmp, text).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    /// `ensureFiles()`: creates the data file with `[]` when it does not exist.
    pub fn ensure(&self) -> Result<(), String> {
        if !self.path.exists() {
            self.write_list(&[])?;
        }
        Ok(())
    }

    /// `findConfession(id)`: the first entry whose `id` is the string `id`.
    pub fn find(&self, id: &str) -> Result<Option<Value>, String> {
        Ok(self.list()?.into_iter().find(|c| str_eq(c.get("id"), id)))
    }

    /// `updateConfession(id, patch)`: `Object.assign` onto the first match, then write. A `None` value is JS
    /// `undefined`: the key is dropped from the file (JSON.stringify omits it). Returns whether it matched.
    pub fn update(&self, id: &str, patch: &[(&str, Option<Value>)]) -> Result<bool, String> {
        let mut list = self.list()?;
        let Some(found) = list.iter_mut().find(|c| str_eq(c.get("id"), id)) else {
            return Ok(false);
        };
        if let Some(obj) = found.as_object_mut() {
            for (k, v) in patch {
                match v {
                    Some(v) => {
                        obj.insert((*k).to_string(), v.clone());
                    }
                    None => {
                        obj.shift_remove(*k);
                    }
                }
            }
        }
        self.write_list(&list)?;
        Ok(true)
    }

    /// `setResolved(id, status)`.
    pub fn set_resolved(&self, id: &str, status: &str) -> Result<(), String> {
        self.update(id, &[("resolved", Some(json!(true))), ("status", Some(json!(status)))])?;
        Ok(())
    }

    /// `readConfessions().find((c) => c.pendingReject?.statusTs === threadTs)`.
    pub fn find_pending(&self, thread_ts: &Value) -> Result<Option<Value>, String> {
        Ok(self.list()?.into_iter().find(|c| c.get("pendingReject").and_then(|p| p.get("statusTs")) == Some(thread_ts)))
    }

    /// `trackConfession`'s write: appends one entry.
    pub fn push(&self, entry: Value) -> Result<(), String> {
        let mut list = self.list()?;
        list.push(entry);
        self.write_list(&list)
    }

    /// `untrackConfession(id, userId)`: removes the user's entries with this id. Writes only when something was removed.
    pub fn untrack(&self, id: &str, user: &str) -> Result<bool, String> {
        let data = self.list()?;
        let kept: Vec<Value> = data.iter().filter(|c| !(str_eq(c.get("id"), id) && str_eq(c.get("userId"), user))).cloned().collect();
        if kept.len() == data.len() {
            return Ok(false);
        }
        self.write_list(&kept)?;
        Ok(true)
    }

    /// `untrackBelow(n, userId)`: removes the user's entries whose numeric id is below `n`. Returns the count removed.
    pub fn untrack_below(&self, n: f64, user: &str) -> Result<usize, String> {
        let data = self.list()?;
        let kept: Vec<Value> = data
            .iter()
            .filter(|c| {
                let num = to_number(c.get("id"));
                !(str_eq(c.get("userId"), user) && num.is_finite() && num < n)
            })
            .cloned()
            .collect();
        let removed = data.len() - kept.len();
        if removed > 0 {
            self.write_list(&kept)?;
        }
        Ok(removed)
    }

    /// `trackedBy(userId)`: the user's unresolved entries, in file order.
    pub fn tracked_by(&self, user: &str) -> Result<Vec<Value>, String> {
        Ok(self.list()?.into_iter().filter(|c| str_eq(c.get("userId"), user) && !truthy(c.get("resolved"))).collect())
    }

    /// `cleanup(oldMs)`: drops resolved entries older than `old_ms` (measured from `now`). Rewrites the file only
    /// when something expired.
    pub fn cleanup(&self, old_ms: f64, now: f64) -> Result<(), String> {
        let all = self.list()?;
        let data: Vec<Value> = all.iter().filter(|c| !(truthy(c.get("resolved")) && now - date_ms(c.get("date")) > old_ms)).cloned().collect();
        if data.len() != all.len() {
            self.write_list(&data)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RETENTION_MS;
    use serde_json::json;

    fn temp_store(name: &str, contents: Option<&str>) -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!("mrox-store-{}-{name}-{}", std::process::id(), fastrand::u64(..)));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("confessions.json");
        if let Some(c) = contents {
            fs::write(&path, c).unwrap();
        }
        (Store::new(&path), dir)
    }

    /// Expected bytes produced by Node: `JSON.stringify(value, null, 2)` (see tests/fixtures/stringify.mjs).
    const NODE_FORMAT: &str = "[\n  {\n    \"id\": \"40101\",\n    \"userId\": \"U1\",\n    \"resolved\": true,\n    \"date\": \"2026-11-21T20:00:00.123Z\",\n    \"postTs\": \"1700000000.000100\",\n    \"status\": \"approved\",\n    \"extra\": {\n      \"nested\": [],\n      \"empty\": {},\n      \"text\": \"caf\u{e9} \\\"q\\\" \\\\ \\n\\t\\u001b /\"\n    }\n  }\n]";

    #[test]
    fn writes_exactly_like_json_stringify_with_indent_two() {
        let (store, dir) = temp_store("format", None);
        let value: Vec<Value> = vec![json!({
            "id": "40101", "userId": "U1", "resolved": true, "date": "2026-11-21T20:00:00.123Z",
            "postTs": "1700000000.000100", "status": "approved",
            "extra": {"nested": [], "empty": {}, "text": "caf\u{e9} \"q\" \\ \n\t\u{1b} /"}
        })];
        store.write_list(&value).unwrap();
        let bytes = fs::read_to_string(store.path()).unwrap();
        assert_eq!(bytes, NODE_FORMAT);
        assert!(!bytes.ends_with('\n'));
        // no temp file left behind
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ensure_creates_an_empty_list_once() {
        let (store, dir) = temp_store("ensure", None);
        store.ensure().unwrap();
        assert_eq!(fs::read_to_string(store.path()).unwrap(), "[]");
        fs::write(store.path(), "[{\"id\":\"1\"}]").unwrap();
        store.ensure().unwrap();
        assert_eq!(fs::read_to_string(store.path()).unwrap(), "[{\"id\":\"1\"}]");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unparsable_file_reads_as_empty_but_non_list_is_an_error() {
        let (store, dir) = temp_store("unparsable", Some("{not json"));
        assert!(store.list().unwrap().is_empty());
        fs::write(store.path(), "{}").unwrap();
        assert!(store.list().is_err());
        assert!(store.cleanup(RETENTION_MS, crate::js::system_ms()).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn update_keeps_key_order_like_object_assign() {
        let (store, dir) = temp_store("assign", Some(r#"[{"id":"5","userId":"U1","resolved":false,"date":"2026-01-01T00:00:00.000Z"}]"#));
        assert!(store.update("5", &[("postTs", Some(json!("1.5")))]).unwrap());
        store.set_resolved("5", "meta").unwrap();
        let text = fs::read_to_string(store.path()).unwrap();
        let order: Vec<usize> = ["\"id\"", "\"userId\"", "\"resolved\"", "\"date\"", "\"postTs\"", "\"status\""]
            .iter()
            .map(|k| text.find(k).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
        assert!(!store.update("missing", &[]).unwrap());
        // an undefined value drops the key, as JSON.stringify would
        assert!(store.update("5", &[("postTs", None)]).unwrap());
        assert!(!fs::read_to_string(store.path()).unwrap().contains("postTs"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_drops_only_expired_resolved_entries_and_writes_only_then() {
        let old = "2020-01-01T00:00:00.000Z";
        let fresh = crate::js::iso_from_ms(crate::js::system_ms());
        let body = format!(
            r#"[{{"id":"1","userId":"U","resolved":true,"date":"{old}"}},{{"id":"2","userId":"U","resolved":true,"date":"{fresh}"}},{{"id":"3","userId":"U","resolved":false,"date":"{old}"}},{{"id":"4","userId":"U","resolved":true,"date":"bogus"}}]"#
        );
        let (store, dir) = temp_store("cleanup", Some(&body));
        store.cleanup(RETENTION_MS, crate::js::system_ms()).unwrap();
        let ids: Vec<String> = store.list().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids, ["2", "3", "4"]);

        // nothing expires: the file must not be rewritten (same bytes, same mtime-independent check via content)
        let before = fs::read_to_string(store.path()).unwrap();
        store.cleanup(RETENTION_MS, crate::js::system_ms()).unwrap();
        assert_eq!(fs::read_to_string(store.path()).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn untrack_and_below_only_touch_the_callers_entries() {
        let body = r#"[{"id":"100","userId":"A"},{"id":"200","userId":"A"},{"id":"50","userId":"B"},{"id":"30","userId":"A"},{"id":"x","userId":"A"}]"#;
        let (store, dir) = temp_store("untrack", Some(body));
        assert!(!store.untrack("999", "A").unwrap());
        assert!(store.untrack("200", "A").unwrap());
        assert!(!store.untrack("50", "A").unwrap(), "another user's entry is not removed");
        assert_eq!(store.untrack_below(101.0, "A").unwrap(), 2, "100 and 30 go; x is not a number");
        let ids: Vec<String> = store.list().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids, ["50", "x"]);
        assert_eq!(store.untrack_below(1.0, "A").unwrap(), 0);
        fs::remove_dir_all(dir).unwrap();
    }
}
