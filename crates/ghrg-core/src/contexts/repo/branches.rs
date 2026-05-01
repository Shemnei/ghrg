use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{
    ContextSpec, ContextValue, DynamicContextData, resolve_optional_context_value,
};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, RepoContextResolver, SampleRepoSeed};

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
    spec(
        Some("protected_branches"),
        &RepoBranchesContext {
            limit: Some(3.into()),
            protected: Some(true.into()),
        },
    )
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    spec(
        Some(KIND),
        &RepoBranchesContext {
            limit: Some(5.into()),
            protected: Some(true.into()),
        },
    )
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

    pub async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
    ) -> Result<Value> {
        let rows = client
            .fetch_repo_branches(
                &repo.owner,
                &repo.name,
                &RepoBranchesQuery {
                    limit: self
                        .limit
                        .as_ref()
                        .and_then(ContextValue::literal)
                        .copied()
                        .map(|value| value.clamp(1, 100) as u8),
                    protected: self
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

pub struct BranchesResolver;
pub static RESOLVER: BranchesResolver = BranchesResolver;

#[async_trait]
impl RepoContextResolver for BranchesResolver {
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
) -> std::result::Result<RepoBranchesContext, String> {
    serde_json::from_value(Value::Object(params.clone())).map_err(|error| error.to_string())
}

fn to_params(context: &RepoBranchesContext) -> serde_json::Map<String, serde_json::Value> {
    serde_json::to_value(context)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn spec(name: Option<&str>, context: &RepoBranchesContext) -> ContextSpec {
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
