use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

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
    ContextSpec {
        base: ContextBase {
            name: Some("repo_languages".to_string()),
        },
        provider: ContextProvider::Languages(RepoLanguagesContext),
    }
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Languages(RepoLanguagesContext),
    }
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

    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoLanguages + Sync,
    {
        client.resolve_repo_languages(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoLanguages {
    async fn resolve_repo_languages(
        &self,
        repo: &RepositoryBase,
        context: &RepoLanguagesContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoLanguages for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_languages(
        &self,
        repo: &RepositoryBase,
        _context: &RepoLanguagesContext,
    ) -> Result<Value> {
        self.fetch_repo_languages(&repo.owner, &repo.name)
            .await
            .map(Value::Object)
    }
}
