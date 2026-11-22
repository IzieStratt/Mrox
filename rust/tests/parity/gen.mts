// Parity fixture generator. Run from the repo root:
//   node node_modules/tsx/dist/cli.mjs rust/tests/parity/gen.mts
//
// Runs the real test.ts under Node with Bolt and WebClient stubbed out. Listeners are captured, every Web API call
// (bot and user token) and response_url post is recorded, and the data file is read back after each step. Time is
// virtual: `Date` and `setTimeout` are replaced, and the "advance" step fires the timers that fall due, so the
// 90 s quiet period and the 10 min cap run instantly. Each scenario runs in its own process, so env-dependent
// constants (config.yaml, REASON_*, RESTRICT_TO_USER...) are read fresh. The results go to
// rust/tests/parity/scenarios.json, which src/bot/parity.rs replays and compares.
//
// Compared per step: the calls grouped by method (each method's order kept, methods sorted, so work the TS starts
// concurrently may interleave differently), the response_url posts, and the data file's exact bytes. Normalised on
// both sides: ISO timestamps, the Date.now() block id, and the random stupid lines.
import { mkdtempSync, writeFileSync, readFileSync, symlinkSync, mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..', '..');
const RESPOND_URL = 'https://hooks.example/resp';
const LOGS = 'C08R8LE6DDG';
const OWL = 'B0C6JPG4W0G';
const OWL_USER = 'U0C5Q89C2P4';
const ALLOWED = 'U00000TEST';
const STUPID = [
  "that confession doesn't exist... yet :eyes:",
  "whoa there, that's not supposed to happen. neat.",
  "you've broken reality. congrats i guess? :neocat_confused:",
  'this confession is in a superposition of approved and denied :hyperbongocat:',
  'error 404: common sense not found',
];
const TS0 = '1700000500.000200'; // the default status ts of owl() messages

// ---- scenario helpers -------------------------------------------------------------------------------------------
const cmd = (text: string, user: string) => ({ kind: 'command', payload: { command: '/mrox', text, user_id: user, response_url: RESPOND_URL } });
const msg = (fields: Record<string, unknown>) => ({ kind: 'message', event: { type: 'message', ts: TS0, ...fields } });
const owl = (text: string, ts = TS0) => msg({ channel: LOGS, bot_id: OWL, text, ts });
const reply = (thread: string, user: string, text: string, ts: string) => msg({ channel: LOGS, user, thread_ts: thread, ts, text });
const home = (user: string) => ({ kind: 'home', event: { type: 'app_home_opened', user } });
const track = (value: string, user: string) => ({ kind: 'track', body: { user: { id: user } }, action: { action_id: 'track_confession_input', value } });
const untrack = (value: string, user: string) => ({ kind: 'untrack', body: { user: { id: user } }, action: { action_id: 'untrack_confession', value } });
const cleanup = () => ({ kind: 'cleanup' });
const advance = (ms: number) => ({ kind: 'advance', ms });
const resume = () => ({ kind: 'resume' });
const entry = (id: string, userId: string, resolved = false, extra: Record<string, unknown> = {}) => ({ id, userId, resolved, date: new Date().toISOString(), ...extra });
const data = (...entries: unknown[]) => JSON.stringify(entries);
const page = (messages: unknown[], next = '') => ({ ok: true, messages, response_metadata: { next_cursor: next } });
const ERR = (code: string) => ({ __error: code });
const SIX = 90_000;

interface Scenario {
  name: string;
  restrict?: boolean;
  verify?: boolean;
  userToken?: boolean;
  data?: string;
  replies?: Record<string, unknown[]>;
  steps: unknown[];
}

const SCENARIOS: Scenario[] = [
  {
    name: 'track, approve with link, re-track',
    data: '[]',
    replies: { 'conversations.history': [page([{ text: '40101: other', ts: '1.1' }, { text: '40102: hi', ts: '1700000000.000100' }])] },
    steps: [cmd('40102', ALLOWED), owl('Confession #40102 was approved to <#CNMU9L92Q>'), cmd('#40102', ALLOWED)],
  },
  {
    name: 'meta approval: confessions page, then meta page',
    data: data(entry('40103', 'U1')),
    replies: { 'conversations.history': [page([], ''), page([{ text: '40103: meta post', ts: '1700.5' }])] },
    steps: [owl('40103 approved to <#C0188CY57PZ>')],
  },
  {
    name: 'denial waits for the reviewer, then quotes only the reviewer',
    data: data(entry('40104', 'U1')),
    replies: {
      'conversations.replies': [
        page([{ text: '40104 denied', ts: TS0 }, { text: 'bot note', bot_id: 'B9' }, { text: '   ', user: 'U_H' }, { text: '  too spicy  ', user: 'U_H' }]),
      ],
    },
    steps: [owl('40104 denied'), advance(SIX)],
  },
  {
    name: 'unapproved, denial without a reason',
    data: data(entry('40105', 'U1'), entry('40106', 'U1')),
    steps: [owl('40105 unapproved'), owl('40106 rejected'), advance(SIX)],
  },
  {
    name: 'status for a resolved confession gets a stupid line',
    data: data(entry('40107', 'U1', true, { status: 'approved' })),
    steps: [owl('40107 approved to <#CNMU9L92Q>')],
  },
  {
    name: 'untrusted senders, DMs and other channels',
    data: data(entry('40108', 'U1')),
    steps: [
      msg({ channel: LOGS, user: 'U_H', text: '40108 unapproved' }),
      msg({ channel: LOGS, bot_id: 'B_OTHER', text: '40108 unapproved' }),
      msg({ channel: 'D1', channel_type: 'im', user: 'U5', text: '40108' }),
      msg({ channel: 'D1', bot_id: 'B1', text: 'x' }),
      msg({ channel: 'C_OTHER', bot_id: OWL, text: '40108 unapproved' }),
    ],
  },
  {
    name: 'app home, track box and untrack buttons',
    data: data(
      ...Array.from({ length: 25 }, (_, i) => entry(String(40000 + i), ALLOWED)),
      entry('1', ALLOWED, true, { status: 'meta' }),
      entry('2', 'U_OTHER'),
    ),
    steps: [home(ALLOWED), track('  40102 ', ALLOWED), track('   ', ALLOWED), untrack('40109', ALLOWED), track('untrack below 40010', ALLOWED), home(ALLOWED)],
  },
  {
    name: 'restricted user: every entry point refuses',
    data: '[]',
    steps: [cmd('1', 'U_X'), track('1', 'U_X'), untrack('1', 'U_X'), home('U_X')],
  },
  {
    name: 'help text and untrack commands',
    data: data(entry('40102', ALLOWED), entry('40110', ALLOWED)),
    steps: [cmd('', ALLOWED), cmd('abc', ALLOWED), cmd('untrack', ALLOWED), cmd('untrack 40102', ALLOWED), cmd('untrack 40102', ALLOWED), cmd('untrack below 50000', ALLOWED), cmd('UNTRACK #3', ALLOWED)],
  },
  {
    name: 'five history pages per channel, then no link',
    data: data(entry('40111', 'U1')),
    replies: { 'conversations.history': Array.from({ length: 12 }, () => page([], 'more')) },
    steps: [owl('40111 approved to <#CNMU9L92Q>')],
  },
  {
    name: 'a failing history channel is skipped',
    data: data(entry('40112', 'U1')),
    replies: { 'conversations.history': [ERR('channel_not_found'), page([{ text: '40112: meta', ts: '1.2' }])] },
    steps: [owl('40112 approved to <#CNMU9L92Q>')],
  },
  {
    name: 'VERIFY_ID_IN_STATUS requires the #id',
    verify: true,
    data: data(entry('40113', 'U1')),
    steps: [owl('40113 unapproved'), owl('#40113 unapproved')],
  },
  {
    name: 'cached postTs is used without a search',
    data: data(entry('40114', 'U1', false, { postTs: '1700.25' })),
    steps: [owl('40114 approved to <#CNMU9L92Q>')],
  },
  {
    name: 'a failed DM is logged and the status is still saved',
    data: data(entry('40115', 'U1')),
    replies: { 'chat.postMessage': [ERR('user_not_found')] },
    steps: [owl('40115 unapproved')],
  },
  {
    name: 'message_changed reads the inner message',
    data: data(entry('40116', 'U1')),
    steps: [msg({ subtype: 'message_changed', channel: LOGS, ts: '1700.9', message: { text: '40116 unapproved', bot_id: OWL, ts: '1700.1' } })],
  },
  {
    name: 'corrupt data file reads as empty and is rewritten',
    data: '{not json',
    steps: [cmd('40102', ALLOWED), owl('40102 unapproved')],
  },
  {
    name: 'data file that is not a list makes every handler fail quietly',
    data: '{}',
    steps: [cmd('40102', ALLOWED), home(ALLOWED), track('40102', ALLOWED), owl('40102 unapproved'), cleanup(), resume(), advance(SIX)],
  },
  {
    name: 'cleanup removes only expired resolved entries',
    data: data(entry('1', 'U', true, { date: '2020-01-01T00:00:00.000Z' }), entry('2', 'U', true), entry('3', 'U', false, { date: '2020-01-01T00:00:00.000Z' })),
    steps: [cleanup()],
  },
  {
    name: 'a failed home publish is logged',
    data: '[]',
    replies: { 'views.publish': [ERR('invalid_arguments')] },
    steps: [home(ALLOWED), home(ALLOWED)],
  },
  {
    name: 'another user cannot see or untrack my entry',
    restrict: false,
    data: data(entry('8', 'U1')),
    steps: [cmd('8', 'U2'), untrack('8', 'U2'), cmd('untrack 8', 'U1'), cmd('8', 'U1')],
  },
  {
    name: 'reviewer replies push the DM back by the quiet period',
    data: data(entry('40117', 'U1')),
    replies: {
      'conversations.replies': [page([{ text: '40117 denied', ts: TS0 }, { text: 'too spicy', user: 'U_REV' }, { text: 'and cringe', user: 'U_REV' }, { text: 'lol', user: 'U_OTHER' }])],
    },
    steps: [
      owl('40117 denied'),
      advance(89_000),
      reply(TS0, 'U_REV', 'too spicy', '1700000520.000100'),
      advance(89_999),
      advance(1),
    ],
  },
  {
    name: 'repeated replies cannot hold a rejection past ten minutes',
    data: data(entry('40118', 'U1')),
    replies: { 'conversations.replies': [page([{ text: '40118 denied' }, { text: 'still', user: 'U_REV' }])] },
    steps: [
      owl('40118 denied'),
      advance(80_000), reply(TS0, 'U_REV', 'still', '1700000600.000100'),
      advance(80_000), reply(TS0, 'U_REV', 'still', '1700000600.000100'),
      advance(80_000), reply(TS0, 'U_REV', 'still', '1700000600.000100'),
      advance(80_000), reply(TS0, 'U_REV', 'still', '1700000600.000100'),
      advance(80_000), reply(TS0, 'U_REV', 'still', '1700000600.000100'),
      advance(80_000), reply(TS0, 'U_REV', 'still', '1700000600.000100'),
      advance(80_000),
      advance(40_000),
    ],
  },
  {
    name: 'unrejected cancels a pending rejection',
    data: data(entry('40119', 'U1')),
    steps: [owl('40119 denied'), owl('40119 unrejected'), advance(700_000)],
  },
  {
    name: 'a pending rejection is resumed after a restart',
    data: data(entry('40120', 'U1', false, { pendingReject: { statusTs: TS0, at: new Date().toISOString() } })),
    replies: { 'conversations.replies': [page([{ text: '40120 denied' }, { text: 'later', user: 'U_REV' }])] },
    steps: [resume(), advance(89_999), advance(1)],
  },
  {
    name: 'a reply from a bot, or the root itself, is not a reviewer reply',
    data: data(entry('40121', 'U1')),
    replies: { 'conversations.replies': [page([{ text: '40121 denied' }])] },
    steps: [owl('40121 denied'), msg({ channel: LOGS, user: 'U_REV', thread_ts: TS0, ts: TS0, text: 'hi' }), msg({ channel: LOGS, bot_id: 'B_BOT', thread_ts: TS0, ts: '1700000510.000100', text: 'hi' }), advance(SIX)],
  },
  {
    name: 'startup looks up every open entry; resolved and pending ones are skipped',
    data: data(
      entry('40201', 'U1'),
      entry('40202', 'U1', true, { status: 'approved' }),
      entry('40203', 'U1', false, { pendingReject: { statusTs: TS0, at: new Date().toISOString() } }),
    ),
    replies: {
      'conversations.history': [
        page([{ bot_id: OWL, text: '40201 approved to <#CNMU9L92Q>', ts: '1700000900.000100' }]),
        page([{ text: '40201: hi', ts: '1700000000.000300' }]),
      ],
      'conversations.replies': [page([{ text: '40203 denied' }, { text: 'later', user: 'U_REV' }])],
    },
    steps: [{ kind: 'startup' }, advance(SIX)],
  },
  {
    name: 'an approval that lands first stops the timer from sending a denial',
    data: data(entry('40122', 'U1')),
    steps: [owl('40122 denied'), owl('40122 approved to <#CNMU9L92Q>'), advance(SIX)],
  },
  {
    name: 'a failed rejection DM still resolves the entry',
    data: data(entry('40123', 'U1')),
    replies: { 'chat.postMessage': [ERR('user_not_found')] },
    steps: [owl('40123 denied'), advance(SIX)],
  },
  {
    name: 'track looks back and applies a status posted before tracking (history)',
    data: '[]',
    replies: {
      'conversations.history': [
        page([{ bot_id: OWL, text: '40124 approved to <#CNMU9L92Q>', ts: '1700000900.000100' }]),
        page([{ text: '40124: hi', ts: '1700000000.000300' }]),
      ],
    },
    steps: [cmd('40124', ALLOWED)],
  },
  {
    name: 'track looks back and finds a denial; the reason comes later (history)',
    data: '[]',
    replies: {
      'conversations.history': [page([{ bot_id: OWL, text: '40125 denied', ts: '1700000700.000100' }])],
      'conversations.replies': [page([{ text: '40125 denied' }, { text: 'nope', user: 'U_REV' }])],
    },
    steps: [cmd('40125', ALLOWED), advance(SIX)],
  },
  {
    name: 'search.messages finds the status first, so the history is not read',
    userToken: true,
    data: '[]',
    replies: {
      'search.messages': [{ ok: true, messages: { matches: [{ channel: { id: LOGS }, bot_id: OWL, text: '40126 approved to <#CNMU9L92Q>', ts: '1700000900.000100' }] } }],
      'conversations.history': [page([{ text: '40126: hi', ts: '1700000000.000300' }])],
    },
    steps: [cmd('40126', ALLOWED)],
  },
  {
    name: 'search.messages with no hit falls back to the history scan',
    userToken: true,
    data: '[]',
    replies: {
      'search.messages': [{ ok: true, messages: { matches: [] } }],
      'conversations.history': [page([{ bot_id: OWL, text: '40127 denied', ts: '1700000700.000100' }])],
      'conversations.replies': [page([{ text: '40127 denied' }, { text: 'why', user: 'U_REV' }])],
    },
    steps: [cmd('40127', ALLOWED), advance(SIX)],
  },
  {
    name: 'a failing search falls back to the history scan',
    userToken: true,
    data: '[]',
    replies: {
      'search.messages': [ERR('ratelimited')],
      'conversations.history': [page([{ bot_id: OWL, text: '40128 unapproved', ts: '1700000800.000100' }])],
    },
    steps: [cmd('40128', ALLOWED)],
  },
  {
    name: 'without a user token there is no search at all',
    data: '[]',
    replies: {
      'conversations.history': [page([{ bot_id: OWL, text: '40129 unapproved', ts: '1700000800.000100' }])],
    },
    steps: [cmd('40129', ALLOWED)],
  },
  {
    name: 'search hits from other users or channels are ignored',
    userToken: true,
    data: '[]',
    replies: {
      'search.messages': [{ ok: true, messages: { matches: [
        { channel: { id: 'C_OTHER' }, bot_id: OWL, text: '40130 approved to <#CNMU9L92Q>', ts: '1.1' },
        { channel: { id: LOGS }, user: 'U_HUMAN', text: '40130 approved to <#CNMU9L92Q>', ts: '1.2' },
      ] } }],
      'conversations.history': [page([])],
    },
    steps: [cmd('40130', ALLOWED)],
  },
];

// ---- normalisation and canonical call form ----------------------------------------------------------------------
export function normalize(text: string, stupid: string[]): string {
  // the not-allowed text names the operator (from config.yaml); mentions are normalised so no ID is in the fixture
  let out = text
    .replace(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z/g, '<ISO>')
    .replace(/track_input_\d+/g, 'track_input_<N>')
    .replace(/<@U[A-Z0-9]+>/g, '<@USER>');
  for (const line of stupid) out = out.split(line).join('<STUPID>');
  return out;
}

export function canon(calls: [string, unknown][]): Record<string, unknown[]> {
  const by = new Map<string, unknown[]>();
  for (const [m, p] of calls) by.set(m, [...(by.get(m) ?? []), p]);
  return Object.fromEntries([...by.keys()].sort().map((k) => [k, by.get(k)]));
}

// ---- runner (child process): imports the transformed test.ts and replays one scenario -------------------------
async function runOne(scenarioPath: string, modulePath: string) {
  const g = globalThis as any;
  const sc = JSON.parse(readFileSync(scenarioPath, 'utf8'));

  // Virtual time: Date and setTimeout follow `vnow`, which only the "advance" step moves.
  let vnow = Date.now();
  const RealDate = Date;
  class VirtualDate extends RealDate {
    constructor(...args: any[]) {
      if (args.length === 0) super(vnow);
      else super(...(args as [any]));
    }
    static now() {
      return vnow;
    }
  }
  g.Date = VirtualDate;
  type Timer = { id: number; at: number; fn: () => void; cleared: boolean };
  const timers: Timer[] = [];
  let nextId = 1;
  g.setTimeout = (fn: () => void, ms?: number) => {
    // Node: anything that is not 1 ms..2^31-1 ms (NaN included) becomes 1 ms
    let d = Number(ms);
    if (!(d >= 1 && d <= 2147483647)) d = 1;
    const t: Timer = { id: nextId++, at: vnow + Math.floor(d), fn, cleared: false };
    timers.push(t);
    return t;
  };
  g.clearTimeout = (t: Timer | undefined) => {
    if (t) t.cleared = true;
  };
  const flush = () => new Promise((resolve) => setImmediate(resolve));
  const advance = async (ms: number) => {
    const target = vnow + ms;
    for (;;) {
      const due = timers.filter((t) => !t.cleared && t.at <= target).sort((a, b) => a.at - b.at || a.id - b.id);
      if (due.length === 0) break;
      const t = due[0];
      t.cleared = true;
      vnow = t.at;
      t.fn();
      await flush();
    }
    vnow = target;
  };

  // main() logs its start-up line; keep stdout for the JSON result
  console.log = (...args: unknown[]) => console.error(...args);
  g.__h = { command: {}, event: {}, action: {} };
  g.__calls = [] as [string, unknown][];
  g.__responded = [] as [string, unknown][];
  g.__queue = {} as Record<string, any[]>;
  for (const [m, list] of Object.entries(sc.replies ?? {})) g.__queue[m] = [...(list as any[])];
  const record = async (method: string, params: unknown) => {
    g.__calls.push([method, JSON.parse(JSON.stringify(params ?? {}))]);
    const r = g.__queue[method]?.shift();
    if (r && r.__error) throw new Error(r.__error);
    return r ?? { ok: true };
  };
  g.__web = {
    chat: { postMessage: (a: unknown) => record('chat.postMessage', a) },
    conversations: { history: (a: unknown) => record('conversations.history', a), replies: (a: unknown) => record('conversations.replies', a) },
  };
  g.__userWeb = { search: { messages: (a: unknown) => record('search.messages', a) } };
  const client = { views: { publish: (a: unknown) => record('views.publish', a) } };
  const respond = async (msg: any) => {
    g.__responded.push([RESPOND_URL, typeof msg === 'string' ? { text: msg } : msg]);
  };

  await import(modulePath);
  const out: unknown[] = [];
  for (const step of sc.steps) {
    try {
      switch (step.kind) {
        case 'command':
          await g.__h.command['/mrox']({ command: step.payload, ack: async () => {}, respond });
          break;
        case 'message':
          await g.__h.event['message']({ event: step.event, logger: { error: () => {} } });
          break;
        case 'home':
          await g.__h.event['app_home_opened']({ event: step.event, client });
          break;
        case 'track':
          await g.__h.action['track_confession_input']({ ack: async () => {}, body: step.body, action: step.action, client });
          break;
        case 'untrack':
          await g.__h.action['untrack_confession']({ ack: async () => {}, body: step.body, action: step.action, client });
          break;
        case 'cleanup':
          g.__cleanup();
          break;
        case 'resume':
          g.__resume();
          break;
        case 'startup':
          // main() minus the file check and the connection: cleanup, pending rejections, status lookbacks
          await g.__main();
          break;
        case 'advance':
          await advance(step.ms);
          break;
      }
    } catch {
      // Bolt logs a listener error and sends nothing more
    }
    await flush();
    out.push({ calls: [...g.__calls], responded: [...g.__responded], data: readFileSync(process.env.DATA_FILE!, 'utf8') });
    g.__calls.length = 0;
    g.__responded.length = 0;
  }
  process.stdout.write(JSON.stringify(out));
  process.exit(0);
}

// ---- orchestrator ------------------------------------------------------------------------------------------------
function transform(src: string): string {
  const swap = (s: string, from: string, to: string) => {
    if (!s.includes(from)) throw new Error(`transform anchor not found: ${from}`);
    return s.replace(from, to);
  };
  let s = src;
  s = swap(s, "import { App } from '@slack/bolt';", `const App = class {
  constructor(_opts: unknown) {}
  command(name: string, fn: any) { (globalThis as any).__h.command[name] = fn; }
  event(name: string, fn: any) { (globalThis as any).__h.event[name] = fn; }
  action(name: string, fn: any) { (globalThis as any).__h.action[name] = fn; }
  async start() {}
};`);
  // one WebClient stub: the user token gets the user client, everything else the bot client
  s = swap(s, "import { WebClient } from '@slack/web-api';", `const WebClient = function (this: unknown, token: unknown) {
  const g = globalThis as any;
  return token !== undefined && token === process.env.SLACK_USER_TOKEN ? g.__userWeb : g.__web;
} as any;`);
  s = swap(s, 'main().catch((error) => {', '(globalThis as any).__main = () => main().catch((error) => {');
  return s + '\n(globalThis as any).__cleanup = cleanup;\n(globalThis as any).__resume = resumePendingRejections;\n';
}

function orchestrate() {
  const work = mkdtempSync(join(tmpdir(), 'mrox-parity-'));
  mkdirSync(work, { recursive: true });
  symlinkSync(join(repo, 'node_modules'), join(work, 'node_modules'), 'dir');
  const modulePath = join(work, 'test-under-test.ts');
  writeFileSync(modulePath, transform(readFileSync(join(repo, 'test.ts'), 'utf8')));
  const fixtures = SCENARIOS.map((sc, i) => {
    const scFile = join(work, `${i}.scenario.json`);
    const dataFile = join(work, `${i}.confessions.json`);
    writeFileSync(dataFile, sc.data ?? '[]');
    writeFileSync(scFile, JSON.stringify({ replies: sc.replies ?? {}, steps: sc.steps }));
    const env: Record<string, string> = {
      PATH: process.env.PATH ?? '',
      DATA_FILE: dataFile,
      CONFIG_FILE: join(repo, 'config.yaml'),
      SLACK_BOT_TOKEN: 'xoxb-test',
    };
    if (sc.userToken) env.SLACK_USER_TOKEN = 'xoxp-test';
    env.ALLOWED_USER_ID = ALLOWED;
    if (sc.restrict === false) env.RESTRICT_TO_USER = 'false';
    if (sc.verify) env.VERIFY_ID_IN_STATUS = 'true';
    const run = spawnSync(process.execPath, [join(repo, 'node_modules', 'tsx', 'dist', 'cli.mjs'), fileURLToPath(import.meta.url), 'run', scFile, modulePath], {
      env,
      cwd: work,
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    if (run.status !== 0) throw new Error(`scenario "${sc.name}" failed:\n${run.stderr}`);
    const steps: { calls: [string, unknown][]; responded: [string, unknown][]; data: string }[] = JSON.parse(run.stdout);
    const expected = steps.map((st) => normalize(JSON.stringify({ calls: canon(st.calls), responded: st.responded, data: st.data }), STUPID));
    return {
      name: sc.name,
      restrict: sc.restrict !== false,
      verify: !!sc.verify,
      userToken: !!sc.userToken,
      data: sc.data ?? '[]',
      replies: sc.replies ?? {},
      steps: sc.steps,
      expected,
    };
  });
  const outFile = join(here, 'scenarios.json');
  writeFileSync(outFile, JSON.stringify({ stupid: STUPID, scenarios: fixtures }, null, 2) + '\n');
  console.log(`wrote ${fixtures.length} scenarios, ${fixtures.reduce((n, f) => n + f.steps.length, 0)} steps to ${outFile}`);
}

if (process.argv[2] === 'run') {
  await runOne(process.argv[3], process.argv[4]);
} else {
  orchestrate();
}
