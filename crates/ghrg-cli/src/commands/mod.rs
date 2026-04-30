pub mod contexts;
pub mod info;
pub mod policy;
pub mod repos;
pub(crate) mod sample_data;

use miette::Result;
use std::fs::{self, OpenOptions};
use tracing::info;
use tracing_subscriber::fmt::writer::BoxMakeWriter;

use crate::cli::{Cli, Command};
use crate::runtime::{RuntimeInfo, collect_runtime_info};

pub async fn run(cli: Cli) -> Result<()> {
    let runtime = collect_runtime_info(
        env!("CARGO_PKG_VERSION"),
        cli.cache_dir.clone(),
        cli.log_dir.clone(),
        cli.log_file.clone(),
        cli.no_disk_cache,
    )?;
    init_tracing(&runtime, &cli.log_level, cli.trace)?;
    info!(
        command = ?cli.command,
        execution_id = %runtime.execution_id,
        log_file = %runtime.log_file.display(),
        cache_dir = %runtime.cache_dir_display(),
        "starting ghrg command"
    );

    match &cli.command {
        Command::Contexts(args) => contexts::run(&cli, &runtime, args).await,
        Command::Info(args) => info::run(&cli, &runtime, args).await,
        Command::Policy(args) => policy::run(&cli, &runtime, args).await,
        Command::Repos(args) => repos::run(&cli, &runtime, args).await,
    }
}

fn init_tracing(runtime: &RuntimeInfo, log_level: &str, trace: bool) -> Result<()> {
    let filter = if trace { "trace" } else { log_level };
    fs::create_dir_all(&runtime.log_dir).map_err(|error| miette::miette!(error.to_string()))?;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&runtime.log_file)
        .map_err(|error| miette::miette!(error.to_string()))?;
    let writer = BoxMakeWriter::new(move || {
        log_file
            .try_clone()
            .expect("cloning configured log file should succeed")
    });

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .map_err(|error| miette::miette!(error.to_string()))?;

    Ok(())
}
