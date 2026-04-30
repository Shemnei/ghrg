mod inspect;
mod test;
mod trace;

use async_trait::async_trait;
use clap::{Args as ClapArgs, Subcommand};
use ghrg_core::contexts::repo::{SampleRepoSeed, repo_context_catalog_entry};
use ghrg_core::policy::{
    ContextRequest, ContextResolver, ContextSpec, Engine, Finished, LoadedPolicyMetadata,
    MetadataSourceKind,
};
use miette::{IntoDiagnostic, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use crate::cli::Cli;
use crate::commands::sample_data::fallback_repo_seed;
use crate::runtime::RuntimeInfo;

#[derive(Debug, ClapArgs)]
#[command(
    about = "Inspect, test, and trace policies locally",
    long_about = "Work on Rego policies without calling GitHub. Inspect policy metadata, run a policy chain against local JSON input, and trace each evaluation step before using the same policies with `ghrg repos`.",
    after_help = "Examples:\n  ghrg policy inspect --policy examples/policies/project-summary.rego\n  ghrg policy test --policy examples/policies/filter-active.rego --policy examples/policies/project-summary.rego --input examples/inputs/repo.json\n  ghrg policy trace --policy examples/policies/filter-active.rego --policy examples/policies/project-summary.rego --input examples/inputs/repo.json"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Show a policy's package, metadata, and requested contexts")]
    Inspect(inspect::Args),
    #[command(about = "Evaluate one or more policies against local JSON input")]
    Test(test::Args),
    #[command(about = "Trace each evaluation step in a local policy chain")]
    Trace(trace::Args),
}

pub async fn run(_cli: &Cli, _runtime: &RuntimeInfo, args: &Args) -> Result<()> {
    match &args.command {
        Command::Inspect(args) => inspect::run(args).await,
        Command::Test(args) => test::run(args).await,
        Command::Trace(args) => trace::run(args).await,
    }
}

#[derive(Debug)]
pub(crate) struct CompiledPolicies {
    pub engine: Engine<Finished>,
    pub context_specs: Vec<ContextSpec>,
}

pub(crate) fn compile_policies(paths: &[PathBuf]) -> Result<CompiledPolicies> {
    let mut engine = Engine::new();
    for path in paths {
        engine.push_file(path)?;
    }

    let engine = engine.finish().into_diagnostic()?;
    let context_specs = engine.context_specs().to_vec();

    Ok(CompiledPolicies {
        engine,
        context_specs,
    })
}

struct LocalSampleResolver {
    seed: SampleRepoSeed,
}

impl LocalSampleResolver {
    fn from_input(input: &Value) -> Self {
        let fallback = fallback_repo_seed();
        let object = input.as_object();

        let name = object
            .and_then(|object| object.get("name"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(&fallback.name)
            .to_string();
        let full_name = object
            .and_then(|object| object.get("full_name"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(&fallback.full_name)
            .to_string();
        let default_branch = object
            .and_then(|object| object.get("default_branch"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(&fallback.default_branch)
            .to_string();

        Self {
            seed: SampleRepoSeed {
                name,
                full_name,
                default_branch,
            },
        }
    }
}

#[async_trait(?Send)]
impl ContextResolver for LocalSampleResolver {
    async fn resolve(
        &self,
        _input: &Value,
        requests: &[ContextRequest],
    ) -> ghrg_core::Result<Map<String, Value>> {
        Ok(Map::from_iter(requests.iter().map(|request| {
            (request.key.clone(), request.spec.sample_value(&self.seed))
        })))
    }
}

fn read_json_input(path: &Path) -> Result<Value> {
    let source = fs::read_to_string(path).into_diagnostic()?;
    serde_json::from_str(&source).into_diagnostic()
}

fn metadata_source_label(metadata: Option<&LoadedPolicyMetadata>) -> String {
    metadata
        .map(|loaded| match loaded.source {
            MetadataSourceKind::Embedded => "embedded",
            MetadataSourceKind::Sidecar => "sidecar",
        })
        .unwrap_or("none")
        .to_string()
}

#[derive(Serialize)]
pub(crate) struct ContextInspectView {
    pub input_key: String,
    pub kind: String,
    pub summary: Option<String>,
    pub config_fields: Vec<String>,
    pub validation_rules: Vec<String>,
    pub example_rego: Option<String>,
    pub performance_note: Option<String>,
    pub spec: Value,
}

pub(crate) fn inspect_contexts(
    metadata: Option<&LoadedPolicyMetadata>,
) -> Result<Vec<ContextInspectView>> {
    metadata
        .map(|loaded| {
            loaded
                .metadata
                .contexts
                .iter()
                .map(inspect_context)
                .collect::<Result<Vec<_>>>()
        })
        .transpose()
        .map(|value| value.unwrap_or_default())
}

pub(crate) fn inspect_context(context: &ContextSpec) -> Result<ContextInspectView> {
    let doc = repo_context_catalog_entry(context.kind());
    Ok(ContextInspectView {
        input_key: context.input_key().to_string(),
        kind: context.kind().to_string(),
        summary: doc.map(|doc| doc.summary.to_string()),
        config_fields: doc
            .map(|doc| {
                doc.fields
                    .iter()
                    .map(|field| {
                        format!(
                            "{}: {}{}",
                            field.name,
                            field.description,
                            if field.required { " (required)" } else { "" }
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        validation_rules: doc
            .map(|doc| {
                doc.validation_rules
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect()
            })
            .unwrap_or_default(),
        example_rego: doc.map(|doc| doc.example_rego.to_string()),
        performance_note: doc.map(|doc| doc.performance_note.to_string()),
        spec: serde_json::to_value(context).into_diagnostic()?,
    })
}
