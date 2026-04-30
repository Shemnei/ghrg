use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::time::Duration;

use crate::commands;

#[derive(Debug, Parser)]
#[command(
    name = "ghrg",
    version,
    about = "Scan GitHub repositories with Rego-based governance policies",
    long_about = "Scan GitHub repositories, enrich them with requested context, and filter or reshape the visible output with Rego policies. Start with `ghrg info`, test policies locally with `ghrg policy test`, then run `ghrg repos` against GitHub.",
    after_help = "Examples:\n  ghrg info\n  ghrg policy test --policy examples/policies/filter-active.rego --policy examples/policies/project-summary.rego --input examples/inputs/repo.json\n  ghrg repos --org acme --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego --format csv"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        help = "Force a specific auth method instead of auto mode"
    )]
    pub auth: Option<AuthMethod>,

    #[arg(
        long,
        global = true,
        default_value = "env",
        help = "Load secrets from environment variables or Secret Service"
    )]
    pub auth_source: AuthSource,

    #[arg(long, global = true, help = "Path to a config file")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, help = "Override the disk cache directory")]
    pub cache_dir: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        help = "Disable the on-disk GitHub response cache"
    )]
    pub no_disk_cache: bool,

    #[arg(long, global = true, default_value = "1h", value_parser = parse_duration, help = "Cache TTL, for example `5m`, `1h`, or `1d`")]
    pub cache_ttl: Duration,

    #[arg(
        long,
        global = true,
        help = "Bypass cached responses and refetch from GitHub"
    )]
    pub force_refetch: bool,

    #[arg(long, global = true, help = "Directory for CLI log files")]
    pub log_dir: Option<PathBuf>,

    #[arg(long, global = true, help = "Write logs to a specific file")]
    pub log_file: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        default_value = "info",
        help = "Tracing level for logs, for example `info` or `debug`"
    )]
    pub log_level: String,

    #[arg(
        long,
        global = true,
        help = "Enable verbose tracing in the CLI runtime"
    )]
    pub trace: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum AuthMethod {
    GhCli,
    GhApp,
}

impl AuthMethod {
    pub fn label(&self) -> &'static str {
        match self {
            Self::GhCli => "gh-cli",
            Self::GhApp => "gh-app",
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AuthSource {
    Env,
    SecretService,
}

impl AuthSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::SecretService => "secret-service",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "List supported context kinds and inspect their shapes")]
    Contexts(commands::contexts::Args),
    #[command(about = "Show runtime, auth, cache, and logging info")]
    Info(commands::info::Args),
    #[command(about = "Inspect, test, and trace policies locally")]
    Policy(commands::policy::Args),
    #[command(about = "Scan repositories from GitHub and apply policies")]
    Repos(commands::repos::Args),
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormatArg {
    Pretty,
    Json,
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    humantime::parse_duration(value).map_err(|error| error.to_string())
}
