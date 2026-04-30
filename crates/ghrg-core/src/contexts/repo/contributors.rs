use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

pub const KIND: &str = "contributors";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch contributor summaries, with optional anonymous entries",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "limit",
            description: "Maximum number of contributors to fetch",
            required: false,
        },
        RepoContextFieldDoc {
            name: "anonymous",
            description: "Include anonymous contributors when supported",
            required: false,
        },
    ],
    validation_rules: &[
        "`limit` must be positive",
        "Live requests clamp `limit` to 100",
    ],
    example_rego: "count(input.contexts.top_contributors) >= 3",
    performance_note: "Moderate cost; limit the result set when you only need a threshold.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some("top_contributors".to_string()),
        },
        provider: ContextProvider::Contributors(RepoContributorsContext {
            limit: Some(5),
            anonymous: Some(false),
        }),
    }
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Contributors(RepoContributorsContext {
            limit: Some(10),
            anonymous: Some(false),
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoContributorsQuery {
    pub limit: Option<u16>,
    pub anonymous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoContributorsContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous: Option<bool>,
}

impl RepoContributorsContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.is_some_and(|value| value == 0) {
            return Err("`contributors.limit` must be a positive integer".to_string());
        }
        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.map(|value| format!("limit={value}")),
            self.anonymous.map(|value| format!("anonymous={value}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
    pub fn sample_value(&self, _seed: &SampleRepoSeed) -> Value {
        let count = self.limit.unwrap_or(3).clamp(1, 5) as usize;
        Value::Array(
            (0..count)
                .map(|index| {
                    let anonymous = self.anonymous.unwrap_or(false) && index + 1 == count;
                    serde_json::json!({
                        "login": (!anonymous).then(|| format!("contributor-{}", index + 1)),
                        "id": (!anonymous).then(|| index + 1000),
                        "type": if anonymous { "Anonymous" } else { "User" },
                        "html_url": (!anonymous).then(|| format!("https://github.com/contributor-{}", index + 1)),
                        "avatar_url": (!anonymous).then(|| format!("https://avatars.githubusercontent.com/u/{}", index + 1000)),
                        "email": anonymous.then(|| format!("contributor{}@example.com", index + 1)),
                        "contributions": 20 - (index as u64 * 3),
                        "anonymous": anonymous,
                    })
                })
                .collect(),
        )
    }

    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoContributors + Sync,
    {
        client.resolve_repo_contributors(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoContributors {
    async fn resolve_repo_contributors(
        &self,
        repo: &RepositoryBase,
        context: &RepoContributorsContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoContributors for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_contributors(
        &self,
        repo: &RepositoryBase,
        context: &RepoContributorsContext,
    ) -> Result<Value> {
        let rows = self
            .fetch_repo_contributors(
                &repo.owner,
                &repo.name,
                &RepoContributorsQuery {
                    limit: context.limit.map(|value| value.clamp(1, 100) as u16),
                    anonymous: context.anonymous.unwrap_or(false),
                },
            )
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}
