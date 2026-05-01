pub mod branches;
pub mod commits;
pub mod contributors;
pub mod files;
pub mod languages;
pub mod properties;
pub mod workflow_runs;

use async_trait::async_trait;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::contexts::{ContextSpec, DynamicContextData};
use crate::error::{GhrgError, Result};
use crate::github::{RepoDataSource, RepositoryBase};

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

#[async_trait]
pub trait RepoContextResolver: Send + Sync {
    fn validate_params(&self, params: &Map<String, Value>) -> std::result::Result<(), String>;
    fn render_params(&self, params: &Map<String, Value>) -> std::result::Result<String, String>;
    fn sample_value(
        &self,
        params: &Map<String, Value>,
        seed: &SampleRepoSeed,
    ) -> std::result::Result<Value, String>;
    fn resolve_dynamic(
        &self,
        params: &Map<String, Value>,
        runtime: &DynamicContextData<'_>,
    ) -> std::result::Result<Map<String, Value>, GhrgError>;
    async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
        params: &Map<String, Value>,
    ) -> std::result::Result<Value, GhrgError>;
}

pub struct RepoContextRegistration {
    pub catalog: RepoContextCatalogEntry,
    pub resolver: &'static dyn RepoContextResolver,
}

static REPO_CONTEXT_REGISTRY: &[RepoContextRegistration] = &[
    RepoContextRegistration {
        catalog: properties::CATALOG_ENTRY,
        resolver: &properties::RESOLVER,
    },
    RepoContextRegistration {
        catalog: languages::CATALOG_ENTRY,
        resolver: &languages::RESOLVER,
    },
    RepoContextRegistration {
        catalog: branches::CATALOG_ENTRY,
        resolver: &branches::RESOLVER,
    },
    RepoContextRegistration {
        catalog: commits::CATALOG_ENTRY,
        resolver: &commits::RESOLVER,
    },
    RepoContextRegistration {
        catalog: files::CATALOG_ENTRY,
        resolver: &files::RESOLVER,
    },
    RepoContextRegistration {
        catalog: contributors::CATALOG_ENTRY,
        resolver: &contributors::RESOLVER,
    },
    RepoContextRegistration {
        catalog: workflow_runs::CATALOG_ENTRY,
        resolver: &workflow_runs::RESOLVER,
    },
];

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
    REPO_CONTEXT_REGISTRY
        .iter()
        .find(|entry| entry.catalog.kind == kind)
        .map(|entry| &entry.catalog)
}

fn repo_context_registration(kind: &str) -> Option<&'static RepoContextRegistration> {
    REPO_CONTEXT_REGISTRY
        .iter()
        .find(|entry| entry.catalog.kind == kind)
}

pub fn repo_context_kinds() -> Vec<&'static str> {
    repo_context_catalog()
        .iter()
        .map(|entry| entry.kind)
        .collect()
}

pub fn validate_context_spec(spec: &ContextSpec) -> std::result::Result<(), String> {
    let Some(registration) = repo_context_registration(spec.kind()) else {
        return Err(format!("invalid context kind `{}`", spec.kind()));
    };

    registration
        .resolver
        .validate_params(&spec.params)
        .map_err(|error| error.to_string())
}

pub fn render_context_params(spec: &ContextSpec) -> std::result::Result<String, String> {
    let Some(registration) = repo_context_registration(spec.kind()) else {
        return Err(format!("invalid context kind `{}`", spec.kind()));
    };

    registration.resolver.render_params(&spec.params)
}

pub fn sample_context_value(
    spec: &ContextSpec,
    seed: &SampleRepoSeed,
) -> std::result::Result<Value, String> {
    let Some(registration) = repo_context_registration(spec.kind()) else {
        return Err(format!("invalid context kind `{}`", spec.kind()));
    };

    registration.resolver.sample_value(&spec.params, seed)
}

pub fn resolve_dynamic_context_spec(
    spec: &ContextSpec,
    runtime: &DynamicContextData<'_>,
) -> Result<ContextSpec> {
    let registration =
        repo_context_registration(spec.kind()).ok_or_else(|| GhrgError::InvalidContextKind {
            kind: spec.kind().to_string(),
        })?;

    Ok(ContextSpec {
        name: spec.name.clone(),
        kind: spec.kind.clone(),
        params: registration
            .resolver
            .resolve_dynamic(&spec.params, runtime)?,
    })
}

pub async fn resolve_context_for_repo<T>(
    client: &T,
    repo: &RepositoryBase,
    spec: &ContextSpec,
) -> Result<Value>
where
    T: RepoDataSource + Sync,
{
    let registration =
        repo_context_registration(spec.kind()).ok_or_else(|| GhrgError::InvalidContextKind {
            kind: spec.kind().to_string(),
        })?;

    registration
        .resolver
        .resolve(client, repo, &spec.params)
        .await
}

pub async fn resolve_all<T>(
    client: &T,
    repo: &RepositoryBase,
    specs: &[ContextSpec],
) -> Result<Map<String, Value>>
where
    T: RepoDataSource + Sync,
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

pub fn sample_contexts(
    seed: &SampleRepoSeed,
    specs: &[ContextSpec],
    explicit: &[String],
) -> Result<Map<String, Value>> {
    let mut contexts = Map::new();
    let mut explicit_specs = BTreeMap::new();

    for kind in explicit {
        let spec = explicit_context_spec(kind, &seed.default_branch)?;
        explicit_specs.insert(spec.input_key().to_string(), spec.clone());
        contexts.insert(spec.input_key().to_string(), spec.sample_value(seed));
    }

    for spec in specs {
        if let Some(explicit_spec) = explicit_specs.get(spec.input_key())
            && !explicit_spec.same_provider(spec)
        {
            return Err(GhrgError::ConflictingContextSpec {
                key: spec.input_key().to_string(),
            });
        }
        contexts.insert(spec.input_key().to_string(), spec.sample_value(seed));
    }

    Ok(contexts)
}
