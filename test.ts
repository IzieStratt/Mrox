import 'dotenv/config';
import { App } from '@slack/bolt';
import { WebClient } from '@slack/web-api';
import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';

const web = new WebClient(process.env.SLACK_BOT_TOKEN);

// ---------------------------------------------------------------------------
// Configuration (env overrides take priority, then these defaults)
// ---------------------------------------------------------------------------
const RESTRICT_TO_USER = process.env.RESTRICT_TO_USER !== "false"; // test guard
const ALLOWED_USER_ID = process.env.ALLOWED_USER_ID ?? "U09KKMHLS15";

const Channel_Confessions = process.env.CHANNEL_CONFESSIONS ?? "CNMU9L92Q";
const Channel_Confession_Logs =
  process.env.CHANNEL_CONFESSION_LOGS ?? "C08R8LE6DDG";
const Channel_Meta = process.env.CHANNEL_META ?? "C0188CY57PZ";

// Prox2 is now "Owl" (app_id A0C5L27PB9P, user_id U0C5Q89C2P4)
const OWL_BOT_ID = process.env.OWL_BOT_ID ?? "B0C6JPG4W0G";

const WORKSPACE_DOMAIN =
  process.env.WORKSPACE_DOMAIN ?? "hackclub.enterprise.slack.com";

const PRIVACY_POLICY_URL =
  "https://hackclub.enterprise.slack.com/docs/T0266FRGM/F0C9ZFGE6U9";

const DATA_FILE = process.env.DATA_FILE ?? path.join(process.cwd(), "confessions.json");
const SETTINGS_FILE = process.env.SETTINGS_FILE ?? path.join(process.cwd(), "settings.json");
const CONFIG_FILE = process.env.CONFIG_FILE ?? path.join(process.cwd(), "config.yaml");

const RETENTION_MS = 2 * 24 * 60 * 60 * 1000; // keep resolved confessions 2 days
const HISTORY_SCAN_LIMIT = 5; // pages of 200 when searching for a confession post
const DEBUG = process.env.DEBUG === "true";

// Require the status message to actually reference the confession id before acting
const VERIFY_ID_IN_STATUS = process.env.VERIFY_ID_IN_STATUS === "true";

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------
const app = new App({
  token: process.env.SLACK_BOT_TOKEN,
  appToken: process.env.SLACK_APP_TOKEN,
  socketMode: true,
});

// ---------------------------------------------------------------------------
// Small utilities
// ---------------------------------------------------------------------------
function log(...args: unknown[]) {
  if (DEBUG) console.log(...args);
}

function warn(...args: unknown[]) {
  console.warn(...args);
}

function err(...args: unknown[]) {
  console.error(...args);
}

// Atomic JSON write: write to a temp file then rename so a crash can't corrupt data
function writeJsonAtomic(file: string, value: unknown) {
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(value, null, 2));
  fs.renameSync(tmp, file);
}

function readJson<T>(file: string, fallback: T): T {
  try {
    return JSON.parse(fs.readFileSync(file, "utf-8")) as T;
  } catch {
    return fallback;
  }
}

function ensureFiles() {
  if (!fs.existsSync(DATA_FILE)) writeJsonAtomic(DATA_FILE, []);
  if (!fs.existsSync(SETTINGS_FILE)) writeJsonAtomic(SETTINGS_FILE, {});
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------
interface Confession {
  id: string;
  userId: string;
  resolved: boolean;
  date: string;
  postTs?: string; // ts of the confession message (for links / thread ops)
  status?: string; // last known status keyword
}

function readConfessions(): Confession[] {
  return readJson<Confession[]>(DATA_FILE, []);
}

function writeConfessions(data: Confession[]) {
  writeJsonAtomic(DATA_FILE, data);
}

function findConfession(id: string): Confession | undefined {
  return readConfessions().find((c) => c.id === id);
}

function updateConfession(id: string, patch: Partial<Confession>) {
  const data = readConfessions();
  const found = data.find((c) => c.id === id);
  if (!found) return undefined;
  Object.assign(found, patch);
  writeConfessions(data);
  return found;
}

function setResolved(id: string, status?: string) {
  updateConfession(id, { resolved: true, status });
}

function statusWord(post: Confession): string {
  return post.resolved ? `resolved (${post.status ?? "unknown"})` : "still being tracked";
}

// Track a confession for a user. Returns the reply message.
function trackConfession(id: string, userId: string): string {
  const existing = findConfession(id);
  if (existing) {
    // Never reveal someone else's confession status
    if (existing.userId !== userId) return t("not_yours", { confessionID: id });
    return t("already_tracked", { confessionID: id, status: statusWord(existing) });
  }
  const data = readConfessions();
  data.push({ id, userId, resolved: false, date: new Date().toISOString() });
  writeConfessions(data);
  return t("now_tracking", { confessionID: id });
}

// Stop tracking one of the user's confessions. Returns true if something was removed.
function untrackConfession(id: string, userId: string): boolean {
  const data = readConfessions();
  const kept = data.filter((c) => !(c.id === id && c.userId === userId));
  if (kept.length === data.length) return false;
  writeConfessions(kept);
  return true;
}

// Stop tracking every one of the user's confessions with a numeric id below n.
function untrackBelow(n: number, userId: string): number {
  const data = readConfessions();
  const kept = data.filter((c) => {
    const num = Number(c.id);
    return !(c.userId === userId && Number.isFinite(num) && num < n);
  });
  const removed = data.length - kept.length;
  if (removed > 0) writeConfessions(kept);
  return removed;
}

function trackedBy(userId: string): Confession[] {
  return readConfessions().filter((c) => c.userId === userId && !c.resolved);
}

// Run a "/mrox"-style input (id or untrack ...) for a user and return the reply
function handleInput(input: string, userId: string): string {
  const un = parseUntrack(input);
  if (un) {
    if (un.below !== undefined) {
      return t("untrack_below", {
        count: untrackBelow(un.below, userId),
        threshold: un.below,
      });
    }
    if (un.id) {
      return untrackConfession(un.id, userId)
        ? t("untracked", { confessionID: un.id })
        : t("untrack_missing", { confessionID: un.id });
    }
    return t("slash_help");
  }
  const id = input.trim().match(/^#?\s*(\d+)$/)?.[1];
  return id ? trackConfession(id, userId) : t("slash_help");
}

function isAllowed(userId: string): boolean {
  return !RESTRICT_TO_USER || userId === ALLOWED_USER_ID;
}

// Parse "untrack 40289" / "untrack #40289" / "untrack below 40763" / "untrack all below 40763"
function parseUntrack(text: string): { id?: string; below?: number } | undefined {
  const m = text.trim().match(/^untrack\s+(.+)$/i);
  if (!m) return undefined;
  const rest = m[1].trim();
  const below = rest.match(/(?:all\s+)?below\s*#?\s*(\d+)/i);
  if (below) return { below: Number(below[1]) };
  const id = rest.match(/^#?\s*(\d+)$/);
  if (id) return { id: id[1] };
  return {};
}

// Delete resolved entries older than the retention window
function cleanup(oldMs: number = RETENTION_MS) {
  const now = Date.now();
  const data = readConfessions().filter((c) => {
    if (c.resolved && now - new Date(c.date).getTime() > oldMs) return false;
    return true;
  });
  writeConfessions(data);
}

// ---------------------------------------------------------------------------
// Per-user settings
// ---------------------------------------------------------------------------
interface Settings {
  [userId: string]: {
    subscribeThread: boolean; // ping + delete to follow the thread
  };
}

function readSettings(): Settings {
  return readJson<Settings>(SETTINGS_FILE, {});
}

function writeSettings(data: Settings) {
  writeJsonAtomic(SETTINGS_FILE, data);
}

function getSubscribeThread(userId: string): boolean {
  return readSettings()[userId]?.subscribeThread ?? false; // default false
}

function setSubscribeThread(userId: string, value: boolean) {
  const s = readSettings();
  s[userId] = { ...(s[userId] ?? {}), subscribeThread: value };
  writeSettings(s);
}

// ---------------------------------------------------------------------------
// Message config (config.yaml) + templating
// ---------------------------------------------------------------------------
interface BotConfig {
  messages: Record<string, string | string[]>;
  home: Record<string, string>;
}

const STUPID_FALLBACK = [
  "that confession doesn't exist... yet :eyes:",
  "whoa there, that's not supposed to happen. neat.",
];

const FALLBACK_MESSAGES: Record<string, string | string[]> = {
  not_allowed: "You're not allowed to use this.",
  slash_help: "Welcome to Mrox! To start tracking a confession please run: `/mrox {id}`, or paste it into the box on my App Home.",
  dm_refuse: "sorry! for your security I can't accept ids via dms, please use /mrox or the app home :D",
  not_yours: "Confession #{confessionID} is already being tracked by someone else.",
  already_tracked: "Confession #{confessionID} is {status}.",
  now_tracking: "Now tracking confession #{confessionID}. I'll DM you when its status changes.",
  untracked: "Stopped tracking confession #{confessionID}.",
  untrack_missing: "Confession #{confessionID} isn't being tracked.",
  untrack_below: "Stopped tracking {count} confession(s) below #{threshold}.",
  approved: ":yay: Your confession #{confessionID} got approved for <#{channel}>!{link}",
  meta_approved: ":meta: Your confession #{confessionID} got approved for <#{channel}>!{link}",
  unapproved: ":uuh: Your confession #{confessionID} was unapproved and the thread has been nuked.",
  rejected: ":neocat_sad: Your confession #{confessionID} was denied.{reason}",
  stupid_lines: [...STUPID_FALLBACK],
};

const FALLBACK_HOME: Record<string, string> = {
  header: "*Mrox* :neocat_glasses:",
  follow_title: "*Auto-follow your confession threads*",
  follow_body: "When your post gets approved, you get subscribed to its thread.",
  follow_on: "Currently *ON*.",
  follow_off: "Currently *OFF*.",
};

function loadConfig(): BotConfig {
  try {
    const raw = yaml.load(fs.readFileSync(CONFIG_FILE, "utf-8")) as Partial<BotConfig>;
    return {
      messages: { ...FALLBACK_MESSAGES, ...(raw.messages ?? {}) },
      home: { ...FALLBACK_HOME, ...(raw.home ?? {}) },
    };
  } catch (error) {
    warn(`Could not load ${CONFIG_FILE}, using built-in messages:`, error);
    return { messages: { ...FALLBACK_MESSAGES }, home: { ...FALLBACK_HOME } };
  }
}

const CONFIG = loadConfig();

// Render a template from config.yaml, filling {placeholders}
function t(key: string, vars?: Record<string, string | number>): string {
  const raw = CONFIG.messages[key];
  const template = Array.isArray(raw) ? raw[0] ?? "" : raw ?? "";
  return template.replace(/\{(\w+)\}/g, (_m: string, name: string) =>
    vars && name in vars ? String(vars[name]) : `{${name}}`
  );
}

function stupidLines(): string[] {
  const raw = CONFIG.messages["stupid_lines"];
  return Array.isArray(raw) && raw.length > 0 ? raw : [...STUPID_FALLBACK];
}

// ---------------------------------------------------------------------------
// Slack helpers
// ---------------------------------------------------------------------------
function messageLink(channel: string, ts: string): string {
  return `https://${WORKSPACE_DOMAIN}/archives/${channel}/p${ts.replace(".", "")}`;
}

async function dm(userId: string, text: string) {
  try {
    const res = await web.chat.postMessage({ channel: userId, text });
    log(`DM sent to ${userId}: ${res.ts}`);
    return res;
  } catch (error) {
    err(`Failed to DM ${userId}:`, error);
    return undefined;
  }
}

// Extract the first numeric id out of arbitrary text
function extractId(text: string): string | undefined {
  const m = text.match(/#\s*(\d+)/) ?? text.match(/(\d{2,})/);
  return m?.[1];
}

// Search #confessions then #meta for the message containing `#<id>`
async function findPostTs(id: string): Promise<string | undefined> {
  for (const channel of [Channel_Confessions, Channel_Meta]) {
    try {
      let cursor: string | undefined;
      for (let page = 0; page < HISTORY_SCAN_LIMIT; page++) {
        const res = await web.conversations.history({
          channel,
          limit: 200,
          cursor,
        });
        for (const m of res.messages ?? []) {
          if (typeof m.text === "string" && m.text.includes(`#${id}`)) {
            return m.ts ?? undefined;
          }
        }
        cursor = res.response_metadata?.next_cursor;
        if (!cursor) break;
      }
    } catch (error) {
      warn(`History scan failed for ${channel}:`, error);
    }
  }
  return undefined;
}

// Resolve (and cache) the confession's message ts so links/threads work
async function resolvePostTs(post: Confession): Promise<string | undefined> {
  if (post.postTs) return post.postTs;
  const ts = await findPostTs(post.id);
  if (ts) updateConfession(post.id, { postTs: ts });
  return ts;
}

// Ping the user in a thread then instantly delete it to subscribe them silently
async function subscribeToThread(post: Confession) {
  const ts = await resolvePostTs(post);
  if (!ts) {
    log(`No postTs for ${post.id}; cannot subscribe to thread`);
    return;
  }
  try {
    const msg = await web.chat.postMessage({
      channel: Channel_Confessions,
      thread_ts: ts,
      text: `<@${post.userId}>`,
    });
    if (msg.ts) {
      await web.chat.delete({ channel: Channel_Confessions, ts: msg.ts });
      log(`Subscribed ${post.userId} to thread of confession ${post.id}`);
    }
  } catch (error) {
    warn(`Thread subscribe failed for ${post.id}:`, error);
  }
}

// Pull the denial reason: the first non-bot reply in the status message's thread
async function getDenialReason(statusTs: string): Promise<string | undefined> {
  try {
    const res = await web.conversations.replies({
      channel: Channel_Confession_Logs,
      ts: statusTs,
    });
    const reply = (res.messages ?? [])
      .slice(1) // skip the root status message
      .find((m) => !m.bot_id && typeof m.text === "string" && m.text.trim());
    return reply?.text?.trim();
  } catch (error) {
    warn(`Could not fetch denial reason:`, error);
    return undefined;
  }
}

// ---------------------------------------------------------------------------
// Easter egg: something that "shouldn't be possible"
// ---------------------------------------------------------------------------
async function stupidThing(channel: string, threadTs?: string) {
  const lines = stupidLines();
  const line = lines[Math.floor(Math.random() * lines.length)];
  try {
    await web.chat.postMessage({
      channel,
      text: line,
      thread_ts: threadTs,
    });
  } catch (error) {
    warn("Failed to do the stupid thing:", error);
  }
}

// ---------------------------------------------------------------------------
// Status handling
// ---------------------------------------------------------------------------
interface StatusResult {
  kind: "meta" | "approved" | "rejected" | "unapproved" | "none";
}

function classify(information: string): StatusResult {
  const lower = information.toLowerCase();
  if (lower.includes("approved for meta")) return { kind: "meta" };
  if (lower.includes("unapproved")) return { kind: "unapproved" };
  if (lower.includes("approved")) return { kind: "approved" };
  if (lower.includes("rejected") || lower.includes("denied"))
    return { kind: "rejected" };
  return { kind: "none" };
}

async function checkStatus(information: string, post: Confession, statusTs: string) {
  // Optionally require the message to reference the confession id
  if (VERIFY_ID_IN_STATUS && !information.includes(`#${post.id}`)) {
    log(`Status doesn't reference confession ${post.id}, skipping`);
    return;
  }

  const { kind } = classify(information);
  if (kind === "none") return;

  // Easter egg: a status arriving for something already resolved shouldn't happen
  if (post.resolved) {
    await stupidThing(Channel_Confession_Logs, statusTs);
    return;
  }

  if (kind === "meta") {
    const ts = await resolvePostTs(post);
    setResolved(post.id, "meta");
    const link = ts ? ` <${messageLink(Channel_Meta, ts)}|View it here>` : "";
    await dm(
      post.userId,
      t("meta_approved", { confessionID: post.id, channel: Channel_Meta, link })
    );
  } else if (kind === "unapproved") {
    setResolved(post.id, "unapproved");
    await dm(post.userId, t("unapproved", { confessionID: post.id }));
  } else if (kind === "approved") {
    const ts = await resolvePostTs(post);
    setResolved(post.id, "approved");
    const link = ts
      ? ` <${messageLink(Channel_Confessions, ts)}|View it here>`
      : "";
    await dm(
      post.userId,
      t("approved", { confessionID: post.id, channel: Channel_Confessions, link })
    );
    if (getSubscribeThread(post.userId)) {
      await subscribeToThread({ ...post, postTs: ts });
    }
  } else if (kind === "rejected") {
    const reason = await getDenialReason(statusTs);
    setResolved(post.id, "rejected");
    await dm(
      post.userId,
      t("rejected", {
        confessionID: post.id,
        reason: reason ? `\n> Reason: ${reason}` : "",
      })
    );
  }
}

// ---------------------------------------------------------------------------
// Slash command: /mrox {id}
// ---------------------------------------------------------------------------
app.command("/mrox", async ({ command, ack, respond }) => {
  await ack();

  if (!isAllowed(command.user_id)) {
    await respond(t("not_allowed"));
    return;
  }

  await respond(handleInput(command.text ?? "", command.user_id));
});

// ---------------------------------------------------------------------------
// Message listener: react to status updates in the confession logs channel
// ---------------------------------------------------------------------------
app.event("message", async ({ event, logger }) => {
  try {
    // Handle edits by unwrapping the changed message
    let raw: any = event;
    if (event.subtype === "message_changed") {
      raw = (event as any).message ?? event;
    } else if (event.subtype && event.subtype !== "bot_message") {
      return; // ignore joins/leaves/deletes etc, but keep bot messages
    }

    const text: string = typeof raw.text === "string" ? raw.text : "";
    // message_changed nests the message, so the channel lives on the outer event
    const channel: string | undefined = (event as any).channel ?? raw.channel;
    const botId: string | undefined = raw.bot_id;

    if (channel === Channel_Confession_Logs) {
      log(`[msg] channel=${channel} bot=${botId} text="${text}"`);
    }

    // DMs: never accept ids here (they'd sit in DM history); point to /mrox or App Home.
    // The incoming text is deliberately not logged or stored.
    const isDM =
      (raw as any).channel_type === "im" ||
      (typeof channel === "string" && channel.startsWith("D"));
    if (isDM) {
      if (botId || raw.subtype === "bot_message" || !raw.user) return;
      await dm(raw.user as string, t("dm_refuse"));
      return;
    }

    if (!channel || channel !== Channel_Confession_Logs) return;
    if (!text) return;
    // Only trust status messages from Owl, not anyone who can type in the channel
    if (botId !== OWL_BOT_ID) {
      log(`Ignoring non-Owl message in logs (bot=${botId})`);
      return;
    }

    const id = extractId(text);
    if (!id) return;

    const post = findConfession(id);
    if (!post) {
      log(`No tracked confession for id ${id}`);
      return;
    }

    await checkStatus(text, post, raw.ts);
  } catch (error) {
    logger.error("Error handling message event:", error as Error);
  }
});

// ---------------------------------------------------------------------------
// App Home: thread-subscribe toggle
// ---------------------------------------------------------------------------
function homeView(userId: string, notice?: string) {
  const subscribe = getSubscribeThread(userId);
  const tracked = trackedBy(userId);
  const blocks: any[] = [
    {
      type: "section",
      text: {
        type: "mrkdwn",
        text: CONFIG.home["header"] ?? "*Mrox* :neocat_glasses:",
      },
    },
    {
      // dispatch_action fires on Enter, so no command is needed
      type: "input",
      block_id: `track_input_${Date.now()}`, // fresh id each publish clears the box
      dispatch_action: true,
      label: { type: "plain_text", text: CONFIG.home["track_label"] ?? "Track a confession" },
      element: {
        type: "plain_text_input",
        action_id: "track_confession_input",
        placeholder: {
          type: "plain_text",
          text: CONFIG.home["track_placeholder"] ?? "Paste a confession ID, e.g. 40289",
        },
        dispatch_action_config: { trigger_actions_on: ["on_enter_pressed"] },
      },
    },
  ];
  if (notice) {
    blocks.push({ type: "context", elements: [{ type: "mrkdwn", text: notice }] });
  }
  if (tracked.length > 0) {
    blocks.push({ type: "divider" });
    blocks.push({
      type: "section",
      text: { type: "mrkdwn", text: "*Being tracked*" },
    });
    // Home views allow 100 blocks; cap the list well under that
    for (const c of tracked.slice(0, 20)) {
      blocks.push({
        type: "section",
        text: { type: "mrkdwn", text: `#${c.id}` },
        accessory: {
          type: "button",
          text: { type: "plain_text", text: "Untrack" },
          action_id: "untrack_confession",
          value: c.id,
        },
      });
    }
  }
  blocks.push(
    { type: "divider" },
    {
      type: "section",
      text: {
        type: "mrkdwn",
        text: `${CONFIG.home["follow_title"] ?? ""}\n${CONFIG.home["follow_body"] ?? ""}`,
      },
      accessory: {
        type: "button",
        text: { type: "plain_text", text: subscribe ? "ON" : "OFF", emoji: true },
        action_id: "toggle_thread_subscribe",
        style: subscribe ? "primary" : "danger",
      },
    },
    {
      type: "context",
      elements: [
        {
          type: "mrkdwn",
          text: subscribe
            ? CONFIG.home["follow_on"] ?? "Currently *ON*."
            : CONFIG.home["follow_off"] ?? "Currently *OFF*.",
        },
      ],
    },
    { type: "divider" },
    {
      type: "context",
      elements: [{ type: "mrkdwn", text: `<${PRIVACY_POLICY_URL}|Privacy policy>` }],
    }
  );
  return { type: "home" as const, blocks };
}

app.event("app_home_opened", async ({ event, client }) => {
  const userId = event.user;
  if (!userId) return;
  try {
    await client.views.publish({ user_id: userId, view: homeView(userId) });
  } catch (error) {
    err("Failed to publish home view:", error);
  }
});

app.action("toggle_thread_subscribe", async ({ ack, body, client }) => {
  await ack();
  const userId = (body as { user: { id: string } }).user.id;
  const newValue = !getSubscribeThread(userId);
  setSubscribeThread(userId, newValue);
  await client.views.publish({ user_id: userId, view: homeView(userId) });
  log(`Set thread subscribe=${newValue} for ${userId}`);
});

// Type/paste a confession id into the App Home box (also accepts "untrack ...")
app.action("track_confession_input", async ({ ack, body, action, client }) => {
  await ack();
  const userId = (body as { user: { id: string } }).user.id;
  const value = (action as { value?: string }).value?.trim() ?? "";
  if (!value) return;
  const notice = isAllowed(userId) ? handleInput(value, userId) : t("not_allowed");
  await client.views.publish({ user_id: userId, view: homeView(userId, notice) });
});

app.action("untrack_confession", async ({ ack, body, action, client }) => {
  await ack();
  const userId = (body as { user: { id: string } }).user.id;
  const id = (action as { value?: string }).value ?? "";
  const notice = isAllowed(userId)
    ? handleInput(`untrack ${id}`, userId)
    : t("not_allowed");
  await client.views.publish({ user_id: userId, view: homeView(userId, notice) });
});

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------
process.on("unhandledRejection", (reason) => err("Unhandled rejection:", reason));
process.on("uncaughtException", (error) => err("Uncaught exception:", error));

async function main() {
  ensureFiles();
  cleanup();
  setInterval(() => cleanup(), 60 * 60 * 1000); // hourly cleanup
  await app.start();
  console.log("(test.ts) Bot is running...");
}

main().catch((error) => {
  err("Failed to start:", error);
  process.exit(1);
});
