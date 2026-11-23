use crate::Daemon;
use nova_ai_usage::{AiUsageConfig, Paths};
use nova_ai_usage_api::Command;
use nova_hub::ClientId;
use nova_socket::{Answer, Caller, done};
use std::sync::Arc;

use nova_ai_usage::FEATURE_ID;

pub struct Usage(Arc<nova_ai_usage::Usage>);

impl std::ops::Deref for Usage {
    type Target = nova_ai_usage::Usage;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn answer(daemon: &Daemon, _: &Caller, command: &Command) -> Answer {
    let usage = daemon.module::<Usage>();
    Answer::Now(match command {
        Command::RefreshAiUsage => done(usage.map(|usage| usage.refresh())),
        Command::ShowAiUsage { provider } => done(usage.and_then(|usage| usage.show(*provider))),
        Command::ReportClaudeUsage { limits } => done(usage.and_then(|usage| usage.report_claude(limits.clone()))),
    })
}

impl nova_modules::Module for Usage {
    type App = crate::modules::Novad;
    const ID: &'static str = FEATURE_ID;
    const TOPIC: Option<nova_carrier::Topic> = Some(nova_ai_usage_api::TOPIC);
    type Sources = Paths;
    type Config = AiUsageConfig;

    fn sources(env: &crate::modules::Env) -> Paths {
        let args = env.args;
        Paths::new(
            &args.state_dir,
            args.codex_home.clone(),
            args.claude_credentials.clone(),
            args.claude_desktop_config.clone(),
        )
    }

    fn config(config: &crate::config::Config) -> AiUsageConfig {
        config.ai_usage.clone()
    }

    fn start(
        ctx: &crate::modules::Context,
        paths: &Paths,
        config: &AiUsageConfig,
        _: &[nova_daemon_info::FeatureState],
    ) -> Arc<Self> {
        Arc::new(Self(nova_ai_usage::Usage::start(
            paths.clone(),
            config,
            Arc::clone(&ctx.hub),
            Arc::clone(&ctx.logger),
        )))
    }

    fn send_state(&self, client: ClientId) {
        self.0.send_state(client);
    }

    fn stop(&self) {
        self.0.stop();
    }
}
