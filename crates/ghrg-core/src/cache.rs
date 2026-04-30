use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, trace};

use crate::contexts::repo::branches::RepoBranchesQuery;
use crate::contexts::repo::commits::RepoCommitsQuery;
use crate::contexts::repo::contributors::RepoContributorsQuery;
use crate::contexts::repo::files::RepoFilesQuery;
use crate::contexts::repo::workflow_runs::RepoWorkflowRunsQuery;
use crate::error::{GhrgError, Result};
use crate::github::{
    RepoBranch, RepoCommitEntry, RepoContributor, RepoDataSource, RepoFileEntry, RepoScope,
    RepoWorkflowRunEntry, RepositoryBase,
};

#[derive(Debug, Clone)]
pub struct CacheSettings {
    pub dir: PathBuf,
    pub disk_enabled: bool,
    pub ttl: Duration,
    pub force_refetch: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct CacheRunStats {
    pub hits: u64,
    pub misses: u64,
    pub stale: u64,
    pub writes: u64,
    pub bypassed_reads: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheStats {
    pub entry_count: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Cache {
    settings: CacheSettings,
    run_stats: Arc<Mutex<CacheRunStats>>,
}

#[derive(Debug, Clone)]
pub struct CacheLayer<T> {
    inner: T,
    cache: Cache,
}

#[derive(Debug, Clone)]
pub struct CacheKey {
    namespace: String,
    digest: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct DiskEntry {
    stored_at: DateTime<Utc>,
    value: serde_json::Value,
}

impl Cache {
    pub fn new(settings: CacheSettings) -> Self {
        Self {
            settings,
            run_stats: Arc::new(Mutex::new(CacheRunStats::default())),
        }
    }

    pub fn stats(&self) -> Result<CacheStats> {
        if !self.settings.disk_enabled {
            return Ok(CacheStats {
                entry_count: 0,
                size_bytes: 0,
            });
        }

        collect_dir_stats(&self.settings.dir)
    }

    pub fn run_stats(&self) -> CacheRunStats {
        *self.run_stats.lock().expect("cache stats mutex poisoned")
    }

    pub fn read<T>(&self, key: &CacheKey) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        if self.settings.force_refetch {
            self.bump_run_stats(|stats| stats.bypassed_reads += 1);
            trace!(
                cache_namespace = key.namespace.as_str(),
                cache_key = key.id(),
                cache_path = %key.disk_path(&self.settings.dir).display(),
                "cache read bypassed by force refetch"
            );
            return Ok(None);
        }

        let value = self.read_disk(key)?;
        Ok(value.map(serde_json::from_value).transpose()?)
    }

    pub fn write<T>(&self, key: &CacheKey, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value)?;
        self.write_disk_value(key, value)
    }

    fn read_disk(&self, key: &CacheKey) -> Result<Option<serde_json::Value>> {
        if !self.settings.disk_enabled {
            self.bump_run_stats(|stats| stats.misses += 1);
            trace!(
                cache_namespace = key.namespace.as_str(),
                cache_key = key.id(),
                "cache miss because disk cache is disabled"
            );
            return Ok(None);
        }

        let path = key.disk_path(&self.settings.dir);
        if !path.exists() {
            self.bump_run_stats(|stats| stats.misses += 1);
            trace!(
                cache_namespace = key.namespace.as_str(),
                cache_key = key.id(),
                cache_path = %path.display(),
                "cache miss"
            );
            return Ok(None);
        }

        let contents = fs::read_to_string(&path).map_err(|source| GhrgError::CacheIo {
            path: path.display().to_string(),
            source,
        })?;
        let entry: DiskEntry = serde_json::from_str(&contents)?;

        if is_stale_datetime(entry.stored_at, self.settings.ttl) {
            self.bump_run_stats(|stats| {
                stats.misses += 1;
                stats.stale += 1;
            });
            trace!(
                cache_namespace = key.namespace.as_str(),
                cache_key = key.id(),
                cache_path = %path.display(),
                cache_ttl_secs = self.settings.ttl.as_secs(),
                "cache entry stale"
            );
            fs::remove_file(&path).map_err(|source| GhrgError::CacheIo {
                path: path.display().to_string(),
                source,
            })?;
            return Ok(None);
        }

        self.bump_run_stats(|stats| stats.hits += 1);
        trace!(
            cache_namespace = key.namespace.as_str(),
            cache_key = key.id(),
            cache_path = %path.display(),
            "cache hit"
        );
        Ok(Some(entry.value))
    }

    fn write_disk_value(&self, key: &CacheKey, value: serde_json::Value) -> Result<()> {
        if !self.settings.disk_enabled {
            trace!(
                cache_namespace = key.namespace.as_str(),
                cache_key = key.id(),
                "cache write skipped because disk cache is disabled"
            );
            return Ok(());
        }

        let path = key.disk_path(&self.settings.dir);
        let Some(parent) = path.parent() else {
            return Ok(());
        };

        fs::create_dir_all(parent).map_err(|source| GhrgError::CacheIo {
            path: parent.display().to_string(),
            source,
        })?;

        let entry = DiskEntry {
            stored_at: Utc::now(),
            value,
        };
        let contents = serde_json::to_vec(&entry)?;
        fs::write(&path, contents).map_err(|source| GhrgError::CacheIo {
            path: path.display().to_string(),
            source,
        })?;

        self.bump_run_stats(|stats| stats.writes += 1);
        trace!(
            cache_namespace = key.namespace.as_str(),
            cache_key = key.id(),
            cache_path = %path.display(),
            "cache write"
        );
        Ok(())
    }

    fn bump_run_stats(&self, update: impl FnOnce(&mut CacheRunStats)) {
        let mut stats = self.run_stats.lock().expect("cache stats mutex poisoned");
        update(&mut stats);
    }

    pub fn log_summary(&self, operation: &str) {
        let stats = self.run_stats();
        debug!(
            cache_operation = operation,
            cache_dir = %self.settings.dir.display(),
            disk_enabled = self.settings.disk_enabled,
            ttl_secs = self.settings.ttl.as_secs(),
            hits = stats.hits,
            misses = stats.misses,
            stale = stats.stale,
            writes = stats.writes,
            bypassed_reads = stats.bypassed_reads,
            "cache summary"
        );
    }
}

impl CacheKey {
    pub fn new(namespace: impl Into<String>, identity: impl Serialize) -> Result<Self> {
        let namespace = namespace.into();
        let payload = serde_json::to_vec(&identity)?;
        let digest = hex::encode(Sha256::digest(payload));
        Ok(Self { namespace, digest })
    }

    pub fn id(&self) -> String {
        format!("{}:{}", self.namespace, self.digest)
    }

    fn disk_path(&self, root: &Path) -> PathBuf {
        root.join(&self.namespace)
            .join(format!("{}.json", self.digest))
    }
}

impl<T> CacheLayer<T> {
    pub fn new(inner: T, cache: Cache) -> Self {
        Self { inner, cache }
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    async fn cached_call<V, F>(
        &self,
        namespace: &str,
        identity: impl Serialize,
        fetch: F,
    ) -> Result<V>
    where
        V: Serialize + DeserializeOwned + Send,
        F: std::future::Future<Output = Result<V>> + Send,
    {
        let key = CacheKey::new(namespace, identity)?;
        if let Some(value) = self.cache.read(&key)? {
            return Ok(value);
        }

        let value = fetch.await?;
        self.cache.write(&key, &value)?;
        Ok(value)
    }
}

#[async_trait]
impl<T> RepoDataSource for CacheLayer<T>
where
    T: RepoDataSource + Clone + Send + Sync,
{
    async fn fetch_repo(&self, owner: &str, name: &str) -> Result<RepositoryBase> {
        self.cached_call(
            "github.repo",
            serde_json::json!({
                "owner": owner,
                "repo": name,
            }),
            self.inner.fetch_repo(owner, name),
        )
        .await
    }

    async fn list_repos(
        &self,
        scope: RepoScope<'_>,
        limit: Option<usize>,
    ) -> Result<Vec<RepositoryBase>> {
        self.cached_call(
            "github.repo-list",
            serde_json::json!({
                "scope": repo_scope_identity(scope),
                "limit": limit,
            }),
            self.inner.list_repos(scope, limit),
        )
        .await
    }

    async fn fetch_repo_properties(
        &self,
        owner: &str,
        repo: &str,
        names: &std::collections::BTreeSet<String>,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        self.cached_call(
            "github.context.properties",
            serde_json::json!({
                "owner": owner,
                "repo": repo,
                "names": names,
            }),
            self.inner.fetch_repo_properties(owner, repo, names),
        )
        .await
    }

    async fn fetch_repo_languages(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        self.cached_call(
            "github.context.languages",
            serde_json::json!({
                "owner": owner,
                "repo": repo,
            }),
            self.inner.fetch_repo_languages(owner, repo),
        )
        .await
    }

    async fn fetch_repo_branches(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoBranchesQuery,
    ) -> Result<Vec<RepoBranch>> {
        self.cached_call(
            "github.context.branches",
            serde_json::json!({
                "owner": owner,
                "repo": repo,
                "query": query,
            }),
            self.inner.fetch_repo_branches(owner, repo, query),
        )
        .await
    }

    async fn fetch_repo_commits(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoCommitsQuery,
    ) -> Result<Vec<RepoCommitEntry>> {
        self.cached_call(
            "github.context.commits",
            serde_json::json!({
                "owner": owner,
                "repo": repo,
                "query": query,
            }),
            self.inner.fetch_repo_commits(owner, repo, query),
        )
        .await
    }

    async fn fetch_repo_files(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoFilesQuery,
    ) -> Result<Vec<RepoFileEntry>> {
        self.cached_call(
            "github.context.files",
            serde_json::json!({
                "owner": owner,
                "repo": repo,
                "query": query,
            }),
            self.inner.fetch_repo_files(owner, repo, query),
        )
        .await
    }

    async fn fetch_repo_contributors(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoContributorsQuery,
    ) -> Result<Vec<RepoContributor>> {
        self.cached_call(
            "github.context.contributors",
            serde_json::json!({
                "owner": owner,
                "repo": repo,
                "query": query,
            }),
            self.inner.fetch_repo_contributors(owner, repo, query),
        )
        .await
    }

    async fn fetch_repo_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
        query: &RepoWorkflowRunsQuery,
    ) -> Result<Vec<RepoWorkflowRunEntry>> {
        self.cached_call(
            "github.context.workflow-runs",
            serde_json::json!({
                "owner": owner,
                "repo": repo,
                "query": query,
            }),
            self.inner.fetch_repo_workflow_runs(owner, repo, query),
        )
        .await
    }
}

fn is_stale_datetime(stored_at: DateTime<Utc>, ttl: Duration) -> bool {
    let Ok(ttl) = chrono::Duration::from_std(ttl) else {
        return false;
    };
    Utc::now().signed_duration_since(stored_at) > ttl
}

fn repo_scope_identity(scope: RepoScope<'_>) -> serde_json::Value {
    match scope {
        RepoScope::Repo(repo) => serde_json::json!({ "repo": repo }),
        RepoScope::Org(org) => serde_json::json!({ "org": org }),
        RepoScope::User(user) => serde_json::json!({ "user": user }),
        RepoScope::Owner(owner) => serde_json::json!({ "owner": owner }),
    }
}

fn collect_dir_stats(path: &Path) -> Result<CacheStats> {
    if !path.exists() {
        return Ok(CacheStats {
            entry_count: 0,
            size_bytes: 0,
        });
    }

    let metadata = fs::metadata(path).map_err(|source| GhrgError::CacheIo {
        path: path.display().to_string(),
        source,
    })?;
    if metadata.is_file() {
        return Ok(CacheStats {
            entry_count: 1,
            size_bytes: metadata.len(),
        });
    }

    let mut stats = CacheStats {
        entry_count: 0,
        size_bytes: 0,
    };

    for entry in fs::read_dir(path).map_err(|source| GhrgError::CacheIo {
        path: path.display().to_string(),
        source,
    })? {
        let entry = entry.map_err(|source| GhrgError::CacheIo {
            path: path.display().to_string(),
            source,
        })?;
        let child_path = entry.path();
        let child = collect_dir_stats(&child_path)?;
        stats.entry_count += child.entry_count;
        stats.size_bytes += child.size_bytes;
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::{Map, Value};
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reuses_disk_entries_within_ttl() {
        let dir = temp_dir();
        let cache = Cache::new(CacheSettings {
            dir: dir.clone(),
            disk_enabled: true,
            ttl: Duration::from_secs(60),
            force_refetch: false,
        });
        let key = CacheKey::new("github.repo", serde_json::json!({"repo": "acme/api"})).unwrap();

        cache
            .write(&key, &serde_json::json!({"name": "api"}))
            .unwrap();
        let value: serde_json::Value = cache.read(&key).unwrap().unwrap();

        assert_eq!(value["name"], "api");
        assert_eq!(cache.run_stats().hits, 1);
        let stats = cache.stats().unwrap();
        assert_eq!(stats.entry_count, 1);
        assert!(stats.size_bytes > 0);
    }

    #[test]
    fn skips_reads_when_force_refetch_is_enabled() {
        let dir = temp_dir();
        let key = CacheKey::new("github.repo", serde_json::json!({"repo": "acme/api"})).unwrap();
        let writer = Cache::new(CacheSettings {
            dir: dir.clone(),
            disk_enabled: true,
            ttl: Duration::from_secs(60),
            force_refetch: false,
        });
        writer
            .write(&key, &serde_json::json!({"name": "api"}))
            .unwrap();

        let reader = Cache::new(CacheSettings {
            dir,
            disk_enabled: true,
            ttl: Duration::from_secs(60),
            force_refetch: true,
        });
        let value: Option<serde_json::Value> = reader.read(&key).unwrap();

        assert!(value.is_none());
        assert_eq!(reader.run_stats().bypassed_reads, 1);
    }

    #[test]
    fn expires_stale_disk_entries() {
        let dir = temp_dir();
        let cache = Cache::new(CacheSettings {
            dir: dir.clone(),
            disk_enabled: true,
            ttl: Duration::from_secs(0),
            force_refetch: false,
        });
        let key = CacheKey::new("github.repo", serde_json::json!({"repo": "acme/api"})).unwrap();

        cache
            .write(&key, &serde_json::json!({"name": "api"}))
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let value: Option<serde_json::Value> = cache.read(&key).unwrap();

        assert!(value.is_none());
        assert_eq!(cache.run_stats().stale, 1);
        assert_eq!(cache.stats().unwrap().entry_count, 0);
    }

    #[tokio::test]
    async fn cache_layer_wraps_repo_data_source() {
        let source = FakeSource::default();
        let cache = Cache::new(CacheSettings {
            dir: temp_dir(),
            disk_enabled: true,
            ttl: Duration::from_secs(60),
            force_refetch: false,
        });
        let layer = CacheLayer::new(source.clone(), cache);

        let first = layer.fetch_repo("acme", "api").await.unwrap();
        let second = layer.fetch_repo("acme", "api").await.unwrap();

        assert_eq!(first.full_name, "acme/api");
        assert_eq!(second.full_name, "acme/api");
        assert_eq!(
            source.fetch_repo_calls.lock().unwrap().as_slice(),
            &["acme/api"]
        );
        assert_eq!(layer.cache().run_stats().hits, 1);
    }

    #[tokio::test]
    async fn cache_layer_caches_repo_lists() {
        let source = FakeSource::default();
        let cache = Cache::new(CacheSettings {
            dir: temp_dir(),
            disk_enabled: true,
            ttl: Duration::from_secs(60),
            force_refetch: false,
        });
        let layer = CacheLayer::new(source.clone(), cache);

        let first = layer
            .list_repos(RepoScope::Org("acme"), Some(25))
            .await
            .unwrap();
        let second = layer
            .list_repos(RepoScope::Org("acme"), Some(25))
            .await
            .unwrap();

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(
            source.list_repo_calls.lock().unwrap().as_slice(),
            &["org:acme:25"]
        );
        assert_eq!(layer.cache().run_stats().hits, 1);
    }

    fn temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ghrg-cache-test-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[derive(Default, Clone)]
    struct FakeSource {
        fetch_repo_calls: Arc<Mutex<Vec<String>>>,
        list_repo_calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl RepoDataSource for FakeSource {
        async fn fetch_repo(&self, owner: &str, name: &str) -> Result<RepositoryBase> {
            self.fetch_repo_calls
                .lock()
                .unwrap()
                .push(format!("{owner}/{name}"));
            Ok(RepositoryBase {
                name: name.to_string(),
                owner: owner.to_string(),
                full_name: format!("{owner}/{name}"),
                archived: false,
                fork: false,
                visibility: "public".to_string(),
                default_branch: "main".to_string(),
                topics: Vec::new(),
                github: Map::new(),
            })
        }

        async fn list_repos(
            &self,
            scope: RepoScope<'_>,
            limit: Option<usize>,
        ) -> Result<Vec<RepositoryBase>> {
            let scope_label = match scope {
                RepoScope::Repo(repo) => format!("repo:{repo}"),
                RepoScope::Org(org) => format!("org:{org}"),
                RepoScope::User(user) => format!("user:{user}"),
                RepoScope::Owner(owner) => format!("owner:{owner}"),
            };
            self.list_repo_calls
                .lock()
                .unwrap()
                .push(format!("{scope_label}:{}", limit.unwrap_or_default()));
            Ok(vec![RepositoryBase {
                name: "api".to_string(),
                owner: "acme".to_string(),
                full_name: "acme/api".to_string(),
                archived: false,
                fork: false,
                visibility: "public".to_string(),
                default_branch: "main".to_string(),
                topics: Vec::new(),
                github: Map::new(),
            }])
        }

        async fn fetch_repo_properties(
            &self,
            _owner: &str,
            _repo: &str,
            _names: &BTreeSet<String>,
        ) -> Result<Map<String, Value>> {
            Ok(Map::new())
        }

        async fn fetch_repo_languages(
            &self,
            _owner: &str,
            _repo: &str,
        ) -> Result<Map<String, Value>> {
            Ok(Map::new())
        }

        async fn fetch_repo_branches(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoBranchesQuery,
        ) -> Result<Vec<RepoBranch>> {
            Ok(Vec::new())
        }

        async fn fetch_repo_commits(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoCommitsQuery,
        ) -> Result<Vec<RepoCommitEntry>> {
            Ok(Vec::new())
        }

        async fn fetch_repo_files(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoFilesQuery,
        ) -> Result<Vec<RepoFileEntry>> {
            Ok(Vec::new())
        }

        async fn fetch_repo_contributors(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoContributorsQuery,
        ) -> Result<Vec<RepoContributor>> {
            Ok(Vec::new())
        }

        async fn fetch_repo_workflow_runs(
            &self,
            _owner: &str,
            _repo: &str,
            _query: &RepoWorkflowRunsQuery,
        ) -> Result<Vec<RepoWorkflowRunEntry>> {
            Ok(Vec::new())
        }
    }
}
