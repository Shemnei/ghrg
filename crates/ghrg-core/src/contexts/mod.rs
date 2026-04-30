pub mod repo;

use serde::{Deserialize, Serialize};

use crate::error::Result;
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
