//! mrox: Socket Mode bot. Start-up mirrors `main()` in `test.ts`: dotenv, settings, data-file check, cleanup,
//! hourly cleanup, connect, then serve.
use mrox::bot::Bot;
use mrox::config::{Messages, Settings, CLEANUP_INTERVAL_SECS};
use mrox::slack::client::SlackClient;
use mrox::slack::socket::{route, serve, Envelope, Sink, Transport};
use mrox::slack::ws::WsTransport;
use mrox::store::Store;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

/// Acked envelopes go to a listener on its own task, so a slow history scan never delays the socket loop.
struct Router {
    bot: Arc<Bot<SlackClient>>,
}

impl Sink for Router {
    fn dispatch(&self, env: Envelope) {
        if let Some(d) = route(&env) {
            let bot = self.bot.clone();
            tokio::spawn(async move { bot.dispatch(d).await });
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    // `import 'dotenv/config'`: reads ./.env without overriding variables already set.
    let dotenv = cwd.join(".env");
    if dotenv.is_file() {
        let _ = dotenvy::from_path(&dotenv);
    }
    let cfg = Settings::from_env(&cwd);
    // Bolt's App constructor throws without these, before anything else runs.
    let Some(bot_token) = cfg.bot_token.clone() else {
        eprintln!("Apps used in a single workspace can be initialized with a token. Apps used in many workspaces should be initialized with oauth installer options or authorize.");
        return ExitCode::from(1);
    };
    let Some(app_token) = cfg.app_token.clone() else {
        eprintln!("You must provide an appToken when socketMode is set to true. To generate an appToken see: https://api.slack.com/apis/connections/socket#token");
        return ExitCode::from(1);
    };

    let http = match reqwest::Client::builder().user_agent("mrox").pool_max_idle_per_host(2).build() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Failed to build the HTTP client: {e}");
            return ExitCode::from(1);
        }
    };
    let msgs = Messages::load(&cfg.config_file);
    let store = Store::new(cfg.data_file.clone());
    let slack = SlackClient::new(http.clone(), cfg.api_base.clone(), bot_token, cfg.user_token.clone());
    let bot = Bot::new(cfg.clone(), msgs, store, slack);

    // main(): ensureFiles(); cleanup(); resumePendingRejections(); the status lookbacks; then the hourly cleanup.
    // A failure before the lookbacks is fatal.
    if cfg.restrict_to_user && cfg.allowed_user_id.is_empty() {
        eprintln!("ALLOWED_USER_ID is not set, so /mrox refuses everyone (set it in .env)");
    }
    if let Err(e) = bot.store.ensure().and_then(|()| bot.start_up()) {
        eprintln!("Failed to start: {e}");
        return ExitCode::from(1);
    }
    let hourly = bot.clone();
    tokio::spawn(async move {
        let period = Duration::from_secs(CLEANUP_INTERVAL_SECS);
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        loop {
            tick.tick().await;
            if let Err(e) = hourly.cleanup() {
                eprintln!("Uncaught exception: {e}");
            }
        }
    });

    // app.start(): the first connection must succeed; later drops reconnect inside `serve`.
    let transport = WsTransport { client: http, api_base: cfg.api_base.clone(), app_token };
    let conn = match transport.connect().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to start: {e}");
            return ExitCode::from(1);
        }
    };
    println!("(test.ts) Bot is running...");
    serve(&transport, &Router { bot }, conn).await;
    ExitCode::SUCCESS
}
