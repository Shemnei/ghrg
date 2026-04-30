mod sample;

use async_trait::async_trait;
use clap::{ArgGroup, Args as ClapArgs, Subcommand};
use futures::stream::{self, StreamExt};
use ghrg_core::cache::{Cache, CacheLayer, CacheRunStats, CacheSettings, CacheStats};
use ghrg_core::contexts::repo::resolve_all;
use ghrg_core::github::{GitHubClient, RepoDataSource, RepoScope, RepositoryBase};
use ghrg_core::policy::{ContextResolver, OutcomeVisitor};
use miette::{IntoDiagnostic, Result, miette};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;

use crate::auth::resolve_credentials;
use crate::cli::Cli;
use crate::commands::policy::compile_policies;
use crate::output::{CommandOutput, OutputFormat, OutputRecord, PrettyFormatOptions};
use crate::runtime::RuntimeInfo;
use crate::ui::Ui;

#[derive(Debug, ClapArgs)]
#[command(subcommand_negates_reqs = true)]
#[command(group(
    ArgGroup::new("scope")
        .args(["org", "user", "owner", "repo"])
        .multiple(false)
        .required(true)
))]
#[command(
    about = "Scan repositories from GitHub and apply policies",
    long_about = "List repositories from a GitHub org, user, owner, or single repo; fetch any contexts requested by policies; then render the final visible output as pretty text, JSON, CSV, or raw debug data.",
    after_help = "Examples:\n  ghrg repos --repo octo-org/api --format json\n  ghrg repos --org acme --policy examples/unarchived-repo-ownership-summary/filter-unarchived.rego --policy examples/unarchived-repo-ownership-summary/repo-ownership-summary.rego --format csv\n  ghrg repos sample --schema-only --policy examples/policies/project-summary.rego"
)]
pub struct Args {
    #[arg(
        long,
        group = "scope",
        help = "Scan repositories in a GitHub organization"
    )]
    pub org: Option<String>,

    #[arg(
        long,
        group = "scope",
        help = "Scan repositories owned by a GitHub user"
    )]
    pub user: Option<String>,

    #[arg(
        long,
        group = "scope",
        help = "Scan repositories for an owner that may be a user or org"
    )]
    pub owner: Option<String>,

    #[arg(
        long,
        group = "scope",
        help = "Scan a single repository in `owner/name` form"
    )]
    pub repo: Option<String>,

    #[arg(
        long,
        help = "Policy file to apply; pass multiple times to evaluate a chain in order"
    )]
    pub policy: Vec<PathBuf>,

    #[arg(
        long,
        default_value = "pretty",
        help = "Render repository results as pretty text, JSON, CSV, or raw debug output"
    )]
    pub format: ReposFormat,

    #[arg(long, help = "Group pretty output by a visible field")]
    pub group_by: Option<String>,

    #[arg(long, help = "Sort pretty output by a visible field")]
    pub sort_by: Option<String>,

    #[arg(long, help = "Write rendered results to a file instead of stdout")]
    pub output: Option<PathBuf>,

    #[arg(long, help = "Cap the number of repositories loaded from GitHub")]
    pub limit: Option<usize>,

    #[arg(long, help = "Maximum number of repositories to process in parallel")]
    pub concurrency: Option<usize>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Generate sample repository input for policy authoring")]
    Sample(sample::Args),
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ReposFormat {
    Pretty,
    Json,
    Csv,
    Raw,
}

#[derive(Debug, Clone, serde::Serialize)]
struct RepoScanFailure {
    repo: String,
    error: String,
}

#[derive(Debug, Clone)]
struct RepoScanSummary {
    total: usize,
    kept: usize,
    dropped: usize,
    failed: usize,
}

#[derive(Debug, Clone)]
struct ReposRunSummary {
    output: CommandOutput,
    scan: RepoScanSummary,
    cache_run: CacheRunStats,
    cache_disk: CacheStats,
    failures: Vec<RepoScanFailure>,
}

enum RepoProcessResult {
    Visible(usize, OutputRecord),
    Dropped,
    Failed(RepoScanFailure),
}

pub async fn run(cli: &Cli, runtime: &RuntimeInfo, args: &Args) -> Result<()> {
    if let Some(command) = &args.command {
        return match command {
            Command::Sample(args) => sample::run(cli, runtime, args).await,
        };
    }

    scan(cli, runtime, args).await
}

async fn scan(cli: &Cli, runtime: &RuntimeInfo, args: &Args) -> Result<()> {
    let ui = Ui::new();
    let prepare = ui.spinner("Preparing repository scan");
    let client = build_repo_client(cli, runtime)?;
    let compiled = (!args.policy.is_empty())
        .then(|| compile_policies(&args.policy))
        .transpose()?;
    ui.finish(
        prepare,
        format!(
            "Prepared repository scan with {} polic{}",
            args.policy.len(),
            if args.policy.len() == 1 { "y" } else { "ies" }
        ),
    );

    let list = ui.spinner(format!(
        "Listing repositories for {}",
        scope_label(selected_scope(args))
    ));
    let records = client
        .list_repos(selected_scope(args), args.limit)
        .await
        .into_diagnostic()?;
    ui.finish(list, format!("Loaded {} repositories", records.len()));
    let (output, scan, failures) = repos_output(
        client.clone(),
        records,
        compiled.map(|compiled| Arc::new(compiled.engine)),
        requested_concurrency(args),
        &ui,
    )
    .await?;
    log_cache_state(client.cache(), "repos scan");
    finish_scan(
        args,
        ReposRunSummary {
            output,
            scan,
            cache_run: client.cache().run_stats(),
            cache_disk: client.cache().stats()?,
            failures,
        },
    )
}

pub(super) fn build_repo_client(
    cli: &Cli,
    runtime: &RuntimeInfo,
) -> Result<CacheLayer<GitHubClient>> {
    let credentials = resolve_credentials(cli)?;

    Ok(CacheLayer::new(
        GitHubClient::new(credentials)?,
        Cache::new(CacheSettings {
            dir: runtime.cache_dir_for_cache(),
            disk_enabled: runtime.disk_cache_enabled,
            ttl: cli.cache_ttl,
            force_refetch: cli.force_refetch,
        }),
    ))
}

pub(super) fn log_cache_state(cache: &Cache, operation: &str) {
    cache.log_summary(operation);
    if let Ok(stats) = cache.stats() {
        debug!(
            cache_entry_count = stats.entry_count,
            cache_size_bytes = stats.size_bytes,
            "cache directory stats after operation"
        );
    }
}

fn finish_scan(args: &Args, result: ReposRunSummary) -> Result<()> {
    let ReposRunSummary {
        output,
        scan,
        cache_run,
        cache_disk,
        failures,
    } = result;
    let rendered = match args.format {
        ReposFormat::Pretty => output.format_pretty(pretty_format_options(args)?),
        ReposFormat::Json => output.format(OutputFormat::Json),
        ReposFormat::Csv => output.format(OutputFormat::Csv),
        ReposFormat::Raw => output.format(OutputFormat::Raw),
    }?;

    if let Some(path) = &args.output {
        fs::write(path, rendered).into_diagnostic()?;
        eprintln!("Wrote repository results to {}", path.display());
    } else {
        println!("{rendered}");
    }

    eprintln!("{}", render_scan_summary(&scan, &cache_run, &cache_disk));

    if !failures.is_empty() {
        eprintln!(
            "{}",
            render_failure_summary(args.format.clone(), &failures)?
        );
    }

    Ok(())
}

fn pretty_format_options(args: &Args) -> Result<PrettyFormatOptions> {
    if matches!(args.format, ReposFormat::Pretty) {
        return Ok(PrettyFormatOptions {
            group_by: args.group_by.clone(),
            sort_by: args.sort_by.clone(),
        });
    }

    if args.group_by.is_some() || args.sort_by.is_some() {
        return Err(miette!(
            "`--group-by` and `--sort-by` are only supported with `--format pretty`"
        ));
    }

    Ok(PrettyFormatOptions::default())
}

fn selected_scope(args: &Args) -> RepoScope<'_> {
    if let Some(repo) = args.repo.as_deref() {
        RepoScope::Repo(repo)
    } else if let Some(org) = args.org.as_deref() {
        RepoScope::Org(org)
    } else if let Some(user) = args.user.as_deref() {
        RepoScope::User(user)
    } else {
        RepoScope::Owner(args.owner.as_deref().expect("clap ensures one scope"))
    }
}

async fn repos_output<T>(
    client: T,
    records: Vec<RepositoryBase>,
    engine: Option<Arc<ghrg_core::policy::Engine<ghrg_core::policy::Finished>>>,
    concurrency: usize,
    ui: &Ui,
) -> Result<(CommandOutput, RepoScanSummary, Vec<RepoScanFailure>)>
where
    T: RepoDataSource + Clone + Sync,
{
    let total = records.len();

    let progress = ui.progress(
        total as u64,
        format!(
            "Scanning repositories with concurrency {}",
            concurrency.max(1)
        ),
    );

    let mut jobs = stream::iter(records.into_iter().enumerate().map(|(index, repo)| {
        let client = client.clone();
        let engine = engine.clone();
        async move { process_repo(index, client, repo, engine).await }
    }))
    .buffer_unordered(concurrency.max(1));

    let mut visible = Vec::new();
    let mut failures = Vec::new();
    let mut dropped = 0usize;

    while let Some(result) = jobs.next().await {
        match result {
            RepoProcessResult::Visible(index, record) => visible.push((index, record)),
            RepoProcessResult::Dropped => dropped += 1,
            RepoProcessResult::Failed(failure) => failures.push(failure),
        }

        progress.inc(1);
        progress.set_message(format!(
            "Scanning repositories with concurrency {} (kept {}, dropped {}, failed {})",
            concurrency.max(1),
            visible.len(),
            dropped,
            failures.len()
        ));
    }

    ui.finish(
        progress,
        format!(
            "Scanned {} repositories: {} kept, {} dropped, {} failed",
            total,
            visible.len(),
            dropped,
            failures.len()
        ),
    );

    let kept = visible.len();
    let failed = failures.len();
    let scan = RepoScanSummary {
        total,
        kept,
        dropped,
        failed,
    };

    aggregate_repo_results(visible, failures).map(|(output, failures)| (output, scan, failures))
}

async fn process_repo<T>(
    index: usize,
    client: T,
    repo: RepositoryBase,
    engine: Option<Arc<ghrg_core::policy::Engine<ghrg_core::policy::Finished>>>,
) -> RepoProcessResult
where
    T: RepoDataSource + Clone + Sync,
{
    let repo_name = repo.full_name.clone();

    match process_repo_inner(client, repo, engine.as_deref()).await {
        Ok(Some(record)) => RepoProcessResult::Visible(index, record),
        Ok(None) => RepoProcessResult::Dropped,
        Err(error) => RepoProcessResult::Failed(RepoScanFailure {
            repo: repo_name,
            error: error.to_string(),
        }),
    }
}

async fn process_repo_inner<T>(
    client: T,
    repo: RepositoryBase,
    engine: Option<&ghrg_core::policy::Engine<ghrg_core::policy::Finished>>,
) -> Result<Option<OutputRecord>>
where
    T: RepoDataSource + Clone + Sync,
{
    let input = serde_json::to_value(repo.clone().into_policy_input()).into_diagnostic()?;

    let Some(engine) = engine else {
        return Ok(Some(OutputRecord::from_serializable(&input)?));
    };

    let resolver = RepoContextResolver {
        client: &client,
        repo: &repo,
    };
    let result = engine.run(&input, &resolver, OutcomeVisitor).await?;
    if !result.keep {
        return Ok(None);
    }

    Ok(Some(OutputRecord::from_serializable(
        &result.result.object.json_value(),
    )?))
}

struct RepoContextResolver<'a, T> {
    client: &'a T,
    repo: &'a RepositoryBase,
}

#[async_trait(?Send)]
impl<T> ContextResolver for RepoContextResolver<'_, T>
where
    T: RepoDataSource + Sync,
{
    async fn resolve(
        &self,
        _input: &serde_json::Value,
        requests: &[ghrg_core::policy::ContextRequest],
    ) -> ghrg_core::Result<serde_json::Map<String, serde_json::Value>> {
        let specs = requests
            .iter()
            .map(|request| request.spec.clone())
            .collect::<Vec<_>>();
        resolve_all(self.client, self.repo, &specs).await
    }
}

fn aggregate_repo_results(
    mut visible: Vec<(usize, OutputRecord)>,
    failures: Vec<RepoScanFailure>,
) -> Result<(CommandOutput, Vec<RepoScanFailure>)> {
    visible.sort_by_key(|(index, _)| *index);

    if visible.is_empty() && !failures.is_empty() {
        return Err(miette!(
            "all repositories failed during scan; first error: {}",
            failures[0].error
        ));
    }

    Ok((
        CommandOutput::collection(
            "repos",
            Some("Repositories".to_string()),
            visible.into_iter().map(|(_, record)| record).collect(),
        ),
        failures,
    ))
}

fn render_scan_summary(
    scan: &RepoScanSummary,
    cache_run: &CacheRunStats,
    cache_disk: &CacheStats,
) -> String {
    format!(
        concat!(
            "scan summary: total={} kept={} dropped={} failed={}\n",
            "cache summary: hits={} misses={} stale={} writes={} bypassed_reads={} entries={} size_bytes={}"
        ),
        scan.total,
        scan.kept,
        scan.dropped,
        scan.failed,
        cache_run.hits,
        cache_run.misses,
        cache_run.stale,
        cache_run.writes,
        cache_run.bypassed_reads,
        cache_disk.entry_count,
        cache_disk.size_bytes,
    )
}

fn render_failure_summary(format: ReposFormat, failures: &[RepoScanFailure]) -> Result<String> {
    match format {
        ReposFormat::Json | ReposFormat::Raw => serde_json::to_string_pretty(&serde_json::json!({
            "kind": "scan_failures",
            "count": failures.len(),
            "failures": failures,
        }))
        .into_diagnostic(),
        ReposFormat::Pretty | ReposFormat::Csv => {
            let mut lines = vec![format!(
                "scan completed with {} repository error(s):",
                failures.len()
            )];
            for failure in failures {
                lines.push(format!("- {}: {}", failure.repo, failure.error));
            }
            Ok(lines.join("\n"))
        }
    }
}

fn requested_concurrency(args: &Args) -> usize {
    args.concurrency.unwrap_or_else(default_concurrency).max(1)
}

fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(8)
}

fn scope_label(scope: RepoScope<'_>) -> String {
    match scope {
        RepoScope::Repo(repo) => format!("repository {repo}"),
        RepoScope::Org(org) => format!("organization {org}"),
        RepoScope::User(user) => format!("user {user}"),
        RepoScope::Owner(owner) => format!("owner {owner}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ghrg_core::contexts::repo::branches::RepoBranchesQuery;
    use ghrg_core::contexts::repo::commits::RepoCommitsQuery;
    use ghrg_core::contexts::repo::contributors::RepoContributorsQuery;
    use ghrg_core::contexts::repo::files::RepoFilesQuery;
    use ghrg_core::contexts::repo::workflow_runs::RepoWorkflowRunsQuery;
    use ghrg_core::github::{
        RepoBranch, RepoCommitEntry, RepoContributor, RepoFileEntry, RepoScope,
        RepoWorkflowRunEntry,
    };
    use ghrg_core::policy::{OutputField, OutputObject};
    use serde_json::{Map, Value};
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn aggregate_repo_results_keeps_visible_records_with_failures() {
        let visible = vec![(1, sample_record("b")), (0, sample_record("a"))];
        let failures = vec![RepoScanFailure {
            repo: "org/fail".to_string(),
            error: "boom".to_string(),
        }];

        let (output, failures) = aggregate_repo_results(visible, failures).unwrap();

        assert_eq!(output.visible_records().len(), 2);
        assert_eq!(
            output.visible_records()[0].object.field("name"),
            Some(&serde_json::json!("a"))
        );
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn aggregate_repo_results_errors_when_all_fail() {
        let error = aggregate_repo_results(
            Vec::new(),
            vec![RepoScanFailure {
                repo: "org/fail".to_string(),
                error: "boom".to_string(),
            }],
        )
        .unwrap_err();

        assert!(error.to_string().contains("all repositories failed"));
    }

    #[test]
    fn render_failure_summary_is_machine_readable_for_json_modes() {
        let rendered = render_failure_summary(
            ReposFormat::Json,
            &[RepoScanFailure {
                repo: "org/fail".to_string(),
                error: "boom".to_string(),
            }],
        )
        .unwrap();

        assert!(rendered.contains("\"kind\": \"scan_failures\""));
        assert!(rendered.contains("\"repo\": \"org/fail\""));
    }

    #[test]
    fn render_scan_summary_includes_scan_and_cache_stats() {
        let rendered = render_scan_summary(
            &RepoScanSummary {
                total: 10,
                kept: 7,
                dropped: 2,
                failed: 1,
            },
            &CacheRunStats {
                hits: 4,
                misses: 6,
                stale: 1,
                writes: 5,
                bypassed_reads: 0,
            },
            &CacheStats {
                entry_count: 12,
                size_bytes: 3456,
            },
        );

        assert!(rendered.contains("scan summary: total=10 kept=7 dropped=2 failed=1"));
        assert!(rendered.contains(
            "cache summary: hits=4 misses=6 stale=1 writes=5 bypassed_reads=0 entries=12 size_bytes=3456"
        ));
    }

    #[test]
    fn pretty_format_options_accepts_group_and_sort_for_pretty() {
        let options = pretty_format_options(&args_with_format(ReposFormat::Pretty)).unwrap();

        assert_eq!(
            options,
            PrettyFormatOptions {
                group_by: Some("team".to_string()),
                sort_by: Some("name".to_string()),
            }
        );
    }

    #[test]
    fn pretty_format_options_rejects_transforms_for_non_pretty_formats() {
        let error = pretty_format_options(&args_with_format(ReposFormat::Json)).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("only supported with `--format pretty`")
        );
    }

    #[tokio::test]
    async fn stops_fetching_later_contexts_after_drop() {
        let dir = temp_dir();
        let first = dir.join("first.rego");
        let second = dir.join("second.rego");
        fs::write(
            &first,
            "# ```ghrg\n# contexts:\n#   - type: properties\n#     names: [\"Team\"]\n# ```\n\npackage ghrg.repos\n\ndefault allow := false\nallow if { false }\noutput := input\n",
        )
        .unwrap();
        fs::write(
            &second,
            "# ```ghrg\n# contexts:\n#   - type: commits\n#     limit: 1\n# ```\n\npackage ghrg.repos\n\ndefault allow := true\noutput := input\n",
        )
        .unwrap();

        let source = FakeSource::default();
        let repo = sample_repo();
        let compiled = compile_policies(&[first, second]).unwrap();

        let result = process_repo_inner(source.clone(), repo, Some(&compiled.engine))
            .await
            .unwrap();
        let calls = source.calls.lock().unwrap().clone();

        assert!(result.is_none());
        assert!(calls.iter().any(|call| call == "properties"));
        assert!(!calls.iter().any(|call| call == "commits"));
    }

    #[test]
    fn compile_policies_rejects_invalid_policy_before_scan() {
        let dir = temp_dir();
        let invalid = dir.join("broken.rego");
        fs::write(
            &invalid,
            "package ghrg.repos\n\ndefault allow := true\nallow if {\n",
        )
        .unwrap();

        let error = compile_policies(&[invalid]).unwrap_err();

        assert!(error.to_string().contains("policy load"));
    }

    fn sample_record(name: &str) -> OutputRecord {
        OutputRecord::from_object(OutputObject::new(vec![OutputField::new("name", name)]))
    }

    fn sample_repo() -> RepositoryBase {
        RepositoryBase {
            name: "api".to_string(),
            owner: "acme".to_string(),
            full_name: "acme/api".to_string(),
            archived: false,
            fork: false,
            visibility: "public".to_string(),
            default_branch: "main".to_string(),
            topics: Vec::new(),
            github: serde_json::Map::new(),
        }
    }

    #[derive(Default, Clone)]
    struct FakeSource {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl RepoDataSource for FakeSource {
        async fn fetch_repo(
            &self,
            owner: &str,
            name: &str,
        ) -> ghrg_core::error::Result<RepositoryBase> {
            Ok(RepositoryBase {
                owner: owner.to_string(),
                name: name.to_string(),
                full_name: format!("{owner}/{name}"),
                ..sample_repo()
            })
        }

        async fn list_repos(
            &self,
            _scope: RepoScope<'_>,
            _limit: Option<usize>,
        ) -> ghrg_core::error::Result<Vec<RepositoryBase>> {
            Ok(vec![sample_repo()])
        }

        async fn fetch_repo_properties(
            &self,
            _owner: &str,
            _repo: &str,
            names: &BTreeSet<String>,
        ) -> ghrg_core::error::Result<Map<String, Value>> {
            self.calls.lock().unwrap().push("properties".to_string());
            Ok(Map::from_iter(
                names
                    .iter()
                    .cloned()
                    .map(|name| (name, Value::String("x".to_string()))),
            ))
        }

        async fn fetch_repo_languages(
            &self,
            _owner: &str,
            _repo: &str,
        ) -> ghrg_core::error::Result<Map<String, Value>> {
            self.calls.lock().unwrap().push("languages".to_string());
            Ok(Map::from_iter([(String::from("Rust"), Value::from(1234))]))
        }

        async fn fetch_repo_branches(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoBranchesQuery,
        ) -> ghrg_core::error::Result<Vec<RepoBranch>> {
            self.calls.lock().unwrap().push("branches".to_string());
            Ok(vec![RepoBranch {
                name: "main".to_string(),
                protected: true,
                sha: "abc".to_string(),
                url: "https://api.github.com/repos/acme/api/branches/main".to_string(),
                html_url: Some("https://github.com/acme/api/tree/main".to_string()),
            }])
        }

        async fn fetch_repo_commits(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoCommitsQuery,
        ) -> ghrg_core::error::Result<Vec<RepoCommitEntry>> {
            self.calls.lock().unwrap().push("commits".to_string());
            Ok(vec![RepoCommitEntry {
                sha: "abc".to_string(),
                message: "example commit".to_string(),
                committed_at: Some("2024-01-01T00:00:00+00:00".to_string()),
                author_login: Some("octocat".to_string()),
            }])
        }

        async fn fetch_repo_files(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoFilesQuery,
        ) -> ghrg_core::error::Result<Vec<RepoFileEntry>> {
            self.calls.lock().unwrap().push("files".to_string());
            Ok(vec![RepoFileEntry {
                name: "lib.rs".to_string(),
                path: "src/lib.rs".to_string(),
                entry_type: "blob".to_string(),
                mode: Some("100644".to_string()),
                sha: Some("abc".to_string()),
                size: Some(42),
                reference: "main".to_string(),
                glob: "**".to_string(),
            }])
        }

        async fn fetch_repo_contributors(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoContributorsQuery,
        ) -> ghrg_core::error::Result<Vec<RepoContributor>> {
            self.calls.lock().unwrap().push("contributors".to_string());
            Ok(vec![RepoContributor {
                login: Some("octocat".to_string()),
                id: Some(1),
                contributor_type: "User".to_string(),
                html_url: Some("https://github.com/octocat".to_string()),
                avatar_url: Some("https://avatars.githubusercontent.com/u/1".to_string()),
                email: None,
                contributions: 42,
                anonymous: false,
            }])
        }

        async fn fetch_repo_workflow_runs(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoWorkflowRunsQuery,
        ) -> ghrg_core::error::Result<Vec<RepoWorkflowRunEntry>> {
            self.calls.lock().unwrap().push("workflow_runs".to_string());
            Ok(vec![RepoWorkflowRunEntry {
                id: 1,
                name: Some("CI".to_string()),
                event: "push".to_string(),
                status: Some("completed".to_string()),
                conclusion: Some("success".to_string()),
                head_branch: Some("main".to_string()),
                head_sha: "abc".to_string(),
                run_number: 42,
                run_attempt: Some(1),
                actor_login: Some("octocat".to_string()),
                html_url: "https://github.com/acme/api/actions/runs/1".to_string(),
                created_at: "2024-01-01T00:00:00Z".to_string(),
                updated_at: "2024-01-01T00:05:00Z".to_string(),
            }])
        }
    }

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ghrg-repos-test-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args_with_format(format: ReposFormat) -> Args {
        Args {
            org: Some("acme".to_string()),
            user: None,
            owner: None,
            repo: None,
            policy: Vec::new(),
            format,
            group_by: Some("team".to_string()),
            sort_by: Some("name".to_string()),
            output: None,
            limit: None,
            concurrency: None,
            command: None,
        }
    }
}
