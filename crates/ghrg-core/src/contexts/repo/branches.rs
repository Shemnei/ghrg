use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{
    ContextBase, ContextProvider, ContextSpec, ContextValue, DynamicContextData,
    resolve_optional_context_value,
};
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
            limit: Some(3.into()),
            protected: Some(true.into()),
        }),
    }
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Branches(RepoBranchesContext {
            limit: Some(5.into()),
            protected: Some(true.into()),
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
    pub limit: Option<ContextValue<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected: Option<ContextValue<bool>>,
}

impl RepoBranchesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.as_ref().and_then(ContextValue::literal) == Some(&0) {
            return Err("`branches.limit` must be a positive integer".to_string());
        }
        if let Some(limit) = self.limit.as_ref().and_then(ContextValue::default_value)
            && *limit == 0
        {
            return Err("`branches.limit.default` must be a positive integer".to_string());
        }

        if let Some(limit) = &self.limit {
            limit.validate_source("branches.limit")?;
        }
        if let Some(protected) = &self.protected {
            protected.validate_source("branches.protected")?;
        }

        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.as_ref().map(render_param("limit")),
            self.protected.as_ref().map(render_param("protected")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
    pub fn sample_value(&self, seed: &SampleRepoSeed) -> Value {
        let protected = self
            .protected
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or(true);
        let names = [
            seed.default_branch.clone(),
            "develop".to_string(),
            "release".to_string(),
        ];
        let count = self
            .limit
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or(3)
            .clamp(1, names.len() as u64) as usize;

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

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            limit: resolve_optional_context_value(&self.limit, runtime, "branches.limit")?
                .map(ContextValue::from),
            protected: resolve_optional_context_value(
                &self.protected,
                runtime,
                "branches.protected",
            )?
            .map(ContextValue::from),
        })
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
                    limit: context
                        .limit
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .copied()
                        .map(|value| value.clamp(1, 100) as u8),
                    protected: context
                        .protected
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .copied(),
                },
            )
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}

fn render_param<T: std::fmt::Display>(name: &str) -> impl FnOnce(&ContextValue<T>) -> String + '_ {
    move |value| match value {
        ContextValue::Literal(value) => format!("{name}={value}"),
        ContextValue::Ref(reference) => format!("{name}<-{}", reference.from),
    }
}
