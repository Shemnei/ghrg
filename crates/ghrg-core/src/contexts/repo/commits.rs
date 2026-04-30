use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

pub const KIND: &str = "commits";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch recent commits, optionally filtered by path, author, or ref",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "limit",
            description: "Maximum number of commits to fetch",
            required: false,
        },
        RepoContextFieldDoc {
            name: "path",
            description: "Restrict commits to a repository path prefix",
            required: false,
        },
        RepoContextFieldDoc {
            name: "author",
            description: "Restrict commits to a specific author login",
            required: false,
        },
        RepoContextFieldDoc {
            name: "ref",
            description: "Git ref or branch to query",
            required: false,
        },
    ],
    validation_rules: &[
        "`limit` must be positive",
        "`path`, `author`, and `ref` must be non-empty when present",
        "Live requests clamp `limit` to 100",
    ],
    example_rego: "some commit in input.contexts.recent_src_commits\ncommit.author == \"octocat\"",
    performance_note: "More expensive than properties or languages; best after cheap filters.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some("recent_src_commits".to_string()),
        },
        provider: ContextProvider::Commits(RepoCommitsContext {
            limit: Some(3),
            path: Some("src/".to_string()),
            author: None,
            reference: Some(default_branch.to_string()),
        }),
    }
}

pub fn explicit_spec(default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Commits(RepoCommitsContext {
            limit: Some(3),
            path: Some("src/".to_string()),
            author: Some("octocat".to_string()),
            reference: Some(default_branch.to_string()),
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoCommitsQuery {
    pub limit: Option<u8>,
    pub path: Option<String>,
    pub author: Option<String>,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoCommitsContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl RepoCommitsContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.is_some_and(|value| value == 0) {
            return Err("`commits.limit` must be a positive integer".to_string());
        }
        for (field, value) in [
            ("path", self.path.as_deref()),
            ("author", self.author.as_deref()),
            ("ref", self.reference.as_deref()),
        ] {
            if value.is_some_and(str::is_empty) {
                return Err(format!("`commits.{field}` must be a non-empty string"));
            }
        }
        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.map(|value| format!("limit={value}")),
            self.path.as_ref().map(|value| format!("path={value}")),
            self.author.as_ref().map(|value| format!("author={value}")),
            self.reference.as_ref().map(|value| format!("ref={value}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
    pub fn sample_value(&self, seed: &SampleRepoSeed) -> Value {
        let count = self.limit.unwrap_or(3).clamp(1, 5) as usize;
        Value::Array(
            (0..count)
                .map(|index| {
                    serde_json::json!({
                        "sha": format!("{:040x}", index + 1),
                        "author": self.author.clone().unwrap_or_else(|| "octocat".to_string()),
                        "message": format!("Sample commit {} for {}", index + 1, seed.full_name),
                        "path": self.path.clone(),
                        "ref": self.reference.clone().unwrap_or_else(|| seed.default_branch.clone()),
                    })
                })
                .collect(),
        )
    }

    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoCommits + Sync,
    {
        client.resolve_repo_commits(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoCommits {
    async fn resolve_repo_commits(
        &self,
        repo: &RepositoryBase,
        context: &RepoCommitsContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoCommits for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_commits(
        &self,
        repo: &RepositoryBase,
        context: &RepoCommitsContext,
    ) -> Result<Value> {
        let rows = self
            .fetch_repo_commits(
                &repo.owner,
                &repo.name,
                &RepoCommitsQuery {
                    limit: context.limit.map(|value| value.clamp(1, 100) as u8),
                    path: context.path.clone(),
                    author: context.author.clone(),
                    reference: context.reference.clone(),
                },
            )
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}
