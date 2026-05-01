use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{
    ContextSpec, ContextValue, DynamicContextData, resolve_optional_context_value,
};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, RepoContextResolver, SampleRepoSeed};

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
    spec(
        Some("top_contributors"),
        &RepoContributorsContext {
            limit: Some(5.into()),
            anonymous: Some(false.into()),
        },
    )
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    spec(
        Some(KIND),
        &RepoContributorsContext {
            limit: Some(10.into()),
            anonymous: Some(false.into()),
        },
    )
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
    pub limit: Option<ContextValue<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous: Option<ContextValue<bool>>,
}

impl RepoContributorsContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.limit.as_ref().and_then(ContextValue::literal) == Some(&0) {
            return Err("`contributors.limit` must be a positive integer".to_string());
        }
        if let Some(limit) = self.limit.as_ref().and_then(ContextValue::default_value)
            && *limit == 0
        {
            return Err("`contributors.limit.default` must be a positive integer".to_string());
        }

        if let Some(limit) = &self.limit {
            limit.validate_source("contributors.limit")?;
        }
        if let Some(anonymous) = &self.anonymous {
            anonymous.validate_source("contributors.anonymous")?;
        }

        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.limit.as_ref().map(render_param("limit")),
            self.anonymous.as_ref().map(render_param("anonymous")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
    pub fn sample_value(&self, _seed: &SampleRepoSeed) -> Value {
        let count = self
            .limit
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or(3)
            .clamp(1, 5) as usize;
        let include_anonymous = self
            .anonymous
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or(false);

        Value::Array(
            (0..count)
                .map(|index| {
                    let anonymous = include_anonymous && index + 1 == count;
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

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            limit: resolve_optional_context_value(&self.limit, runtime, "contributors.limit")?
                .map(ContextValue::from),
            anonymous: resolve_optional_context_value(
                &self.anonymous,
                runtime,
                "contributors.anonymous",
            )?
            .map(ContextValue::from),
        })
    }

    pub async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
    ) -> Result<Value> {
        let rows = client
            .fetch_repo_contributors(
                &repo.owner,
                &repo.name,
                &RepoContributorsQuery {
                    limit: self
                        .limit
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .copied()
                        .map(|value| value.clamp(1, 100) as u16),
                    anonymous: self
                        .anonymous
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .copied()
                        .unwrap_or(false),
                },
            )
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}

pub struct ContributorsResolver;
pub static RESOLVER: ContributorsResolver = ContributorsResolver;

#[async_trait]
impl RepoContextResolver for ContributorsResolver {
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

fn render_param<T: std::fmt::Display>(name: &str) -> impl FnOnce(&ContextValue<T>) -> String + '_ {
    move |value| match value {
        ContextValue::Literal(value) => format!("{name}={value}"),
        ContextValue::Ref(reference) => format!("{name}<-{}", reference.from),
    }
}

fn parse_params(
    params: &serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<RepoContributorsContext, String> {
    serde_json::from_value(Value::Object(params.clone())).map_err(|error| error.to_string())
}

fn to_params(context: &RepoContributorsContext) -> serde_json::Map<String, serde_json::Value> {
    serde_json::to_value(context)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn spec(name: Option<&str>, context: &RepoContributorsContext) -> ContextSpec {
    ContextSpec {
        name: name.map(ToString::to_string),
        kind: KIND.to_string(),
        params: to_params(context),
    }
}
