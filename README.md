# Mrox

Mrox is a Slack bot that watches [Prox2 / Owl] confession activity and DMs the
submitter when their confession's status changes (approved, approved for meta,
denied, or unapproved).

Currently implemented in **`test.ts`** (the actively-developed version).

## What it does

- **`/mrox {id}`** — start tracking a confession by its numeric ID (or
  `/mrox untrack {id}`). Only the configured user can call it while the test
  guard is on.
- **App Home box** — paste/type a confession ID and press Enter, no command
  needed. Tracked confessions are listed with Untrack buttons.
- Watches the confession **logs** channel for status updates like
  `Confession #40289 was approved`.
- **DMs the submitter** with the result:
  - Approved → link to the post in `#confessions`
  - Approved for meta → link in `#meta`
  - Denied → includes the reason pulled from the log thread
  - Unapproved → thread-nuked notice
- **Auto-follow thread** (optional, App Home toggle, default OFF): on approval,
  silently subscribes the submitter to the post's thread by pinging then
  instantly deleting the ping.
- Marks confessions resolved and purges them 2 days later.
- Easter egg for impossible states (a status update on an already-resolved
  confession).

## Setup

1. Create the Slack app from `app_manifest.yaml` (api.slack.com → Create New App
   → From manifest).
2. Add tokens to `.env` (see `.env.example`):
   - `SLACK_BOT_TOKEN` (`xoxb-*`) — Bot User OAuth Token
   - `SLACK_APP_TOKEN` (`xapp-*`) — App-Level Token with `connections:write`
3. Invite the bot to `#confessions`, `#meta`, and the confession logs channel.
4. Install the app, then run:

   ```sh
   npm start
   ```

## Configuration

All channel IDs and toggles have sensible defaults in `test.ts` and can be
overridden with env vars (see `.env.example`):

| Variable | Default | Purpose |
| --- | --- | --- |
| `CHANNEL_CONFESSIONS` | `CNMU9L92Q` | `#confessions` |
| `CHANNEL_CONFESSION_LOGS` | `C08R8LE6DDG` | logs / status channel |
| `CHANNEL_META` | `C0188CY57PZ` | `#meta` |
| `OWL_BOT_ID` | `B0C6JPG4W0G` | Prox2/Owl bot id |
| `WORKSPACE_DOMAIN` | `hackclub.enterprise.slack.com` | permalink building |
| `RESTRICT_TO_USER` | `true` | only `ALLOWED_USER_ID` may use `/mrox` |
| `ALLOWED_USER_ID` | `U09KKMHLS15` | the allowed user |
| `VERIFY_ID_IN_STATUS` | `false` | require status to reference the id |
| `DEBUG` | `false` | verbose logging |

To test without touching the real flow, override the `CHANNEL_*` variables to
point at throwaway channels.

## Data

- `confessions.json` — tracked confessions (`id`, `userId`, `dmChannel`, `resolved`,
  `date`, `postTs`, `status`). `dmChannel` is the DM channel opened with the
  owner on first notification and reused after that. Writes are atomic.
- `settings.json` — per-user preferences (App Home toggle).

## Notes

- Mrox cannot read other users' DMs with Prox2; Slack does not allow bots to
  read DMs they aren't part of. Track via `/mrox {id}` or the App Home box.
- The bot only acts on status messages from the Owl bot (`OWL_BOT_ID`) in the
  logs channel.
- The bot needs `channels:history` and to be a member of the channels it scans.
