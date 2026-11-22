# Parity: `test.ts` (Node) vs `rust/` (Rust)

Reference: `test.ts` on the `rust-port` branch, which includes the `reason-wait` work (deferred rejection DMs, the
reviewer-only reason, `unrejected`, the status lookbacks, the search-first lookup and the startup lookbacks).
`main.ts` is dead code and is not ported.

## How parity is checked

- **Replay fixture** (`tests/parity/scenarios.json`, made by `tests/parity/gen.mts`): the real `test.ts` runs under
  Node with Bolt and `WebClient` stubbed out. Time is virtual (`Date` and `setTimeout` are replaced, and an `advance`
  step fires the timers that fall due). Each scenario's steps record the Web API calls (grouped by method, order kept
  within a method), the `response_url` posts, and the exact bytes of `confessions.json` after the step. The Rust bot
  replays the same inputs (`src/bot/parity.rs`) and must produce the same text for every step.
  35 scenarios, 101 steps, currently all identical.
- **Unit tests** (`src/bot/tests.rs`, `src/store.rs`, `src/config.rs`, `src/js.rs`, `src/slack/socket.rs`): every
  listener, the status regexes and classification, the untrack grammar, cleanup expiry, the JSON byte format, the
  timers (paused tokio clock), and the socket loop.
- **Byte format**: `JSON.stringify(value, null, 2)` was reproduced exactly (checked against Node output).
- Normalised on both sides: ISO timestamps, the `track_input_<ms>` block id, the five random stupid lines, and
  `<@U…>` mentions (the not-allowed text names the operator from `config.yaml`).

## Listeners and entry points

| Listener (`test.ts`) | Rust | Notes |
| --- | --- | --- |
| `app.command("/mrox")` | `Bot::on_command` | Acked first. Not-allowed check, then `handleInput`. Reply via `response_url` POST `{"text": ...}`. |
| `app.event("message")` | `Bot::on_message` | Edits (`message_changed`) unwrap `message`; other subtypes ignored; `bot_message` kept. |
| `app.event("app_home_opened")` | `Bot::on_home_opened` | Publishes the home view for any user (no allowed-user check, as in the TS). |
| `app.action("track_confession_input")` | `Bot::on_track_input` | Empty value: nothing published. Not-allowed gets the not-allowed notice. |
| `app.action("untrack_confession")` | `Bot::on_untrack` | `handleInput("untrack " + value)`. |
| `main()` start-up | `main.rs` + `Bot::start_up` | See "Start-up" below. |
| `setInterval(cleanup, 1h)` | `main.rs` interval task | First run one hour after start. |
| Rejection timers | `Bot::schedule_rejection` / `send_rejection` | See "Timers". |
| Socket Mode | `slack/socket.rs`, `slack/ws.rs` | Ack before handling. |

Routing (`slack/socket.rs::route`): `events_api` `message` and `app_home_opened`; `slash_commands` with command
`/mrox`; `interactive` `block_actions` whose first action has one of the two action ids. Everything else is acked
and dropped.

Bolt's `ignoreSelf` is not replicated. It cannot change behaviour: the bot's own posts are never Owl messages, and
its DMs and stupid lines are never processed.

## Commands and input grammar (`handleInput`)

Input is trimmed (JS whitespace). In order:

1. `^untrack\s+(.+)$` (case-insensitive, `.` excludes line breaks), rest trimmed:
   - contains `below\s*#?\s*N` (optionally `all ` before it; searched, not anchored): untrack every entry of this user
     whose numeric id is below N. Reply `untrack_below` with the count and N.
   - `^#?\s*N$`: untrack that id for this user. Reply `untracked`, or `untrack_missing` when nothing matched.
   - anything else after `untrack`: `slash_help`.
2. `^#?\s*N$`: track N for this user (see "Tracking").
3. Otherwise: `slash_help`. `untrack` alone is `slash_help`.

`N` is `[0-9]+`. Ids are compared as strings.

## Tracking (`trackConfession`)

- Existing entry with this id: if its `userId` differs, `not_yours` (no status revealed). Otherwise `already_tracked`
  with `{status}` = `still being tracked`, or `resolved (<status or "unknown">)`.
- New: append `{"id", "userId", "resolved": false, "date": <ISO now>}`, reply `now_tracking`, then start the status
  lookback for the id (not awaited; it runs alongside the reply, see "Status lookback").

## Status handling (`checkStatus`)

Owl's message (only from `OWL_BOT_ID` in the logs channel; `VERIFY_ID_IN_STATUS=true` also requires `#<id>` in the text).
`extractId`: first `#\s*N`, else the first run of two or more digits.

`classify`, in this order (case-insensitive substring match; `info` is the message text):

1. contains `approved` and `<#` + `CHANNEL_META` (the text, not lower-cased): **meta**
2. contains `unapproved`: **unapproved**
3. contains `unrejected`: **unrejected**
4. contains `approved`: **approved**
5. contains `rejected` or `denied`: **rejected**
6. otherwise: nothing.

Then, for a tracked entry:

- if the entry is already resolved: post a random stupid line in the logs thread (`thread_ts` = the status ts).
- **meta**: resolve `postTs` (cached, or `findPostTs`), set `resolved: true, status: "meta"`, DM `meta_approved` with
  the meta permalink link when a ts is known.
- **approved**: resolve `postTs`, set `resolved, status: "approved"`, DM `approved` with the confessions permalink.
- **unapproved**: set `status: "unapproved"`, DM `unapproved`. No history search.
- **rejected**: write `pendingReject` (below). No DM yet.
- **unrejected**: if `pendingReject` is set, cancel its timer and remove the key. Otherwise nothing.

Permalinks: `https://<WORKSPACE_DOMAIN>/archives/<channel>/p<ts without the first dot>`; the link text is
` <URL|View it here>` (leading space), empty when no ts is known.

### Rejections

- `checkStatus` writes `pendingReject: {"statusTs": <status ts>, "at": <ISO now>}` (no `statusTs` key when the ts is
  missing), then arms the timer for now + `REASON_QUIET_MS`.
- A human reply in the logs thread (`thread_ts` set, `thread_ts` ≠ `ts`, no `bot_id`) whose `thread_ts` matches a
  pending `statusTs` re-arms the timer for now + `REASON_QUIET_MS`. Anything else in the logs channel from a human is
  ignored.
- `sendRejection` (timer): `getDenialReason(statusTs)`, then sets `resolved: true, status: "rejected"` and removes
  `pendingReject`, then DMs `rejected`. Skipped when the entry is no longer pending or is resolved.
- `getDenialReason`: `conversations.replies` on the logs channel with `ts`; skip the root; keep messages with no
  `bot_id`, a `user`, and non-blank text; the reviewer is the first such message's `user`; the reason is the trimmed
  texts of that user's messages joined with `"\n> "`. Empty when none.
- The rejected DM's `{reason}` is `"\n> " + reason` (or empty) and `{link}` is ` <logs-thread-permalink|View the thread>`,
  where the permalink is `messageLink(logs, statusTs)`. A missing `statusTs` throws after the entry is resolved, so no
  DM is sent (both bots do this).

### Stupid lines

Posted to the logs channel as `{channel, text, thread_ts}` when a status arrives for a resolved entry, one picked at
random from `messages.stupid_lines` (or the built-in two when the list is missing or empty).

## Status lookback (`lookbackStatus`)

Started (not awaited) by a new track and, at start-up, for every unresolved entry with no pending rejection.

1. If `SLACK_USER_TOKEN` is set: `search.messages` with `query: "#<id>" in:#<LOGS_CHANNEL_NAME>`, `sort: "timestamp"`,
   `sort_dir: "desc"`, `count: 20`. A match must have `channel.id` = the logs channel, `bot_id` = `OWL_BOT_ID` or
   `user` = `OWL_USER_ID`, text, `extractId` = the id, and a non-`none` classification. A hit applies the status
   (only if the entry is unresolved and has no pending rejection) and the lookback ends. No hit, or any error (logged),
   falls through to step 2.
2. `conversations.history` on the logs channel, `limit: 200`, up to 5 pages (`cursor` from `response_metadata.next_cursor`).
   The first Owl message (`bot_id` = `OWL_BOT_ID`, has `ts`) whose id is the entry's and whose classification is not
   `none` is the current status. It is applied under the same conditions as step 1. The scan ends at that message.

Errors are logged (`Status lookback failed for <id>:`) and never raised.

In the track path the lookback's first Slack call is issued before the reply's. Calls of different methods can
interleave differently (see "Known differences").

## Timers

| Timer | Schedule | Behaviour |
| --- | --- | --- |
| Start-up cleanup | once, before connecting | `cleanup` (retention below). A non-list file is fatal. |
| Hourly cleanup | every 3 600 s, first at +1 h | `cleanup`. Writes only when something expired. An error is logged and the loop continues. |
| Rejection DM | `min(now + REASON_QUIET_MS, at + REASON_MAX_WAIT_MS) − now`, clamped like Node's `setTimeout` (NaN, < 1 ms or > 2^31−1 ms → 1 ms) | Re-armed by each reviewer reply and at start-up. Cancelled by `unrejected`. |
| Socket reconnect | after an error during the session or a failed connect: 10 s | A `disconnect` from Slack reconnects at once. |

Defaults: `REASON_QUIET_MS` = 90 000, `REASON_MAX_WAIT_MS` = 600 000 (a set value is `Number(value)`, so `""` is 0).
Retention: 2 days (172 800 000 ms) for resolved entries, measured from `date`.

## Slack API calls

| Method | Token | Params (in this order) | Used by | Paging |
| --- | --- | --- | --- | --- |
| `chat.postMessage` | bot | `channel` = user id, `text` | DMs: `dm_refuse`, `approved`, `meta_approved`, `unapproved`, `rejected` | none |
| `chat.postMessage` | bot | `channel` = logs, `text` = stupid line, `thread_ts` | stupid line | none |
| `conversations.history` | bot | `channel`, `limit` = 200, `cursor` (not on the first page) | `findPostTs` (#confessions, then #meta) and the lookback | 5 pages |
| `conversations.replies` | bot | `channel` = logs, `ts` | `getDenialReason` | one call |
| `search.messages` | user | `query`, `sort` = "timestamp", `sort_dir` = "desc", `count` = 20 | the lookback (when the user token is set) | one call |
| `views.publish` | bot | `user_id`, `view` | App Home | none |
| `apps.connections.open` | app | bearer app token | Socket Mode connect | none |
| response_url POST | none | `{"text": ...}` | `/mrox` replies | none |

`findPostTs` looks for a message whose text starts with `<id>:`; the first match's `ts` is the post ts (none if the
match has no `ts`). A failing channel is logged and skipped; the search then continues with the other channel.
Web API retries: up to 10, with `Retry-After` on 429 and exponential backoff on 5xx and transport errors.

## Views and blocks

`views.publish` view (`type: "home"`), in order:

1. `section`, `text` `{type: mrkdwn, text: <home.header>}`.
2. `input`: `block_id` `track_input_<ms>` (fresh each publish), `dispatch_action: true`, `label`
   `{plain_text, <home.track_label | "Track a confession">}`, `element`: `plain_text_input`, `action_id`
   `track_confession_input`, `placeholder` `{plain_text, <home.track_placeholder | "Paste a confession ID, e.g. 40289">}`,
   `dispatch_action_config` `{trigger_actions_on: ["on_enter_pressed"]}`.
3. (when there is a notice) `context`, `elements` `[{mrkdwn, <notice>}]`.
4. (when the user has unresolved entries) `divider`; `section` `*Being tracked*`; for each of the first 20 unresolved
   entries in file order: `section` `text` `{mrkdwn, "#<id>"}` with `accessory` `{type: button, text: {plain_text,
   "Untrack"}, action_id: "untrack_confession", value: "<id>"}`.
5. `divider`; `context` `[{mrkdwn, "<https://hackclub.enterprise.slack.com/docs/T0266FRGM/F0C9ZFGE6U9|Privacy policy>"}]`.

Home header default: `*Mrox* :neocat_glasses:`. The config value (`home.header`) is used as given, including non-string
JSON values.

## Message catalogue (`config.yaml`, with the built-in fallbacks)

Placeholders are replaced in one pass; unknown ones stay as written. A list entry uses its first item; a null or
missing entry renders as empty. `{link}` and `{reason}` are described above.

| Key | Config text | Placeholders | Built-in fallback |
| --- | --- | --- | --- |
| `not_allowed` | `sorgy u cant do this, dm <@OPERATOR> if this shouldnt have happened` | | `You're not allowed to use this.` |
| `slash_help` | `to track a confession use ` + "`/mrox {id}`" + ` (or paste the id into the box on my app home) and ill dm u when its approved, use ` + "`/mrox untrack {id}`" + ` to untrack somthing` | | `Welcome to Mrox! ...` (longer text in `test.ts`) |
| `dm_refuse` | `sorry! for your security I can't accept ids via dms, please use /mrox or the app home :D` | | same |
| `not_yours` | `Confession #{confessionID} is already being tracked by someone else.` | confessionID | same |
| `already_tracked` | `Confession #{confessionID} is {status}.` | confessionID, status | same |
| `now_tracking` | `Now tracking confession #{confessionID}. I'll DM you when its status changes. :neocat:` | confessionID | without `:neocat:` |
| `untracked` | `Stopped tracking confession #{confessionID}.` | confessionID | same |
| `untrack_missing` | `Confession #{confessionID} isn't being tracked.` | confessionID | same |
| `untrack_below` | `Stopped tracking {count} confession(s) below #{threshold}.` | count, threshold | same |
| `approved` | `:yay: Your confession #{confessionID} got approved for <#{channel}>!{link}` | confessionID, channel, link | same |
| `meta_approved` | `:meta: Your confession #{confessionID} got approved for <#{channel}>!{link}` | confessionID, channel, link | same |
| `unapproved` | `:uuh: Your confession #{confessionID} was unapproved and the thread has been nuked.` | confessionID | same |
| `rejected` | `:neocat_sad: Your confession #{confessionID} was denied. Reason (will be blank if none was provided): {reason}{link}` | confessionID, reason, link | `:neocat_sad: Your confession #{confessionID} was denied.{reason}{link}` |
| `stupid_lines` | five lines: "that confession doesn't exist... yet :eyes:", "whoa there, that's not supposed to happen. neat.", "you've broken reality. congrats i guess? :neocat_confused:", "this confession is in a superposition of approved and denied :hyperbongocat:", "error 404: common sense not found" | | the first two |
| `home.header` | `*Mrox* :neocat_glasses:` + newline + `Paste your confession ID below and press Enter, and I'll DM you when its status changes.` | | `*Mrox* :neocat_glasses:` |
| `home.track_label`, `home.track_placeholder` | not set in config.yaml | | see the view above |

Config failure (missing or unparsable file) warns and uses the built-in fallbacks alone.

## Start-up (`main()`)

1. Load `.env` (`dotenv`, does not override set variables) and read the settings. Missing `SLACK_BOT_TOKEN` or
   `SLACK_APP_TOKEN` exits 1 (Bolt's constructor throws for both).
2. Create `confessions.json` with `[]` if it does not exist.
3. `cleanup()`, `resumePendingRejections()`, then the status lookbacks for every unresolved entry with no pending
   rejection. Any failure before the lookbacks exits 1 (`Failed to start:`).
4. Start the hourly cleanup, connect Socket Mode (a failed first connect exits 1), and print
   `(test.ts) Bot is running...` to stdout.

## Environment variables

Read by `test.ts` (defaults in brackets), with the Rust behaviour:

| Variable | Default | Rust |
| --- | --- | --- |
| `SLACK_BOT_TOKEN` | none | same; required |
| `SLACK_APP_TOKEN` | none | same; required |
| `SLACK_USER_TOKEN` | unset | same; enables the status search |
| `ALLOWED_USER_ID` | the operator's user id (see "Known differences") | **no default**: unset refuses everyone and a warning is printed |
| `RESTRICT_TO_USER` | on; only the exact string `false` turns it off | same |
| `CHANNEL_CONFESSIONS` | `CNMU9L92Q` | same |
| `CHANNEL_CONFESSION_LOGS` | `C08R8LE6DDG` | same |
| `CHANNEL_META` | `C0188CY57PZ` | same |
| `OWL_BOT_ID` | `B0C6JPG4W0G` | same |
| `OWL_USER_ID` | `U0C5Q89C2P4` | same |
| `LOGS_CHANNEL_NAME` | `confessions-log` | same |
| `WORKSPACE_DOMAIN` | `hackclub.enterprise.slack.com` | same |
| `DATA_FILE` | `<cwd>/confessions.json` | same |
| `CONFIG_FILE` | `<cwd>/config.yaml` | same |
| `VERIFY_ID_IN_STATUS` | off; only `true` turns it on | same |
| `DEBUG` | off; only `true` turns it on | same |
| `REASON_QUIET_MS` | `90000` | same (`Number` semantics) |
| `REASON_MAX_WAIT_MS` | `600000` | same (`Number` semantics) |
| `MROX_SLACK_API_BASE` | not read | Rust only: a test hook that points the Web API at a mock (default `https://slack.com/api`) |

`.env` also has `CHANNEL_TO_USE`, which `test.ts` never reads (only the dead `main.ts` did). `settings.json` is `{}`
and is not read by either bot.

## App manifest

`app_manifest.yaml` (not code, nothing to port) adds the bot scopes `mpim:history` and `mpim:read`, and the user scope
`search:read`. The user scope is what `SLACK_USER_TOKEN` needs for the status search; without that token the search is
skipped, as in `test.ts`.

## State file: `confessions.json`

A JSON array, written with `JSON.stringify(value, null, 2)` (two-space indent, no trailing newline), via
`<file>.<pid>.tmp` and a rename. Entries:

- `id` (string), `userId` (string), `resolved` (bool), `date` (ISO 8601, UTC, milliseconds)
- `postTs` (string, once resolved through `findPostTs`), `status` (string, `meta` | `approved` | `unapproved` |
  `rejected`)
- `pendingReject` (`{statusTs?, at}`) while a rejection waits; removed when it resolves or is unrejected

Key order follows JavaScript's object assignment: a key that is added later goes last, and a key removed by
`undefined` disappears from the file (the Rust `Store::update` treats `None` the same way). The Rust writer produces
the same bytes as Node for the cases in the fixture.

Reads: a missing, unreadable or unparsable file reads as `[]`. A file that parses to something other than an array
makes every handler fail without writing (both bots), and start-up exits 1.

## Logs

stdout: `(test.ts) Bot is running...`, and with `DEBUG=true`: `[msg] channel=… bot=… text="…"`,
`Ignoring non-Owl message in logs (bot=…)`, `No tracked confession for id …`, `DM sent to …: <ts>`,
`Status doesn't reference confession …, skipping`.

stderr: `Failed to DM …`, `History scan failed for …`, `Could not fetch denial reason`, `Status search failed for …`,
`Status lookback failed for …`, `Failed to publish home view`, `Error handling message event`, `Failed to start`, and
the unhandled-rejection / uncaught-exception lines. The wording is the same; the Node error dumps are not.

## Known differences

1. **`ALLOWED_USER_ID` has no built-in default in Rust.** `test.ts` falls back to the operator's user id. The port
   does not carry that id; with the variable unset it refuses `/mrox` for everyone and prints a warning. Set
   `ALLOWED_USER_ID` in `.env` before switching over. This is the one behaviour difference that depends on the
   environment, not on the code.
2. **Concurrent calls.** The TypeScript starts the lookback without awaiting it, so its calls can interleave with the
   reply's and with other work in a different order. The Rust bot polls the lookback first and runs both inside one
   task. The fixture compares the order of calls within each method, not across methods. Slack gives no ordering
   guarantee across concurrent requests, so the observable effects are the same.
3. **Wire encoding.** `@slack/web-api` form-encodes most method bodies; the Rust client sends JSON. Slack accepts both
   for these methods. Confirm on the first deploy.
4. **Retry details.** The retry count (10) and the 429 handling match. The backoff curve is an approximation of
   `@slack/web-api`'s.
5. **Socket Mode reconnect pacing.** Bolt's own reconnect backoff is not copied: failures wait a fixed 10 s, and a
   Slack `disconnect` reconnects at once (as Bolt does).
6. **Library logs.** Bolt's and `@slack/*` internal log lines are not reproduced.
7. **YAML.** `yaml-rust2` is used instead of `js-yaml`. The config in the repo parses to the same values (checked by
   the tests). Differences would only show for YAML features the config does not use: unquoted dates (js-yaml makes
   a `Date`), merge keys, and some non-core scalars.
8. **`.env` parsing.** `dotenvy` is used instead of `dotenv`. Plain `KEY=value` lines, comments and quoting match;
   exotic quoting or escapes may not.
9. **Date parsing.** `Date.parse` is replicated for ISO 8601 forms (what the bot writes). Non-ISO legacy forms are not.
10. **Numbers in `confessions.json`.** The bot never writes numbers, but a hand-edited number may be re-printed
    differently (for example `1.0`). String and structure round-trip exactly.
11. **Template placeholders named like `Object.prototype` members** (`{constructor}` and the like) are not
    special-cased; no config uses them.
12. **Errors inside a listener** are logged in the same place and ordering, but with different wording.
13. **Date and clock.** The clock is the system clock (milliseconds). Node and Rust both use it for `Date.now()` and
    `toISOString()`.

## Measurements

Release build (`opt-level = "s"`, LTO, one codegen unit, stripped): binary 2.6 MB.

Idle RSS: 7 696 KiB (about 7.5 MiB) ten seconds after start, with the bot connected to a local mock of Slack
(Socket Mode `hello`, no events) and outbound HTTPS to Slack blocked by a `sandbox-exec` profile. The process made no
connections outside loopback. The TypeScript bot was not measured under the same mock: Bolt's Slack base URL cannot be
overridden.
