use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

pub const KIND: &str = "workflow_runs";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch recent GitHub Actions workflow runs with optional branch/event/status filters",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "limit",
            description: "Maximum number of workflow runs to fetch",
            required: false,
        },
        RepoContextFieldDoc {
            name: "branch",
            description: "Restrict workflow runs to a branch name",
            required: false,
        },
        RepoContextFieldDoc {
            name: "event",
            description: "Restrict workflow runs to a triggering event such as push or pull_request",
            required: false,
        },
        RepoContextFieldDoc {
            name: "status",
            description: "Restrict workflow runs by status or conclusion such as completed or success",
            required: false,
        },
        RepoContextFieldDoc {
            name: "actor",
            description: "Restrict workflow runs to a specific triggering actor login",
            required: false,
        },
    ],
    validation_rules: &[
        "`limit` must be positive",
        "`branch`, `event`, `status`, and `actor` must be non-empty when present",
        "Live requests clamp `limit` to 100",
    ],
    example_rego: "some run in input.contexts.recent_workflow_runs\nrun.conclusion == \"success\"",
    performance_note: "Moderate cost; limit and filter aggressively when scanning many repositories.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(_default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some("recent_workflow_runs".to_string()),
        },
        provider: ContextProvider::WorkflowRuns(RepoWorkflowRunsContext {
            limit: Some(5),
            branch: Some("main".to_string()),
            event: None,
            status: Some("completed".to_string()),
            actor: None,
        }),
    }
}

pub fn explicit_spec(default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::WorkflowRuns(RepoWorkflowRunsContext {
            limit: Some(5),
            branch: Some(default_branch.to_string()),
            event: Some("push".to_string()),
            status: Some("completed".to_string()),
            actor: None,
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoWorkflowRunsQuery {
    pub limit: Option<u8>,
    pub branch: Option<String>,
    pub event: Option<String>,
    pub status: Option<String>,
    pub actor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoWorkflowRunsContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

impl RepoWorkflowRunsContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.is_some_and(|value| value == 0) {
            return Err("`workflow_runs.limit` must be a positive integer".to_string());
        }
        for (field, value) in [
            ("branch", self.branch.as_deref()),
            ("event", self.event.as_deref()),
            ("status", self.status.as_deref()),
            ("actor", self.actor.as_deref()),
        ] {
            if value.is_some_and(str::is_empty) {
                return Err(format!(
                    "`workflow_runs.{field}` must be a non-empty string"
                ));
            }
        }
        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.map(|value| format!("limit={value}")),
            self.branch.as_ref().map(|value| format!("branch={value}")),
            self.event.as_ref().map(|value| format!("event={value}")),
            self.status.as_ref().map(|value| format!("status={value}")),
            self.actor.as_ref().map(|value| format!("actor={value}")),
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
                    let run_number = index as u64 + 41;
                    let status = self
                        .status
                        .clone()
                        .unwrap_or_else(|| "completed".to_string());
                    let conclusion = if status == "completed" {
                        Some(if index % 2 == 0 { "success" } else { "failure" })
                    } else {
                        None
                    };
                    serde_json::json!({
                        "id": 9000 + index as u64,
                        "name": if index % 2 == 0 { "CI" } else { "Release" },
                        "event": self.event.clone().unwrap_or_else(|| "push".to_string()),
                        "status": status,
                        "conclusion": conclusion,
                        "head_branch": self.branch.clone().unwrap_or_else(|| seed.default_branch.clone()),
                        "head_sha": format!("{:040x}", index + 301),
                        "run_number": run_number,
                        "run_attempt": 1,
                        "actor_login": self.actor.clone().unwrap_or_else(|| "octocat".to_string()),
                        "html_url": format!("https://github.com/{}/actions/runs/{}", seed.full_name, 9000 + index as u64),
                        "created_at": format!("2026-01-0{}T12:00:00Z", index + 1),
                        "updated_at": format!("2026-01-0{}T12:05:00Z", index + 1),
                    })
                })
                .collect(),
        )
    }

    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoWorkflowRuns + Sync,
    {
        client.resolve_repo_workflow_runs(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoWorkflowRuns {
    async fn resolve_repo_workflow_runs(
        &self,
        repo: &RepositoryBase,
        context: &RepoWorkflowRunsContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoWorkflowRuns for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_workflow_runs(
        &self,
        repo: &RepositoryBase,
        context: &RepoWorkflowRunsContext,
    ) -> Result<Value> {
        let rows = self
            .fetch_repo_workflow_runs(
                &repo.owner,
                &repo.name,
                &RepoWorkflowRunsQuery {
                    limit: context.limit.map(|value| value.clamp(1, 100) as u8),
                    branch: context.branch.clone(),
                    event: context.event.clone(),
                    status: context.status.clone(),
                    actor: context.actor.clone(),
                },
            )
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}
