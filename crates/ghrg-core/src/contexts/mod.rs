pub mod repo;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{GhrgError, Result};
use crate::github::RepositoryBase;

macro_rules! dispatch_provider {
    ($provider:expr, $method:ident $(, $arg:expr )* ) => {
        match $provider {
            ContextProvider::Properties(v) => v.$method($($arg),*),
            ContextProvider::Branches(v) => v.$method($($arg),*),
            ContextProvider::Commits(v) => v.$method($($arg),*),
            ContextProvider::Languages(v) => v.$method($($arg),*),
            ContextProvider::Files(v) => v.$method($($arg),*),
            ContextProvider::Contributors(v) => v.$method($($arg),*),
            ContextProvider::WorkflowRuns(v) => v.$method($($arg),*),
        }
    };
}

macro_rules! dispatch_provider_async {
    ($provider:expr, $method:ident $(, $arg:expr )* ) => {
        match $provider {
            ContextProvider::Properties(v) => v.$method($($arg),*).await,
            ContextProvider::Branches(v) => v.$method($($arg),*).await,
            ContextProvider::Commits(v) => v.$method($($arg),*).await,
            ContextProvider::Languages(v) => v.$method($($arg),*).await,
            ContextProvider::Files(v) => v.$method($($arg),*).await,
            ContextProvider::Contributors(v) => v.$method($($arg),*).await,
            ContextProvider::WorkflowRuns(v) => v.$method($($arg),*).await,
        }
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContextBase {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: serde::Deserialize<'de>"))]
pub enum ContextValue<T> {
    Literal(T),
    Ref(ContextValueRef<T>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: serde::Deserialize<'de>"))]
pub struct ContextValueRef<T> {
    pub from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<T>,
}

#[derive(Debug, Clone)]
pub struct DynamicContextData<'a> {
    input: &'a Value,
    meta_last: Option<&'a Value>,
    meta_policies: &'a Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
#[serde(deny_unknown_fields)]
pub enum ContextProvider {
    #[serde(rename = "properties")]
    Properties(repo::properties::RepoPropertiesContext),
    #[serde(rename = "branches")]
    Branches(repo::branches::RepoBranchesContext),
    #[serde(rename = "commits")]
    Commits(repo::commits::RepoCommitsContext),
    #[serde(rename = "languages")]
    Languages(repo::languages::RepoLanguagesContext),
    #[serde(rename = "files")]
    Files(repo::files::RepoFilesContext),
    #[serde(rename = "contributors")]
    Contributors(repo::contributors::RepoContributorsContext),
    #[serde(rename = "workflow_runs")]
    WorkflowRuns(repo::workflow_runs::RepoWorkflowRunsContext),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextSpec {
    #[serde(flatten)]
    pub base: ContextBase,
    #[serde(flatten)]
    pub provider: ContextProvider,
}

impl<'a> DynamicContextData<'a> {
    pub fn new(
        input: &'a Value,
        meta_last: Option<&'a Value>,
        meta_policies: &'a Map<String, Value>,
    ) -> Self {
        Self {
            input,
            meta_last,
            meta_policies,
        }
    }

    pub fn resolve_source(&self, source: &str) -> Option<Value> {
        if source == "input" {
            return Some(self.input.clone());
        }
        if let Some(path) = source.strip_prefix("input.") {
            return value_at_path(self.input, path).cloned();
        }

        if let Some(key) = source.strip_prefix("env.") {
            if key.is_empty() {
                return None;
            }
            return std::env::var_os(key)
                .map(|value| Value::String(value.to_string_lossy().into_owned()));
        }

        if source == "meta.last" {
            return self.meta_last.cloned();
        }
        if let Some(path) = source.strip_prefix("meta.last.") {
            return self
                .meta_last
                .and_then(|value| value_at_path(value, path).cloned());
        }

        if source == "meta.policies" {
            return Some(Value::Object(self.meta_policies.clone()));
        }

        let Some(remainder) = source.strip_prefix("meta.policies.") else {
            return None;
        };
        if remainder.is_empty() {
            return Some(Value::Object(self.meta_policies.clone()));
        }

        if let Some(value) = self.meta_policies.get(remainder) {
            return Some(value.clone());
        }

        let mut keys = self.meta_policies.keys().collect::<Vec<_>>();
        keys.sort_by_key(|key| std::cmp::Reverse(key.len()));

        for key in keys {
            if remainder == key {
                return self.meta_policies.get(key).cloned();
            }

            let Some(path) = remainder
                .strip_prefix(key.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
            else {
                continue;
            };

            if let Some(value) = self
                .meta_policies
                .get(key)
                .and_then(|value| value_at_path(value, path))
            {
                return Some(value.clone());
            }
        }

        None
    }
}

impl<T> ContextValue<T> {
    pub fn validate_source(&self, field: &str) -> std::result::Result<(), String> {
        if let Self::Ref(reference) = self {
            validate_dynamic_source(field, &reference.from)?;
        }
        Ok(())
    }

    pub fn literal(&self) -> Option<&T> {
        match self {
            Self::Literal(value) => Some(value),
            Self::Ref(_) => None,
        }
    }

    pub fn default_value(&self) -> Option<&T> {
        match self {
            Self::Literal(_) => None,
            Self::Ref(reference) => reference.default.as_ref(),
        }
    }

    pub fn sample_or_default(&self) -> Option<T>
    where
        T: Clone,
    {
        match self {
            Self::Literal(value) => Some(value.clone()),
            Self::Ref(reference) => reference.default.clone(),
        }
    }

    pub fn resolve(&self, runtime: &DynamicContextData<'_>, field: &str) -> Result<T>
    where
        T: Clone + DeserializeOwned + Serialize,
    {
        match self {
            Self::Literal(value) => Ok(value.clone()),
            Self::Ref(reference) => {
                let from = reference.from.clone();
                let raw_value = runtime.resolve_source(&reference.from);

                let value = match raw_value {
                    Some(value) => value,
                    None => {
                        if let Some(default) = reference.default.as_ref() {
                            serde_json::to_value(default).map_err(GhrgError::from)?
                        } else {
                            return Err(GhrgError::ContextDynamicSourceMissing {
                                field: field.to_string(),
                                source_path: from,
                            });
                        }
                    }
                };

                match serde_json::from_value::<T>(value.clone()) {
                    Ok(value) => Ok(value),
                    Err(source) => {
                        if let Value::String(text) = &value
                            && let Ok(parsed) = serde_json::from_str::<T>(text)
                        {
                            return Ok(parsed);
                        }

                        Err(GhrgError::ContextDynamicTypeMismatch {
                            field: field.to_string(),
                            source_path: from,
                            value,
                            details: source.to_string(),
                        })
                    }
                }
            }
        }
    }
}

impl<T> From<T> for ContextValue<T> {
    fn from(value: T) -> Self {
        Self::Literal(value)
    }
}

impl<T> Default for ContextValue<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::Literal(T::default())
    }
}

impl ContextSpec {
    pub fn input_key(&self) -> &str {
        self.base.name.as_deref().unwrap_or(self.kind())
    }

    pub fn kind(&self) -> &str {
        self.provider.kind()
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        self.provider.validate()
    }

    pub fn render(&self) -> String {
        let label = if let Some(name) = &self.base.name {
            format!("{} as {}", self.kind(), name)
        } else {
            self.kind().to_string()
        };
        let params = self.provider.render_params();
        if params.is_empty() {
            label
        } else {
            format!("{label}({params})")
        }
    }

    pub fn sample_value(&self, seed: &repo::SampleRepoSeed) -> serde_json::Value {
        self.provider.sample_value(seed)
    }

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            base: self.base.clone(),
            provider: self.provider.resolve_dynamic(runtime)?,
        })
    }

    pub async fn resolve_for_repo<T>(
        &self,
        client: &T,
        repo: &RepositoryBase,
    ) -> Result<serde_json::Value>
    where
        T: repo::ResolveRepoContext + Sync,
    {
        self.provider.resolve_for_repo(client, repo).await
    }
}

impl ContextProvider {
    pub fn kind(&self) -> &str {
        match self {
            ContextProvider::Properties(_) => repo::properties::KIND,
            ContextProvider::Branches(_) => repo::branches::KIND,
            ContextProvider::Commits(_) => repo::commits::KIND,
            ContextProvider::Languages(_) => repo::languages::KIND,
            ContextProvider::Files(_) => repo::files::KIND,
            ContextProvider::Contributors(_) => repo::contributors::KIND,
            ContextProvider::WorkflowRuns(_) => repo::workflow_runs::KIND,
        }
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        dispatch_provider!(self, validate)
    }

    pub fn render_params(&self) -> String {
        dispatch_provider!(self, render_params)
    }

    pub fn sample_value(&self, seed: &repo::SampleRepoSeed) -> serde_json::Value {
        dispatch_provider!(self, sample_value, seed)
    }

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        match self {
            ContextProvider::Properties(v) => {
                Ok(ContextProvider::Properties(v.resolve_dynamic(runtime)?))
            }
            ContextProvider::Branches(v) => {
                Ok(ContextProvider::Branches(v.resolve_dynamic(runtime)?))
            }
            ContextProvider::Commits(v) => {
                Ok(ContextProvider::Commits(v.resolve_dynamic(runtime)?))
            }
            ContextProvider::Languages(v) => {
                Ok(ContextProvider::Languages(v.resolve_dynamic(runtime)?))
            }
            ContextProvider::Files(v) => Ok(ContextProvider::Files(v.resolve_dynamic(runtime)?)),
            ContextProvider::Contributors(v) => {
                Ok(ContextProvider::Contributors(v.resolve_dynamic(runtime)?))
            }
            ContextProvider::WorkflowRuns(v) => {
                Ok(ContextProvider::WorkflowRuns(v.resolve_dynamic(runtime)?))
            }
        }
    }

    pub async fn resolve_for_repo<T>(
        &self,
        client: &T,
        repo: &RepositoryBase,
    ) -> Result<serde_json::Value>
    where
        T: repo::ResolveRepoContext + Sync,
    {
        dispatch_provider_async!(self, resolve, client, repo)
    }
}

pub fn resolve_optional_context_value<T>(
    value: &Option<ContextValue<T>>,
    runtime: &DynamicContextData<'_>,
    field: &str,
) -> Result<Option<T>>
where
    T: Clone + DeserializeOwned + Serialize,
{
    value
        .as_ref()
        .map(|value| value.resolve(runtime, field))
        .transpose()
}

fn validate_dynamic_source(field: &str, source: &str) -> std::result::Result<(), String> {
    if source.is_empty() {
        return Err(format!("`{field}.from` must be a non-empty string"));
    }

    if source == "input"
        || source.starts_with("input.")
        || source.starts_with("env.")
        || source == "meta.last"
        || source.starts_with("meta.last.")
        || source == "meta.policies"
        || source.starts_with("meta.policies.")
    {
        return Ok(());
    }

    Err(format!(
        "`{field}.from` must start with `input`, `env.`, `meta.last`, or `meta.policies`"
    ))
}

fn value_at_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }

    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            return None;
        }
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(list) => {
                let index = segment.parse::<usize>().ok()?;
                list.get(index)?
            }
            _ => return None,
        };
    }

    Some(current)
}
