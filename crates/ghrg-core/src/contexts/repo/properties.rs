use crate::contexts::{ContextSpec, ContextValue, DynamicContextData};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, RepoContextResolver, SampleRepoSeed};

pub const KIND: &str = "properties";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch selected custom repository properties by name",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "names",
            description: "List of custom repository property names to fetch",
            required: true,
        },
    ],
    validation_rules: &["`names` must contain at least one non-empty string"],
    example_rego: "input.contexts.repo_properties.Team",
    performance_note: "Usually cheap; good for ownership or reporting enrichments.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(_default_branch: &str) -> ContextSpec {
    spec(
        Some("repo_properties"),
        &RepoPropertiesContext {
            names: vec!["Team".to_string(), "CodeOwner".to_string()].into(),
        },
    )
}

pub fn explicit_spec(_default_branch: &str) -> ContextSpec {
    spec(
        Some(KIND),
        &RepoPropertiesContext {
            names: vec!["Team".to_string(), "CodeOwner".to_string()].into(),
        },
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoPropertiesContext {
    pub names: ContextValue<Vec<String>>,
}

impl RepoPropertiesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        self.names.validate_source("properties.names")?;

        if let Some(names) = self.names.literal()
            && (names.is_empty() || names.iter().any(|value| value.is_empty()))
        {
            return Err(
                "`properties.names` must contain at least one non-empty string".to_string(),
            );
        }

        if let Some(default) = self.names.default_value()
            && (default.is_empty() || default.iter().any(|value| value.is_empty()))
        {
            return Err(
                "`properties.names.default` must contain at least one non-empty string".to_string(),
            );
        }

        Ok(())
    }

    pub fn render_params(&self) -> String {
        match &self.names {
            ContextValue::Literal(names) if names.is_empty() => String::new(),
            ContextValue::Literal(names) => {
                format!("names={}", serde_json::to_string(names).unwrap_or_default())
            }
            ContextValue::Ref(reference) => format!("names<-{}", reference.from),
        }
    }

    pub fn sample_value(&self, _seed: &SampleRepoSeed) -> Value {
        let names = self
            .names
            .sample_or_default()
            .unwrap_or_else(|| vec!["Team".to_string(), "CodeOwner".to_string()]);

        Value::Object(Map::from_iter(names.iter().map(|name| {
            (
                name.clone(),
                match name.as_str() {
                    "Team" => Value::String("platform".to_string()),
                    "CodeOwner" => Value::String("@example/platform".to_string()),
                    "CostCenter" => Value::String("ENG-001".to_string()),
                    _ => Value::String(format!("example-{}", name.to_ascii_lowercase())),
                },
            )
        })))
    }

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            names: self.names.resolve(runtime, "properties.names")?.into(),
        })
    }

    pub async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
    ) -> Result<Value> {
        let names = self.names.literal().cloned().unwrap_or_default();
        client
            .fetch_repo_properties(&repo.owner, &repo.name, &names.into_iter().collect())
            .await
            .map(Value::Object)
    }
}

pub struct PropertiesResolver;
pub static RESOLVER: PropertiesResolver = PropertiesResolver;

#[async_trait]
impl RepoContextResolver for PropertiesResolver {
    fn validate_params(&self, params: &Map<String, Value>) -> std::result::Result<(), String> {
        parse_params(params)?.validate()
    }

    fn render_params(&self, params: &Map<String, Value>) -> std::result::Result<String, String> {
        Ok(parse_params(params)?.render_params())
    }

    fn sample_value(
        &self,
        params: &Map<String, Value>,
        seed: &SampleRepoSeed,
    ) -> std::result::Result<Value, String> {
        Ok(parse_params(params)?.sample_value(seed))
    }

    fn resolve_dynamic(
        &self,
        params: &Map<String, Value>,
        runtime: &DynamicContextData<'_>,
    ) -> std::result::Result<Map<String, Value>, crate::error::GhrgError> {
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
        params: &Map<String, Value>,
    ) -> std::result::Result<Value, crate::error::GhrgError> {
        let context = parse_params(params).map_err(|details| {
            crate::error::GhrgError::InvalidContextParams {
                kind: KIND.to_string(),
                details,
            }
        })?;
        context.resolve(client, repo).await
    }
}

fn parse_params(params: &Map<String, Value>) -> std::result::Result<RepoPropertiesContext, String> {
    serde_json::from_value(Value::Object(params.clone())).map_err(|error| error.to_string())
}

fn to_params(context: &RepoPropertiesContext) -> Map<String, Value> {
    serde_json::to_value(context)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn spec(name: Option<&str>, context: &RepoPropertiesContext) -> ContextSpec {
    ContextSpec {
        name: name.map(ToString::to_string),
        kind: KIND.to_string(),
        params: to_params(context),
    }
}
