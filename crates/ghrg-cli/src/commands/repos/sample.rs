use clap::Args as ClapArgs;
use ghrg_core::contexts::repo::{
    SampleRepoSeed, explicit_context_spec, repo_context_kinds, resolve_all, sample_contexts,
};
use ghrg_core::github::{RepoDataSource, RepositoryBase, parse_repo_slug};
use ghrg_core::policy::ContextSpec;
use miette::{IntoDiagnostic, Result, miette};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use super::{build_repo_client, log_cache_state};
use crate::cli::Cli;
use crate::commands::policy::compile_policies;
use crate::commands::sample_data::fallback_repo_base;
use crate::runtime::RuntimeInfo;
use crate::ui::Ui;

#[derive(Debug, ClapArgs)]
#[command(
    about = "Generate sample repository input for policy authoring",
    long_about = "Build sample repository input either from a sanitized schema-only template or from a real repository. Policy-declared contexts are included so you can iterate locally with `ghrg policy test` and `ghrg policy trace`.",
    after_help = "Examples:\n  ghrg repos sample --schema-only --policy examples/policies/project-summary.rego\n  ghrg repos sample --repo octo-org/api --policy path/to/policy.rego --format yaml\n\nBrowse supported explicit contexts with `ghrg contexts repos list`."
)]
pub struct Args {
    #[arg(
        long,
        conflicts_with = "schema_only",
        help = "Fetch a live sample from a real repository in `owner/name` form"
    )]
    pub repo: Option<String>,

    #[arg(
        long,
        conflicts_with = "repo",
        help = "Build a sanitized schema-style sample without calling GitHub"
    )]
    pub schema_only: bool,

    #[arg(long, value_parser = parse_explicit_context_kind, help = "Add an explicit repository context kind; see `ghrg contexts repos list` for supported values")]
    pub context: Vec<String>,

    #[arg(
        long,
        help = "Policy file whose declared contexts should be included in the sample"
    )]
    pub policy: Vec<PathBuf>,

    #[arg(
        long,
        default_value = "json",
        help = "Render the sample as JSON or YAML"
    )]
    pub format: SampleFormat,

    #[arg(long, help = "Write the sample to a file instead of stdout")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SampleFormat {
    Json,
    Yaml,
}

pub async fn run(cli: &Cli, runtime: &RuntimeInfo, args: &Args) -> Result<()> {
    let ui = Ui::new();
    let spinner = ui.spinner("Building repository sample");
    if args.repo.is_none() && !args.schema_only {
        return Err(miette!("`repos sample` currently requires `--schema-only`"));
    }

    let sample = if let Some(repo) = &args.repo {
        build_live_sample(cli, runtime, args, repo).await?
    } else {
        build_schema_sample(args)?
    };
    let rendered = match args.format {
        SampleFormat::Json => serde_json::to_string_pretty(&sample).into_diagnostic()?,
        SampleFormat::Yaml => serde_yaml::to_string(&sample).into_diagnostic()?,
    };

    if let Some(path) = &args.output {
        fs::write(path, rendered).into_diagnostic()?;
        ui.finish(
            spinner,
            format!("Wrote repository sample to {}", path.display()),
        );
    } else {
        ui.finish(spinner, "Built repository sample");
        print!("{rendered}");
    }

    Ok(())
}

fn build_schema_sample(args: &Args) -> Result<Value> {
    let mut repo = fallback_repo_base();
    sanitize_repo_base(&mut repo);

    let specs = collect_sample_context_specs(&args.policy, &args.context, &repo.default_branch)?;
    let contexts =
        sample_contexts(&SampleRepoSeed::from_repo(&repo), &specs, &[]).into_diagnostic()?;
    serde_json::to_value(repo.into_policy_input().with_contexts(contexts)).into_diagnostic()
}

async fn build_live_sample(
    cli: &Cli,
    runtime: &RuntimeInfo,
    args: &Args,
    repo: &str,
) -> Result<Value> {
    let (owner, name) = parse_repo_slug(repo)?;
    let client = build_repo_client(cli, runtime)?;
    let repo = client.fetch_repo(owner, name).await.into_diagnostic()?;
    let specs = collect_sample_context_specs(&args.policy, &args.context, &repo.default_branch)?;
    let mut contexts =
        sample_contexts(&SampleRepoSeed::from_repo(&repo), &specs, &[]).into_diagnostic()?;
    for (key, value) in resolve_all(&client, &repo, &specs)
        .await
        .into_diagnostic()?
    {
        contexts.insert(key, value);
    }
    log_cache_state(client.cache(), "repos sample");

    serde_json::to_value(repo.into_policy_input().with_contexts(contexts)).into_diagnostic()
}

fn collect_sample_context_specs(
    policy_paths: &[PathBuf],
    explicit_kinds: &[String],
    default_branch: &str,
) -> Result<Vec<ContextSpec>> {
    let mut specs = compile_policies(policy_paths)?.context_specs;

    for kind in explicit_kinds {
        let spec = explicit_context_spec(kind, default_branch).into_diagnostic()?;
        if let Some(existing) = specs
            .iter()
            .find(|existing| existing.input_key() == spec.input_key())
        {
            if existing.provider != spec.provider {
                return Err(miette!(
                    "conflicting context specifications for `{}`",
                    spec.input_key()
                ));
            }
            continue;
        }
        specs.push(spec);
    }

    Ok(specs)
}

fn sanitize_repo_base(repo: &mut RepositoryBase) {
    repo.owner = sanitize_identifier(&repo.owner, "example-org");
    repo.name = sanitize_identifier(&repo.name, "example-repo");
    repo.full_name = format!("{}/{}", repo.owner, repo.name);
    repo.archived = false;
    repo.fork = false;
    repo.visibility = sanitize_choice(
        &repo.visibility,
        &["public", "private", "internal"],
        "public",
    );
    repo.default_branch = sanitize_identifier(&repo.default_branch, "main");
    repo.topics = repo
        .topics
        .iter()
        .map(|topic| sanitize_identifier(topic, "governance"))
        .take(3)
        .collect();
    if repo.topics.is_empty() {
        repo.topics = vec!["governance".to_string()];
    }
}

fn sanitize_choice(value: &str, choices: &[&str], fallback: &str) -> String {
    let sanitized = sanitize_identifier(value, fallback);
    if choices.iter().any(|choice| *choice == sanitized) {
        sanitized
    } else {
        fallback.to_string()
    }
}

fn sanitize_identifier(value: &str, fallback: &str) -> String {
    let sanitized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .collect::<String>();
    if sanitized.len() < 3 {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn parse_explicit_context_kind(value: &str) -> std::result::Result<String, String> {
    let kinds = repo_context_kinds();
    if kinds.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(format!(
            "invalid context kind `{value}`; supported kinds: {}",
            kinds.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn schema_sample_includes_policy_contexts() {
        let dir = temp_dir();
        let policy = dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: commits\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();

        let sample = build_schema_sample(&Args {
            repo: None,
            schema_only: true,
            context: vec!["files".to_string()],
            policy: vec![policy],
            format: SampleFormat::Json,
            output: None,
        })
        .unwrap();

        let contexts = sample.get("contexts").and_then(Value::as_object).unwrap();
        assert!(contexts.contains_key("files"));
        assert!(contexts.contains_key("commits"));
        assert!(contexts.get("files").and_then(Value::as_array).is_some());
        assert!(contexts.get("commits").and_then(Value::as_array).is_some());
    }

    #[test]
    fn schema_sample_uses_named_policy_contexts() {
        let dir = temp_dir();
        let policy = dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - name: repo_properties\n#     type: properties\n#     names: [\"Team\"]\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();

        let sample = build_schema_sample(&Args {
            repo: None,
            schema_only: true,
            context: vec![],
            policy: vec![policy],
            format: SampleFormat::Json,
            output: None,
        })
        .unwrap();

        let contexts = sample.get("contexts").and_then(Value::as_object).unwrap();
        assert_eq!(
            contexts.get("repo_properties"),
            Some(&serde_json::json!({"Team": "platform"}))
        );
    }

    #[test]
    fn schema_sample_includes_workflow_runs_context() {
        let sample = build_schema_sample(&Args {
            repo: None,
            schema_only: true,
            context: vec!["workflow_runs".to_string()],
            policy: vec![],
            format: SampleFormat::Json,
            output: None,
        })
        .unwrap();

        let contexts = sample.get("contexts").and_then(Value::as_object).unwrap();
        assert!(contexts.contains_key("workflow_runs"));
        assert!(
            contexts
                .get("workflow_runs")
                .and_then(Value::as_array)
                .is_some()
        );
    }

    #[test]
    fn schema_sample_rejects_unknown_explicit_context() {
        let error = build_schema_sample(&Args {
            repo: None,
            schema_only: true,
            context: vec!["unknown".to_string()],
            policy: vec![],
            format: SampleFormat::Json,
            output: None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("invalid context kind `unknown`"));
    }

    #[test]
    fn schema_sample_rejects_conflicting_explicit_and_policy_contexts() {
        let dir = temp_dir();
        let policy = dir.join("sample.rego");
        fs::write(
            &policy,
            "# ```ghrg\n# contexts:\n#   - type: files\n#     limit: 1\n# ```\n\npackage ghrg.repos\n",
        )
        .unwrap();

        let error = build_schema_sample(&Args {
            repo: None,
            schema_only: true,
            context: vec!["files".to_string()],
            policy: vec![policy],
            format: SampleFormat::Json,
            output: None,
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("conflicting context specifications for `files`")
        );
    }

    #[test]
    fn collected_context_specs_include_explicit_contexts_for_live_resolution() {
        let specs =
            collect_sample_context_specs(&[], &["workflow_runs".to_string()], "main").unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].input_key(), "workflow_runs");
        assert_eq!(specs[0].kind(), "workflow_runs");
    }

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ghrg-cli-test-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
