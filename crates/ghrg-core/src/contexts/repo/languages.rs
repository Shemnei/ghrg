use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::ContextSpec;
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, RepoContextResolver, SampleRepoSeed};

pub const KIND: &str = "languages";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch the repository language byte breakdown",
    fields: &[RepoContextFieldDoc {
        name: "name",
        description: "Optional custom key under input.contexts",
        required: false,
    }],
    validation_rules: &["No extra validation beyond a non-empty optional `name`"],
    example_rego: "input.contexts.languages.Rust",
    performance_note: "Usually cheap; good for rough composition checks.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(_default_branch: &str) -> ContextSpec {
    spec(Some("repo_languages"), &RepoLanguagesContext)
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    spec(Some(KIND), &RepoLanguagesContext)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoLanguagesContext;

impl RepoLanguagesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        Ok(())
    }

    pub fn render_params(&self) -> String {
        String::new()
    }

    pub fn sample_value(&self, _seed: &SampleRepoSeed) -> Value {
        serde_json::json!({
            "Rust": 18234,
            "Shell": 312,
            "Dockerfile": 88,
        })
    }

    pub fn resolve_dynamic(
        &self,
        _runtime: &crate::contexts::DynamicContextData<'_>,
    ) -> Result<Self> {
        Ok(self.clone())
    }

    pub async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
    ) -> Result<Value> {
        client
            .fetch_repo_languages(&repo.owner, &repo.name)
            .await
            .map(Value::Object)
    }
}

pub struct LanguagesResolver;
pub static RESOLVER: LanguagesResolver = LanguagesResolver;

#[async_trait]
impl RepoContextResolver for LanguagesResolver {
    fn validate_params(
        &self,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> std::result::Result<(), String> {
        parse_params(params)?.validate()
    }

    fn render_params(
        &self,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> std::result::Result<String, String> {
        Ok(parse_params(params)?.render_params())
    }

    fn sample_value(
        &self,
        params: &serde_json::Map<String, serde_json::Value>,
        seed: &SampleRepoSeed,
    ) -> std::result::Result<serde_json::Value, String> {
        Ok(parse_params(params)?.sample_value(seed))
    }

    fn resolve_dynamic(
        &self,
        params: &serde_json::Map<String, serde_json::Value>,
        runtime: &crate::contexts::DynamicContextData<'_>,
    ) -> std::result::Result<serde_json::Map<String, serde_json::Value>, crate::error::GhrgError>
    {
        let context = parse_params(params).map_err(|details| {
            crate::error::GhrgError::InvalidContextParams {
                kind: KIND.to_string(),
                details,
            }
        })?;
        let resolved = context.resolve_dynamic(runtime)?;
        Ok(to_params(&resolved))
    }

    async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> std::result::Result<serde_json::Value, crate::error::GhrgError> {
        let context = parse_params(params).map_err(|details| {
            crate::error::GhrgError::InvalidContextParams {
                kind: KIND.to_string(),
                details,
            }
        })?;
        context.resolve(client, repo).await
    }
}

fn parse_params(
    params: &serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<RepoLanguagesContext, String> {
    if params.is_empty() {
        Ok(RepoLanguagesContext)
    } else {
        Err("`languages` does not accept any params".to_string())
    }
}

fn to_params(context: &RepoLanguagesContext) -> serde_json::Map<String, serde_json::Value> {
    serde_json::to_value(context)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn spec(name: Option<&str>, context: &RepoLanguagesContext) -> ContextSpec {
    ContextSpec {
        name: name.map(ToString::to_string),
        kind: KIND.to_string(),
        params: to_params(context),
    }
}
