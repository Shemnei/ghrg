use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

pub const KIND: &str = "properties";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch selected custom repository properties by name",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "names",
            description: "List of custom repository property names to fetch",
            required: true,
        },
    ],
    validation_rules: &["`names` must contain at least one non-empty string"],
    example_rego: "input.contexts.repo_properties.Team",
    performance_note: "Usually cheap; good for ownership or reporting enrichments.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some("repo_properties".to_string()),
        },
        provider: ContextProvider::Properties(RepoPropertiesContext {
            names: vec!["Team".to_string(), "CodeOwner".to_string()],
        }),
    }
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Properties(RepoPropertiesContext {
            names: vec!["Team".to_string(), "CodeOwner".to_string()],
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoPropertiesContext {
    pub names: Vec<String>,
}

impl RepoPropertiesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.names.is_empty() || self.names.iter().any(|value| value.is_empty()) {
            return Err(
                "`properties.names` must contain at least one non-empty string".to_string(),
            );
        }
        Ok(())
    }

    pub fn render_params(&self) -> String {
        if self.names.is_empty() {
            String::new()
        } else {
            format!(
                "names={}",
                serde_json::to_string(&self.names).unwrap_or_default()
            )
        }
    }

    pub fn sample_value(&self, _seed: &SampleRepoSeed) -> Value {
        Value::Object(Map::from_iter(self.names.iter().map(|name| {
            (
                name.clone(),
                match name.as_str() {
                    "Team" => Value::String("platform".to_string()),
                    "CodeOwner" => Value::String("@example/platform".to_string()),
                    "CostCenter" => Value::String("ENG-001".to_string()),
                    _ => Value::String(format!("example-{}", name.to_ascii_lowercase())),
                },
            )
        })))
    }
    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoProperties + Sync,
    {
        client.resolve_repo_properties(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoProperties {
    async fn resolve_repo_properties(
        &self,
        repo: &RepositoryBase,
        context: &RepoPropertiesContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoProperties for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_properties(
        &self,
        repo: &RepositoryBase,
        context: &RepoPropertiesContext,
    ) -> Result<Value> {
        self.fetch_repo_properties(
            &repo.owner,
            &repo.name,
            &context.names.iter().cloned().collect(),
        )
        .await
        .map(Value::Object)
    }
}
