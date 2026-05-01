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
            limit: Some(5.into()),
            branch: Some("main".to_string().into()),
            event: None,
            status: Some("completed".to_string().into()),
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
            limit: Some(5.into()),
            branch: Some(default_branch.to_string().into()),
            event: Some("push".to_string().into()),
            status: Some("completed".to_string().into()),
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
    pub limit: Option<ContextValue<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<ContextValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<ContextValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ContextValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ContextValue<String>>,
}

impl RepoWorkflowRunsContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.as_ref().and_then(ContextValue::literal) == Some(&0) {
            return Err("`workflow_runs.limit` must be a positive integer".to_string());
        }

        if let Some(limit) = self.limit.as_ref().and_then(ContextValue::default_value)
            && *limit == 0
        {
            return Err("`workflow_runs.limit.default` must be a positive integer".to_string());
        }

        if let Some(limit) = &self.limit {
            limit.validate_source("workflow_runs.limit")?;
        }
        if let Some(branch) = &self.branch {
            branch.validate_source("workflow_runs.branch")?;
        }
        if let Some(event) = &self.event {
            event.validate_source("workflow_runs.event")?;
        }
        if let Some(status) = &self.status {
            status.validate_source("workflow_runs.status")?;
        }
        if let Some(actor) = &self.actor {
            actor.validate_source("workflow_runs.actor")?;
        }

        for (field, value) in [
            (
                "branch",
                self.branch.as_ref().and_then(ContextValue::literal),
            ),
            ("event", self.event.as_ref().and_then(ContextValue::literal)),
            (
                "status",
                self.status.as_ref().and_then(ContextValue::literal),
            ),
            ("actor", self.actor.as_ref().and_then(ContextValue::literal)),
        ] {
            if value.is_some_and(|value| value.is_empty()) {
                return Err(format!(
                    "`workflow_runs.{field}` must be a non-empty string"
                ));
            }
        }

        for (field, value) in [
            (
                "branch",
                self.branch.as_ref().and_then(ContextValue::default_value),
            ),
            (
                "event",
                self.event.as_ref().and_then(ContextValue::default_value),
            ),
            (
                "status",
                self.status.as_ref().and_then(ContextValue::default_value),
            ),
            (
                "actor",
                self.actor.as_ref().and_then(ContextValue::default_value),
            ),
        ] {
            if value.is_some_and(|value| value.is_empty()) {
                return Err(format!(
                    "`workflow_runs.{field}.default` must be a non-empty string"
                ));
            }
        }

        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.as_ref().map(render_param("limit")),
            self.branch.as_ref().map(render_param("branch")),
            self.event.as_ref().map(render_param("event")),
            self.status.as_ref().map(render_param("status")),
            self.actor.as_ref().map(render_param("actor")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }

    pub fn sample_value(&self, seed: &SampleRepoSeed) -> Value {
        let count = self
            .limit
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or(3)
            .clamp(1, 5) as usize;
        let branch = self
            .branch
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or_else(|| seed.default_branch.clone());
        let event = self
            .event
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or_else(|| "push".to_string());
        let status = self
            .status
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or_else(|| "completed".to_string());
        let actor = self
            .actor
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or_else(|| "octocat".to_string());

        Value::Array(
            (0..count)
                .map(|index| {
                    let run_number = index as u64 + 41;
                    let conclusion = if status == "completed" {
                        Some(if index % 2 == 0 { "success" } else { "failure" })
                    } else {
                        None
                    };
                    serde_json::json!({
                        "id": 9000 + index as u64,
                        "name": if index % 2 == 0 { "CI" } else { "Release" },
                        "event": event.clone(),
                        "status": status.clone(),
                        "conclusion": conclusion,
                        "head_branch": branch.clone(),
                        "head_sha": format!("{:040x}", index + 301),
                        "run_number": run_number,
                        "run_attempt": 1,
                        "actor_login": actor.clone(),
                        "html_url": format!("https://github.com/{}/actions/runs/{}", seed.full_name, 9000 + index as u64),
                        "created_at": format!("2026-01-0{}T12:00:00Z", index + 1),
                        "updated_at": format!("2026-01-0{}T12:05:00Z", index + 1),
                    })
                })
                .collect(),
        )
    }

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            limit: resolve_optional_context_value(&self.limit, runtime, "workflow_runs.limit")?
                .map(ContextValue::from),
            branch: resolve_optional_context_value(&self.branch, runtime, "workflow_runs.branch")?
                .map(ContextValue::from),
            event: resolve_optional_context_value(&self.event, runtime, "workflow_runs.event")?
                .map(ContextValue::from),
            status: resolve_optional_context_value(&self.status, runtime, "workflow_runs.status")?
                .map(ContextValue::from),
            actor: resolve_optional_context_value(&self.actor, runtime, "workflow_runs.actor")?
                .map(ContextValue::from),
        })
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
                    limit: context
                        .limit
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .copied()
                        .map(|value| value.clamp(1, 100) as u8),
                    branch: context
                        .branch
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .cloned(),
                    event: context
                        .event
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .cloned(),
                    status: context
                        .status
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .cloned(),
                    actor: context
                        .actor
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .cloned(),
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
