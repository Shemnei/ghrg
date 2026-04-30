use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

pub const KIND: &str = "branches";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch repository branches with optional protection filtering",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "limit",
            description: "Maximum number of branches to fetch",
            required: false,
        },
        RepoContextFieldDoc {
            name: "protected",
            description: "Filter for protected branches when supported",
            required: false,
        },
    ],
    validation_rules: &[
        "`limit` must be positive",
        "Live requests clamp `limit` to 100",
    ],
    example_rego: "count(input.contexts.protected_branches) > 0",
    performance_note: "Moderate cost; keep `limit` narrow when possible.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some("protected_branches".to_string()),
        },
        provider: ContextProvider::Branches(RepoBranchesContext {
            limit: Some(3),
            protected: Some(true),
        }),
    }
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Branches(RepoBranchesContext {
            limit: Some(5),
            protected: Some(true),
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoBranchesQuery {
    pub limit: Option<u8>,
    pub protected: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoBranchesContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected: Option<bool>,
}

impl RepoBranchesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.is_some_and(|value| value == 0) {
            return Err("`branches.limit` must be a positive integer".to_string());
        }
        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.map(|value| format!("limit={value}")),
            self.protected.map(|value| format!("protected={value}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
    pub fn sample_value(&self, seed: &SampleRepoSeed) -> Value {
        let protected = self.protected.unwrap_or(true);
        let names = [
            seed.default_branch.clone(),
            "develop".to_string(),
            "release".to_string(),
        ];
        let count = self.limit.unwrap_or(3).clamp(1, names.len() as u64) as usize;

        Value::Array(
            names
                .into_iter()
                .take(count)
                .enumerate()
                .map(|(index, name)| {
                    serde_json::json!({
                        "name": name,
                        "protected": if index == 0 { protected } else { false },
                        "sha": format!("{:040x}", index + 201),
                        "url": format!(
                            "https://api.github.com/repos/{}/branches/{}",
                            seed.full_name,
                            if index == 0 { seed.default_branch.as_str() } else { if index == 1 { "develop" } else { "release" } }
                        ),
                    })
                })
                .collect(),
        )
    }

    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoBranches + Sync,
    {
        client.resolve_repo_branches(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoBranches {
    async fn resolve_repo_branches(
        &self,
        repo: &RepositoryBase,
        context: &RepoBranchesContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoBranches for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_branches(
        &self,
        repo: &RepositoryBase,
        context: &RepoBranchesContext,
    ) -> Result<Value> {
        let rows = self
            .fetch_repo_branches(
                &repo.owner,
                &repo.name,
                &RepoBranchesQuery {
                    limit: context.limit.map(|value| value.clamp(1, 100) as u8),
                    protected: context.protected,
                },
            )
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}
