use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{
    ContextSpec, ContextValue, DynamicContextData, resolve_optional_context_value,
};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, RepoContextResolver, SampleRepoSeed};

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
    spec(
        Some("recent_src_commits"),
        &RepoCommitsContext {
            limit: Some(3.into()),
            path: Some("src/".to_string().into()),
            author: None,
            reference: Some(default_branch.to_string().into()),
        },
    )
}

pub fn explicit_spec(default_branch: &str) -> ContextSpec {
    spec(
        Some(KIND),
        &RepoCommitsContext {
            limit: Some(3.into()),
            path: Some("src/".to_string().into()),
            author: Some("octocat".to_string().into()),
            reference: Some(default_branch.to_string().into()),
        },
    )
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
    pub limit: Option<ContextValue<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<ContextValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<ContextValue<String>>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ContextValue<String>>,
}

impl RepoCommitsContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.as_ref().and_then(ContextValue::literal) == Some(&0) {
            return Err("`commits.limit` must be a positive integer".to_string());
        }
        if let Some(limit) = self.limit.as_ref().and_then(ContextValue::default_value)
            && *limit == 0
        {
            return Err("`commits.limit.default` must be a positive integer".to_string());
        }

        if let Some(limit) = &self.limit {
            limit.validate_source("commits.limit")?;
        }
        if let Some(path) = &self.path {
            path.validate_source("commits.path")?;
        }
        if let Some(author) = &self.author {
            author.validate_source("commits.author")?;
        }
        if let Some(reference) = &self.reference {
            reference.validate_source("commits.ref")?;
        }

        for (field, value) in [
            ("path", self.path.as_ref().and_then(ContextValue::literal)),
            (
                "author",
                self.author.as_ref().and_then(ContextValue::literal),
            ),
            (
                "ref",
                self.reference.as_ref().and_then(ContextValue::literal),
            ),
        ] {
            if value.is_some_and(|value| value.is_empty()) {
                return Err(format!("`commits.{field}` must be a non-empty string"));
            }
        }

        for (field, value) in [
            (
                "path",
                self.path.as_ref().and_then(ContextValue::default_value),
            ),
            (
                "author",
                self.author.as_ref().and_then(ContextValue::default_value),
            ),
            (
                "ref",
                self.reference
                    .as_ref()
                    .and_then(ContextValue::default_value),
            ),
        ] {
            if value.is_some_and(|value| value.is_empty()) {
                return Err(format!(
                    "`commits.{field}.default` must be a non-empty string"
                ));
            }
        }

        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.as_ref().map(render_param("limit")),
            self.path.as_ref().map(render_param("path")),
            self.author.as_ref().map(render_param("author")),
            self.reference.as_ref().map(render_param("ref")),
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
        let author = self
            .author
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or_else(|| "octocat".to_string());
        let path = self.path.as_ref().and_then(ContextValue::sample_or_default);
        let reference = self
            .reference
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or_else(|| seed.default_branch.clone());

        Value::Array(
            (0..count)
                .map(|index| {
                    serde_json::json!({
                        "sha": format!("{:040x}", index + 1),
                        "author": author.clone(),
                        "message": format!("Sample commit {} for {}", index + 1, seed.full_name),
                        "path": path.clone(),
                        "ref": reference.clone(),
                    })
                })
                .collect(),
        )
    }

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            limit: resolve_optional_context_value(&self.limit, runtime, "commits.limit")?
                .map(ContextValue::from),
            path: resolve_optional_context_value(&self.path, runtime, "commits.path")?
                .map(ContextValue::from),
            author: resolve_optional_context_value(&self.author, runtime, "commits.author")?
                .map(ContextValue::from),
            reference: resolve_optional_context_value(&self.reference, runtime, "commits.ref")?
                .map(ContextValue::from),
        })
    }

    pub async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
    ) -> Result<Value> {
        let rows = client
            .fetch_repo_commits(
                &repo.owner,
                &repo.name,
                &RepoCommitsQuery {
                    limit: self
                        .limit
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .copied()
                        .map(|value| value.clamp(1, 100) as u8),
                    path: self.path.as_ref().and_then(ContextValue::literal).cloned(),
                    author: self
                        .author
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .cloned(),
                    reference: self
                        .reference
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .cloned(),
                },
            )
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}

pub struct CommitsResolver;
pub static RESOLVER: CommitsResolver = CommitsResolver;

#[async_trait]
impl RepoContextResolver for CommitsResolver {
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
        runtime: &DynamicContextData<'_>,
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
) -> std::result::Result<RepoCommitsContext, String> {
    serde_json::from_value(Value::Object(params.clone())).map_err(|error| error.to_string())
}

fn to_params(context: &RepoCommitsContext) -> serde_json::Map<String, serde_json::Value> {
    serde_json::to_value(context)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn spec(name: Option<&str>, context: &RepoCommitsContext) -> ContextSpec {
    ContextSpec {
        name: name.map(ToString::to_string),
        kind: KIND.to_string(),
        params: to_params(context),
    }
}

fn render_param<T: std::fmt::Display>(name: &str) -> impl FnOnce(&ContextValue<T>) -> String + '_ {
    move |value| match value {
        ContextValue::Literal(value) => format!("{name}={value}"),
        ContextValue::Ref(reference) => format!("{name}<-{}", reference.from),
    }
}
