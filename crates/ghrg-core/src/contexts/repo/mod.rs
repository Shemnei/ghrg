pub mod branches;
pub mod commits;
pub mod contributors;
pub mod files;
pub mod languages;
pub mod properties;
pub mod workflow_runs;

use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::error::{GhrgError, Result};
use crate::github::RepositoryBase;

use super::ContextSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepoContextFieldDoc {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RepoContextCatalogEntry {
    pub kind: &'static str,
    pub summary: &'static str,
    pub fields: &'static [RepoContextFieldDoc],
    pub validation_rules: &'static [&'static str],
    pub example_rego: &'static str,
    pub performance_note: &'static str,
    pub example_spec: fn(&str) -> ContextSpec,
    pub explicit_spec: fn(&str) -> ContextSpec,
}

#[derive(Debug, Clone)]
pub struct SampleRepoSeed {
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
}

impl SampleRepoSeed {
    pub fn from_repo(repo: &RepositoryBase) -> Self {
        Self {
            name: repo.name.clone(),
            full_name: repo.full_name.clone(),
            default_branch: repo.default_branch.clone(),
        }
    }
}

pub trait ResolveRepoContext:
    properties::ResolveRepoProperties
    + branches::ResolveRepoBranches
    + commits::ResolveRepoCommits
    + languages::ResolveRepoLanguages
    + files::ResolveRepoFiles
    + contributors::ResolveRepoContributors
    + workflow_runs::ResolveRepoWorkflowRuns
{
}

impl<T> ResolveRepoContext for T where
    T: properties::ResolveRepoProperties
        + branches::ResolveRepoBranches
        + commits::ResolveRepoCommits
        + languages::ResolveRepoLanguages
        + files::ResolveRepoFiles
        + contributors::ResolveRepoContributors
        + workflow_runs::ResolveRepoWorkflowRuns
{
}

pub async fn resolve_all<T>(
    client: &T,
    repo: &RepositoryBase,
    specs: &[ContextSpec],
) -> Result<Map<String, Value>>
where
    T: ResolveRepoContext + Sync,
{
    let mut contexts = Map::new();

    for spec in specs {
        let value = spec.resolve_for_repo(client, repo).await?;
        contexts.insert(spec.input_key().to_string(), value);
    }

    Ok(contexts)
}

pub fn explicit_context_spec(kind: &str, default_branch: &str) -> Result<ContextSpec> {
    repo_context_catalog_entry(kind)
        .map(|entry| (entry.explicit_spec)(default_branch))
        .ok_or_else(|| GhrgError::InvalidContextKind {
            kind: kind.to_string(),
        })
}

pub fn repo_context_catalog() -> &'static [RepoContextCatalogEntry] {
    &[
        properties::CATALOG_ENTRY,
        languages::CATALOG_ENTRY,
        branches::CATALOG_ENTRY,
        commits::CATALOG_ENTRY,
        files::CATALOG_ENTRY,
        contributors::CATALOG_ENTRY,
        workflow_runs::CATALOG_ENTRY,
    ]
}

pub fn repo_context_catalog_entry(kind: &str) -> Option<&'static RepoContextCatalogEntry> {
    repo_context_catalog()
        .iter()
        .find(|entry| entry.kind == kind)
}

pub fn repo_context_kinds() -> Vec<&'static str> {
    repo_context_catalog()
        .iter()
        .map(|entry| entry.kind)
        .collect()
}

pub fn sample_contexts(
    seed: &SampleRepoSeed,
    specs: &[ContextSpec],
    explicit: &[String],
) -> Result<Map<String, Value>> {
    let mut contexts = Map::new();
    let mut explicit_specs = BTreeMap::new();

    for kind in explicit {
        let spec = explicit_context_spec(kind, &seed.default_branch)?;
        explicit_specs.insert(spec.input_key().to_string(), spec.provider.clone());
        contexts.insert(spec.input_key().to_string(), spec.sample_value(seed));
    }

    for spec in specs {
        if let Some(explicit_provider) = explicit_specs.get(spec.input_key())
            && explicit_provider != &spec.provider
        {
            return Err(GhrgError::ConflictingContextSpec {
                key: spec.input_key().to_string(),
            });
        }
        contexts.insert(spec.input_key().to_string(), spec.sample_value(seed));
    }

    Ok(contexts)
}
