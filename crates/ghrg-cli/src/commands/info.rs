use clap::Args as ClapArgs;
use ghrg_core::cache::{Cache, CacheSettings};
use miette::Result;
use serde::Serialize;
use tracing::debug;

use crate::auth::{AuthLookupInfo, auth_lookup_info};
use crate::cli::{Cli, OutputFormatArg};
use crate::output::{CommandOutput, OutputFormat, OutputRecord};
use crate::runtime::RuntimeInfo;
use crate::ui::Ui;

#[derive(Debug, ClapArgs)]
#[command(
    about = "Show runtime, auth, cache, and logging info",
    long_about = "Print the current runtime configuration, including auth lookup order, cache settings, log paths, and version information. This is the fastest way to verify a first-run setup.",
    after_help = "Example:\n  ghrg info --format json"
)]
pub struct Args {
    #[arg(
        long,
        default_value = "pretty",
        help = "Render the output as a human-readable table or JSON"
    )]
    pub format: OutputFormatArg,
}

pub async fn run(cli: &Cli, runtime: &RuntimeInfo, args: &Args) -> Result<()> {
    let ui = Ui::new();
    let spinner = ui.spinner("Collecting runtime info");
    let cache = Cache::new(CacheSettings {
        dir: runtime.cache_dir_for_cache(),
        disk_enabled: runtime.disk_cache_enabled,
        ttl: cli.cache_ttl,
        force_refetch: cli.force_refetch,
    });
    let cache_stats = cache.stats()?;
    cache.log_summary("info");
    debug!(
        cache_entry_count = cache_stats.entry_count,
        cache_size_bytes = cache_stats.size_bytes,
        "cache directory stats for info command"
    );

    let output = info_output(
        cli,
        runtime,
        cache_stats.entry_count,
        cache_stats.size_bytes,
    );
    let format = match args.format {
        OutputFormatArg::Pretty => OutputFormat::Pretty,
        OutputFormatArg::Json => OutputFormat::Json,
    };

    let rendered = output.format(format)?;
    ui.finish(spinner, "Collected runtime info");
    println!("{rendered}");

    Ok(())
}

fn info_output(
    cli: &Cli,
    runtime: &RuntimeInfo,
    cache_entry_count: u64,
    cache_size_bytes: u64,
) -> CommandOutput {
    #[derive(Serialize)]
    struct InfoView {
        auth_method: String,
        auth_source: String,
        auth_lookup: AuthLookupInfo,
        config: String,
        disk_cache: bool,
        cache_dir: String,
        cache_ttl: String,
        cache_entry_count: u64,
        cache_size_bytes: u64,
        log_dir: String,
        log_file: String,
        execution_id: String,
        started_at: String,
        platform: String,
        version: String,
    }

    let auth_lookup = auth_lookup_info(cli);

    CommandOutput::record(
        "info",
        Some(format!("ghrg {}", runtime.version)),
        OutputRecord::from_serializable(&InfoView {
            auth_method: auth_lookup.auth_method.clone(),
            auth_source: auth_lookup.auth_source.clone(),
            auth_lookup,
            config: display_optional_path(cli.config.as_deref()),
            disk_cache: runtime.disk_cache_enabled,
            cache_dir: runtime.cache_dir_display(),
            cache_ttl: humantime::format_duration(cli.cache_ttl).to_string(),
            cache_entry_count,
            cache_size_bytes,
            log_dir: runtime.log_dir.display().to_string(),
            log_file: runtime.log_file.display().to_string(),
            execution_id: runtime.execution_id.clone(),
            started_at: runtime.started_at.clone(),
            platform: runtime.platform.clone(),
            version: runtime.version.clone(),
        })
        .expect("info output should serialize to an object"),
    )
}

fn display_optional_path(path: Option<&std::path::Path>) -> String {
    path.map(|value| value.display().to_string())
        .unwrap_or_else(|| "none".to_string())
}
