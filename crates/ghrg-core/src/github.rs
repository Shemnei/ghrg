use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use jsonwebtoken::EncodingKey;
use octocrab::{
    Octocrab, Page,
    models::{AppId, Contributor, InstallationId, Repository, repos::RepoCommit},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

use crate::contexts::repo::branches::RepoBranchesQuery;
use crate::contexts::repo::commits::RepoCommitsQuery;
use crate::contexts::repo::contributors::RepoContributorsQuery;
use crate::contexts::repo::files::RepoFilesQuery;
use crate::contexts::repo::workflow_runs::RepoWorkflowRunsQuery;
use crate::error::{GhrgError, Result};

#[derive(Debug, Clone)]
pub enum GitHubCredentials {
    None,
    PersonalToken(String),
    AppInstallation(GitHubAppCredentials),
}

#[derive(Debug, Clone)]
pub struct GitHubAppCredentials {
    pub app_id: u64,
    pub installation_id: u64,
    pub private_key: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub enum RepoScope<'a> {
    Repo(&'a str),
    Org(&'a str),
    User(&'a str),
    Owner(&'a str),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepositoryBase {
    pub name: String,
    pub owner: String,
    pub full_name: String,
    pub archived: bool,
    pub fork: bool,
    pub visibility: String,
    pub default_branch: String,
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub github: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepositoryPolicyInput {
    #[serde(flatten)]
    pub base: RepositoryBase,
    #[serde(default)]
    pub contexts: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoBranch {
    pub name: String,
    pub protected: bool,
    pub sha: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoContributor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(rename = "type")]
    pub contributor_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub contributions: u64,
    pub anonymous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoFileEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    pub reference: String,
    pub glob: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoCommitEntry {
    pub sha: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_login: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoWorkflowRunEntry {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conclusion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_branch: Option<String>,
    pub head_sha: String,
    pub run_number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_attempt: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_login: Option<String>,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
}

impl RepositoryBase {
    pub fn into_policy_input(self) -> RepositoryPolicyInput {
        RepositoryPolicyInput {
            base: self,
            contexts: Map::new(),
        }
    }
}

impl RepositoryPolicyInput {
    pub fn with_contexts(mut self, contexts: Map<String, Value>) -> Self {
        self.contexts = contexts;
        self
    }
}

#[async_trait]
pub trait RepoDataSource: Clone + Send + Sync {
    async fn fetch_repo(&self, owner: &str, name: &str) -> Result<RepositoryBase>;
    async fn list_repos(
        &self,
        scope: RepoScope<'_>,
        limit: Option<usize>,
    ) -> Result<Vec<RepositoryBase>>;
    async fn fetch_repo_properties(
        &self,
        owner: &str,
        repo: &str,
        names: &BTreeSet<String>,
    ) -> Result<Map<String, Value>>;
    async fn fetch_repo_languages(&self, owner: &str, repo: &str) -> Result<Map<String, Value>>;
    async fn fetch_repo_branches(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoBranchesQuery,
    ) -> Result<Vec<RepoBranch>>;
    async fn fetch_repo_commits(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoCommitsQuery,
    ) -> Result<Vec<RepoCommitEntry>>;
    async fn fetch_repo_files(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoFilesQuery,
    ) -> Result<Vec<RepoFileEntry>>;
    async fn fetch_repo_contributors(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoContributorsQuery,
    ) -> Result<Vec<RepoContributor>>;
    async fn fetch_repo_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoWorkflowRunsQuery,
    ) -> Result<Vec<RepoWorkflowRunEntry>>;
}

#[derive(Clone)]
pub struct GitHubClient {
    client: Octocrab,
}

impl GitHubClient {
    pub fn new(credentials: GitHubCredentials) -> Result<Self> {
        let client = build_octocrab(credentials)?;

        Ok(Self { client })
    }

    async fn list_org_repos(&self, org: &str, limit: Option<usize>) -> Result<Vec<RepositoryBase>> {
        let page = self
            .client
            .orgs(org)
            .list_repos()
            .per_page(100)
            .send()
            .await;
        let page = github_request_result(page, format!("list repositories for org {org}"))?;
        self.collect_repo_pages(page, limit, format!("list repositories for org {org}"))
            .await
    }

    async fn list_user_repos(
        &self,
        user: &str,
        limit: Option<usize>,
    ) -> Result<Vec<RepositoryBase>> {
        let page = self.client.users(user).repos().per_page(100).send().await;
        let page = github_request_result(page, format!("list repositories for user {user}"))?;
        self.collect_repo_pages(page, limit, format!("list repositories for user {user}"))
            .await
    }

    async fn list_owner_repos(
        &self,
        owner: &str,
        limit: Option<usize>,
    ) -> Result<Vec<RepositoryBase>> {
        match self.list_org_repos(owner, limit).await {
            Ok(repos) => Ok(repos),
            Err(error) if is_not_found(&error) => self.list_user_repos(owner, limit).await,
            Err(error) => Err(error),
        }
    }

    async fn collect_repo_pages(
        &self,
        mut page: Page<Repository>,
        limit: Option<usize>,
        operation: String,
    ) -> Result<Vec<RepositoryBase>> {
        let mut repos = Vec::new();

        loop {
            repos.extend(
                page.items
                    .into_iter()
                    .map(|repo| normalize_repo(repo, "", "")),
            );
            if let Some(limit) = limit
                && repos.len() >= limit
            {
                repos.truncate(limit);
                return Ok(repos);
            }

            let Some(next) = self
                .client
                .get_page::<Repository>(&page.next)
                .await
                .map_err(|source| {
                    github_request_error(format!("{operation} (next page)"), source)
                })?
            else {
                return Ok(repos);
            };
            page = next;
        }
    }

    async fn fetch_repo_blob_content(&self, owner: &str, repo: &str, sha: &str) -> Result<String> {
        let route = format!("/repos/{owner}/{repo}/git/blobs/{sha}");
        let blob = github_request_result(
            self.client
                .get::<RepoBlobResponse, _, _>(route, None::<&()>)
                .await,
            format!("fetch blob {sha} for repository {owner}/{repo}"),
        )?;

        decode_repo_blob_content(blob, owner, repo, sha)
    }
}

#[async_trait]
impl RepoDataSource for GitHubClient {
    async fn fetch_repo(&self, owner: &str, name: &str) -> Result<RepositoryBase> {
        let repo = self.client.repos(owner, name).get().await;
        let repo = github_request_result(repo, format!("fetch repository {owner}/{name}"))?;

        Ok(normalize_repo(repo, owner, name))
    }

    async fn list_repos(
        &self,
        scope: RepoScope<'_>,
        limit: Option<usize>,
    ) -> Result<Vec<RepositoryBase>> {
        match scope {
            RepoScope::Repo(repo) => {
                let (owner, name) = parse_repo_slug(repo)?;
                Ok(vec![self.fetch_repo(owner, name).await?])
            }
            RepoScope::Org(org) => self.list_org_repos(org, limit).await,
            RepoScope::User(user) => self.list_user_repos(user, limit).await,
            RepoScope::Owner(owner) => self.list_owner_repos(owner, limit).await,
        }
    }

    async fn fetch_repo_properties(
        &self,
        owner: &str,
        repo: &str,
        names: &BTreeSet<String>,
    ) -> Result<Map<String, Value>> {
        let route = format!("/repos/{owner}/{repo}/properties/values");
        let values = self
            .client
            .get::<Vec<RepoPropertyValue>, _, _>(route, None::<&()>)
            .await;
        let values = match github_request_result(
            values,
            format!("fetch custom properties for repository {owner}/{repo}"),
        ) {
            Ok(values) => values,
            Err(error) if is_not_found(&error) => {
                return Ok(empty_repo_properties(names));
            }
            Err(error) => return Err(error),
        };

        let mut properties = Map::new();
        for entry in values {
            if !names.is_empty() && !names.contains(&entry.name) {
                continue;
            }
            properties.insert(entry.name, entry.value.unwrap_or(Value::Null));
        }

        for name in names {
            properties.entry(name.clone()).or_insert(Value::Null);
        }

        Ok(properties)
    }

    async fn fetch_repo_languages(&self, owner: &str, repo: &str) -> Result<Map<String, Value>> {
        let values = self.client.repos(owner, repo).list_languages().await;
        let values = github_request_result(
            values,
            format!("fetch languages for repository {owner}/{repo}"),
        )?;

        Ok(Map::from_iter(
            values
                .into_iter()
                .map(|(language, bytes)| (language, Value::from(bytes))),
        ))
    }

    async fn fetch_repo_commits(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoCommitsQuery,
    ) -> Result<Vec<RepoCommitEntry>> {
        let limit = query.limit.unwrap_or(10).clamp(1, 100);

        let repo_handler = self.client.repos(owner, repo);
        let mut builder = repo_handler.list_commits().per_page(limit);

        if let Some(path) = query.path.as_deref() {
            builder = builder.path(path);
        }
        if let Some(author) = query.author.as_deref() {
            builder = builder.author(author);
        }
        if let Some(reference) = query.reference.as_deref() {
            builder = builder.branch(reference);
        }

        let page = github_request_result(
            builder.send().await,
            format!("fetch commits for repository {owner}/{repo}"),
        )?;

        Ok(page
            .items
            .into_iter()
            .take(limit as usize)
            .map(normalize_commit)
            .collect())
    }

    async fn fetch_repo_branches(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoBranchesQuery,
    ) -> Result<Vec<RepoBranch>> {
        let limit = query.limit.unwrap_or(30).clamp(1, 100) as usize;
        let route = format!("/repos/{owner}/{repo}/branches");
        let branches = github_request_result(
            self.client
                .get::<Vec<RepoBranchResponse>, _, _>(
                    route,
                    Some(&serde_json::json!({
                        "protected": query.protected,
                        "per_page": limit,
                    })),
                )
                .await,
            format!("fetch branches for repository {owner}/{repo}"),
        )?;

        Ok(branches
            .into_iter()
            .take(limit)
            .map(normalize_branch)
            .collect())
    }

    async fn fetch_repo_files(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoFilesQuery,
    ) -> Result<Vec<RepoFileEntry>> {
        let reference = query.reference.as_deref().unwrap_or("HEAD");
        let route = format!("/repos/{owner}/{repo}/git/trees/{reference}");
        let tree = github_request_result(
            self.client
                .get::<RepoTreeResponse, _, _>(
                    route,
                    Some(&serde_json::json!({ "recursive": "1" })),
                )
                .await,
            format!("fetch file tree for repository {owner}/{repo}"),
        )?;

        let limit = query.limit.unwrap_or(100).clamp(1, 500) as usize;
        let matcher = compile_files_matcher(query.glob.as_deref())?;

        let mut entries = tree
            .tree
            .into_iter()
            .filter(|entry| matcher.is_match(&entry.path))
            .take(limit)
            .map(|entry| {
                normalize_file_entry(entry, reference, query.glob.as_deref().unwrap_or("**"))
            })
            .collect::<Vec<_>>();

        if query.include_content {
            for entry in &mut entries {
                if entry.entry_type != "blob" {
                    continue;
                }

                let Some(sha) = entry.sha.as_deref() else {
                    continue;
                };

                entry.content = Some(self.fetch_repo_blob_content(owner, repo, sha).await?);
            }
        }

        Ok(entries)
    }

    async fn fetch_repo_contributors(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoContributorsQuery,
    ) -> Result<Vec<RepoContributor>> {
        let limit = query.limit.unwrap_or(100).clamp(1, 100) as usize;
        let page = github_request_result(
            self.client
                .repos(owner, repo)
                .list_contributors()
                .anon(query.anonymous)
                .per_page(limit.min(100) as u8)
                .send()
                .await,
            format!("fetch contributors for repository {owner}/{repo}"),
        )?;

        collect_contributor_pages(
            &self.client,
            page,
            limit,
            format!("fetch contributors for repository {owner}/{repo}"),
        )
        .await
    }

    async fn fetch_repo_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoWorkflowRunsQuery,
    ) -> Result<Vec<RepoWorkflowRunEntry>> {
        let limit = query.limit.unwrap_or(10).clamp(1, 100) as usize;
        let route = format!("/repos/{owner}/{repo}/actions/runs");
        let runs = github_request_result(
            self.client
                .get::<RepoWorkflowRunsResponse, _, _>(
                    route,
                    Some(&serde_json::json!({
                        "per_page": limit,
                        "branch": query.branch.as_deref(),
                        "event": query.event.as_deref(),
                        "status": query.status.as_deref(),
                        "actor": query.actor.as_deref(),
                    })),
                )
                .await,
            format!("fetch workflow runs for repository {owner}/{repo}"),
        )?;

        Ok(runs
            .workflow_runs
            .into_iter()
            .take(limit)
            .map(normalize_workflow_run)
            .collect())
    }
}

pub fn parse_repo_slug(repo: &str) -> Result<(&str, &str)> {
    let mut parts = repo.split('/');
    let Some(owner) = parts.next() else {
        return Err(GhrgError::InvalidRepositorySelector {
            value: repo.to_string(),
        });
    };
    let Some(name) = parts.next() else {
        return Err(GhrgError::InvalidRepositorySelector {
            value: repo.to_string(),
        });
    };
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return Err(GhrgError::InvalidRepositorySelector {
            value: repo.to_string(),
        });
    }
    Ok((owner, name))
}

fn normalize_repo(repo: Repository, fallback_owner: &str, fallback_name: &str) -> RepositoryBase {
    let github = serde_json::to_value(&repo)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let owner = repo
        .owner
        .map(|owner| owner.login)
        .unwrap_or_else(|| fallback_owner.to_string());
    let name = if repo.name.is_empty() {
        fallback_name.to_string()
    } else {
        repo.name
    };
    let full_name = repo.full_name.unwrap_or_else(|| format!("{owner}/{name}"));
    let visibility = repo
        .visibility
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            if repo.private.unwrap_or(false) {
                "private".to_string()
            } else {
                "public".to_string()
            }
        });

    RepositoryBase {
        name,
        owner,
        full_name,
        archived: repo.archived.unwrap_or(false),
        fork: repo.fork.unwrap_or(false),
        visibility,
        default_branch: repo.default_branch.unwrap_or_else(|| "main".to_string()),
        topics: repo.topics.unwrap_or_default(),
        github,
    }
}

#[derive(Debug, Deserialize)]
struct RepoPropertyValue {
    #[serde(rename = "property_name")]
    name: String,
    value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RepoBranchResponse {
    name: String,
    protected: bool,
    commit: RepoBranchCommit,
    #[serde(default)]
    _links: RepoBranchLinks,
}

#[derive(Debug, Deserialize)]
struct RepoBranchCommit {
    sha: String,
    url: String,
}

#[derive(Debug, Default, Deserialize)]
struct RepoBranchLinks {
    #[serde(default)]
    html: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepoTreeResponse {
    tree: Vec<RepoTreeEntry>,
}

#[derive(Debug, Deserialize)]
struct RepoBlobResponse {
    content: String,
    encoding: String,
}

#[derive(Debug, Deserialize)]
struct RepoTreeEntry {
    path: String,
    mode: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    sha: Option<String>,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RepoWorkflowRunsResponse {
    workflow_runs: Vec<RepoWorkflowRunResponse>,
}

#[derive(Debug, Deserialize)]
struct RepoWorkflowRunResponse {
    id: u64,
    name: Option<String>,
    event: String,
    status: Option<String>,
    conclusion: Option<String>,
    head_branch: Option<String>,
    head_sha: String,
    run_number: u64,
    run_attempt: Option<u64>,
    html_url: String,
    created_at: String,
    updated_at: String,
    actor: Option<RepoWorkflowRunActor>,
}

#[derive(Debug, Deserialize)]
struct RepoWorkflowRunActor {
    login: String,
}

fn compile_files_matcher(glob: Option<&str>) -> Result<globset::GlobMatcher> {
    let pattern = glob
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("**");

    globset::Glob::new(pattern)
        .map(|glob| glob.compile_matcher())
        .map_err(|error| GhrgError::GitHubClientBuild {
            message: format!("invalid files glob `{pattern}`"),
            details: error.to_string(),
        })
}

fn normalize_file_entry(entry: RepoTreeEntry, reference: &str, glob: &str) -> RepoFileEntry {
    let name = entry
        .path
        .rsplit('/')
        .next()
        .unwrap_or(entry.path.as_str())
        .to_string();

    RepoFileEntry {
        name,
        path: entry.path,
        entry_type: entry.kind,
        mode: entry.mode,
        sha: entry.sha,
        size: entry.size,
        reference: reference.to_string(),
        glob: glob.to_string(),
        content: None,
    }
}

fn decode_repo_blob_content(
    blob: RepoBlobResponse,
    owner: &str,
    repo: &str,
    sha: &str,
) -> Result<String> {
    if blob.encoding != "base64" {
        return Err(GhrgError::GitHubRequest {
            operation: format!("decode blob {sha} for repository {owner}/{repo}"),
            message: format!("unsupported blob encoding `{}`", blob.encoding),
            status: None,
            body: None,
            details: "expected GitHub blob responses to use base64 encoding".to_string(),
        });
    }

    let normalized = blob.content.lines().collect::<String>();
    let bytes = STANDARD
        .decode(normalized)
        .map_err(|error| GhrgError::GitHubRequest {
            operation: format!("decode blob {sha} for repository {owner}/{repo}"),
            message: "invalid base64 blob payload".to_string(),
            status: None,
            body: None,
            details: error.to_string(),
        })?;

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn normalize_branch(branch: RepoBranchResponse) -> RepoBranch {
    RepoBranch {
        name: branch.name,
        protected: branch.protected,
        sha: branch.commit.sha,
        url: branch.commit.url,
        html_url: branch._links.html,
    }
}

fn empty_repo_properties(names: &BTreeSet<String>) -> Map<String, Value> {
    Map::from_iter(names.iter().cloned().map(|name| (name, Value::Null)))
}

async fn collect_contributor_pages(
    client: &Octocrab,
    mut page: Page<Contributor>,
    limit: usize,
    operation: String,
) -> Result<Vec<RepoContributor>> {
    let mut contributors = Vec::new();

    loop {
        contributors.extend(page.items.into_iter().map(normalize_contributor));
        if contributors.len() >= limit {
            contributors.truncate(limit);
            return Ok(contributors);
        }

        let Some(next) = client
            .get_page::<Contributor>(&page.next)
            .await
            .map_err(|source| github_request_error(format!("{operation} (next page)"), source))?
        else {
            return Ok(contributors);
        };
        page = next;
    }
}

fn normalize_contributor(contributor: Contributor) -> RepoContributor {
    let anonymous = contributor.author.email.is_some();

    RepoContributor {
        login: Some(contributor.author.login),
        id: Some(contributor.author.id.0),
        contributor_type: contributor.author.r#type.to_string(),
        html_url: Some(contributor.author.html_url.to_string()),
        avatar_url: Some(contributor.author.avatar_url.to_string()),
        email: contributor.author.email,
        contributions: u64::from(contributor.contributions),
        anonymous,
    }
}

fn build_octocrab(credentials: GitHubCredentials) -> Result<Octocrab> {
    match credentials {
        GitHubCredentials::None => Octocrab::builder()
            .build()
            .map_err(github_client_build_error),
        GitHubCredentials::PersonalToken(token) => Octocrab::builder()
            .personal_token(token)
            .build()
            .map_err(github_client_build_error),
        GitHubCredentials::AppInstallation(auth) => {
            let key = parse_github_app_private_key(&auth.private_key)?;

            let app_client = Octocrab::builder()
                .app(AppId(auth.app_id), key)
                .build()
                .map_err(github_client_build_error)?;

            app_client
                .installation(InstallationId(auth.installation_id))
                .map_err(github_client_build_error)
        }
    }
}

fn parse_github_app_private_key(private_key: &[u8]) -> Result<EncodingKey> {
    if private_key.starts_with(b"-----BEGIN") {
        EncodingKey::from_rsa_pem(private_key).map_err(|error| GhrgError::GitHubAppKeyInvalid {
            message: error.to_string(),
        })
    } else {
        Ok(EncodingKey::from_rsa_der(private_key))
    }
}

fn github_client_build_error(source: octocrab::Error) -> GhrgError {
    GhrgError::GitHubClientBuild {
        message: source.to_string(),
        details: format!("{source:#?}"),
    }
}

fn github_request_result<T>(
    result: std::result::Result<T, octocrab::Error>,
    operation: impl Into<String>,
) -> Result<T> {
    result.map_err(|source| github_request_error(operation.into(), source))
}

fn github_request_error(operation: String, source: octocrab::Error) -> GhrgError {
    let (status, body) = github_error_status_and_body(&source);

    GhrgError::GitHubRequest {
        operation,
        message: format_github_error_message(status, body.as_deref(), &source),
        status,
        body,
        details: format!("{source:#?}"),
    }
}

fn is_not_found(error: &GhrgError) -> bool {
    matches!(error, GhrgError::GitHubRequest { status, message, details, .. } if status == &Some(404) || message.contains("404") || details.contains("404") || message.contains("Not Found") || details.contains("Not Found"))
}

fn normalize_commit(commit: RepoCommit) -> RepoCommitEntry {
    RepoCommitEntry {
        sha: commit.sha,
        message: commit.commit.message,
        committed_at: commit
            .commit
            .committer
            .and_then(|value| value.date)
            .map(|value| value.to_rfc3339()),
        author_login: commit.author.map(|value| value.login),
    }
}

fn normalize_workflow_run(run: RepoWorkflowRunResponse) -> RepoWorkflowRunEntry {
    RepoWorkflowRunEntry {
        id: run.id,
        name: run.name,
        event: run.event,
        status: run.status,
        conclusion: run.conclusion,
        head_branch: run.head_branch,
        head_sha: run.head_sha,
        run_number: run.run_number,
        run_attempt: run.run_attempt,
        actor_login: run.actor.map(|actor| actor.login),
        html_url: run.html_url,
        created_at: run.created_at,
        updated_at: run.updated_at,
    }
}

fn github_error_status_and_body(source: &octocrab::Error) -> (Option<u16>, Option<String>) {
    match source {
        octocrab::Error::GitHub { source, .. } => {
            let body = format_github_body(
                &source.message,
                source.errors.as_ref(),
                source.documentation_url.as_deref(),
            );
            (Some(source.status_code.as_u16()), Some(body))
        }
        _ => (None, None),
    }
}

fn format_github_error_message(
    status: Option<u16>,
    body: Option<&str>,
    source: &octocrab::Error,
) -> String {
    match (status, body) {
        (Some(status), Some(body)) => format!("GitHub API returned HTTP {status}: {body}"),
        (Some(status), None) => format!("GitHub API returned HTTP {status}: {source}"),
        (None, Some(body)) => body.to_string(),
        (None, None) => source.to_string(),
    }
}

fn format_github_body(
    message: &str,
    errors: Option<&Vec<Value>>,
    documentation_url: Option<&str>,
) -> String {
    let mut parts = vec![message.trim().to_string()];

    if let Some(errors) = errors.filter(|errors| !errors.is_empty()) {
        let joined = errors
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        parts.push(format!("errors: {joined}"));
    }

    if let Some(url) = documentation_url.filter(|url| !url.is_empty()) {
        parts.push(format!("docs: {url}"));
    }

    parts.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_repo_preserves_raw_github_fields_for_policies() {
        let repo: Repository = serde_json::from_value(serde_json::json!({
            "id": 1,
            "name": "api",
            "full_name": "acme/api",
            "private": false,
            "url": "https://api.github.com/repos/acme/api",
            "html_url": "https://github.com/acme/api",
            "archived": false,
            "fork": false,
            "visibility": "public",
            "default_branch": "main",
            "topics": ["governance"],
            "pushed_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-02-01T00:00:00Z"
        }))
        .expect("repository payload should deserialize");

        let normalized = normalize_repo(repo, "acme", "api");

        assert_eq!(normalized.owner, "acme");
        assert_eq!(
            normalized.github.get("pushed_at"),
            Some(&serde_json::json!("2024-01-01T00:00:00Z"))
        );
        assert_eq!(
            normalized.github.get("updated_at"),
            Some(&serde_json::json!("2024-02-01T00:00:00Z"))
        );
        assert_eq!(
            normalized.github.get("html_url"),
            Some(&serde_json::json!("https://github.com/acme/api"))
        );
    }

    #[test]
    fn empty_repo_properties_sets_requested_names_to_null() {
        let properties = empty_repo_properties(&BTreeSet::from([
            "CodeOwner".to_string(),
            "Team".to_string(),
        ]));

        assert_eq!(properties.get("CodeOwner"), Some(&Value::Null));
        assert_eq!(properties.get("Team"), Some(&Value::Null));
    }

    #[test]
    fn decode_repo_blob_content_decodes_wrapped_base64() {
        let blob = RepoBlobResponse {
            content: "eyJtaW5pbXVtUmVsZWFzZUFnZSI6ICIxNCBkYXlzIn0=\n".to_string(),
            encoding: "base64".to_string(),
        };

        let content = decode_repo_blob_content(blob, "acme", "api", "abc").unwrap();

        assert_eq!(content, r#"{"minimumReleaseAge": "14 days"}"#);
    }
}
