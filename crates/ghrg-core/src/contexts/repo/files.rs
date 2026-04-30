use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

pub const KIND: &str = "files";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch repository file entries, optionally filtered by glob and ref",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "glob",
            description: "Glob or path prefix used to narrow file lookups",
            required: false,
        },
        RepoContextFieldDoc {
            name: "limit",
            description: "Maximum number of file entries to fetch",
            required: false,
        },
        RepoContextFieldDoc {
            name: "ref",
            description: "Git ref or branch to inspect",
            required: false,
        },
    ],
    validation_rules: &[
        "`glob` and `ref` must be non-empty when present",
        "`limit` must be positive",
        "Live requests clamp `limit` to 500",
        "Omitted `ref` defaults to the repo default branch",
    ],
    example_rego: "count(input.contexts.workflow_files) > 0",
    performance_note: "Often one of the more expensive contexts; always narrow by `glob` and `limit`.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some("workflow_files".to_string()),
        },
        provider: ContextProvider::Files(RepoFilesContext {
            glob: Some(".github/workflows/*.yml".to_string()),
            limit: Some(5),
            reference: Some(default_branch.to_string()),
        }),
    }
}

pub fn explicit_spec(default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Files(RepoFilesContext {
            glob: Some("src/**".to_string()),
            limit: Some(5),
            reference: Some(default_branch.to_string()),
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoFilesQuery {
    pub limit: Option<u16>,
    pub glob: Option<String>,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoFilesContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl RepoFilesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.glob.as_deref().is_some_and(str::is_empty) {
            return Err("`files.glob` must be a non-empty string".to_string());
        }
        if self.reference.as_deref().is_some_and(str::is_empty) {
            return Err("`files.ref` must be a non-empty string".to_string());
        }
        if self.limit.is_some_and(|value| value == 0) {
            return Err("`files.limit` must be a positive integer".to_string());
        }
        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.glob.as_ref().map(|value| format!("glob={value}")),
            self.limit.map(|value| format!("limit={value}")),
            self.reference.as_ref().map(|value| format!("ref={value}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
    pub fn sample_value(&self, seed: &SampleRepoSeed) -> Value {
        let count = self.limit.unwrap_or(3).clamp(1, 5) as usize;
        let base = match self.glob.as_deref() {
            Some(pattern) if pattern.starts_with("docs/") => "docs",
            Some(pattern) if pattern.starts_with(".github/") => ".github",
            _ => "src",
        };

        Value::Array(
            (0..count)
                .map(|index| {
                    let path = match base {
                        "docs" => format!("docs/page-{}.md", index + 1),
                        ".github" => format!(".github/workflows/check-{}.yml", index + 1),
                        _ => format!("src/module_{}/lib.rs", index + 1),
                    };
                    serde_json::json!({
                        "name": path.rsplit('/').next().unwrap_or(seed.name.as_str()),
                        "path": path,
                        "type": "blob",
                        "mode": "100644",
                        "sha": format!("{:040x}", index + 101),
                        "size": 200 + (index as u64 * 17),
                        "reference": self.reference.clone().unwrap_or_else(|| seed.default_branch.clone()),
                        "glob": self.glob.clone().unwrap_or_else(|| "**".to_string()),
                    })
                })
                .collect(),
        )
    }

    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoFiles + Sync,
    {
        client.resolve_repo_files(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoFiles {
    async fn resolve_repo_files(
        &self,
        repo: &RepositoryBase,
        context: &RepoFilesContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoFiles for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_files(
        &self,
        repo: &RepositoryBase,
        context: &RepoFilesContext,
    ) -> Result<Value> {
        let mut query = RepoFilesQuery {
            limit: context.limit.map(|value| value.clamp(1, 500) as u16),
            glob: context.glob.clone(),
            reference: context.reference.clone(),
        };
        if query.reference.is_none() {
            query.reference = Some(repo.default_branch.clone());
        }
        let rows = self
            .fetch_repo_files(&repo.owner, &repo.name, &query)
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}
