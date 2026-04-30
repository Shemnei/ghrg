use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use ghrg_core::error::{GhrgError, Result};

const APP_NAME: &str = "ghrg";

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub app_name: String,
    pub version: String,
    pub platform: String,
    pub disk_cache_enabled: bool,
    pub cache_dir: Option<PathBuf>,
    pub log_dir: PathBuf,
    pub log_file: PathBuf,
    pub execution_id: String,
    pub started_at: String,
}

impl RuntimeInfo {
    pub fn cache_dir_for_cache(&self) -> PathBuf {
        self.cache_dir
            .clone()
            .unwrap_or_else(disabled_cache_dir_placeholder)
    }

    pub fn cache_dir_display(&self) -> String {
        self.cache_dir
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "disabled".to_string())
    }
}

pub fn collect_runtime_info(
    version: impl Into<String>,
    cache_dir_override: Option<PathBuf>,
    log_dir_override: Option<PathBuf>,
    log_file_override: Option<PathBuf>,
    no_disk_cache: bool,
) -> Result<RuntimeInfo> {
    let execution_id = Uuid::new_v4().to_string();
    let started_at = Utc::now();

    let cache_dir = resolve_cache_dir(cache_dir_override, !no_disk_cache, default_cache_dir())?;

    let log_dir = match &log_file_override {
        Some(path) => path
            .parent()
            .map(PathBuf::from)
            .ok_or(GhrgError::MissingRuntimePath {
                kind: "log file parent directory",
            })?,
        None => log_dir_override
            .or_else(default_log_dir)
            .ok_or(GhrgError::MissingRuntimePath {
                kind: "log directory",
            })?,
    };

    let log_file = log_file_override
        .unwrap_or_else(|| default_log_file_path(&log_dir, started_at, &execution_id));

    Ok(RuntimeInfo {
        app_name: APP_NAME.to_string(),
        version: version.into(),
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        disk_cache_enabled: !no_disk_cache,
        cache_dir,
        log_dir,
        log_file,
        execution_id,
        started_at: started_at.to_rfc3339_opts(SecondsFormat::Secs, true),
    })
}

fn default_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join(APP_NAME))
}

fn disabled_cache_dir_placeholder() -> PathBuf {
    std::env::temp_dir()
        .join(APP_NAME)
        .join("disk-cache-disabled")
}

fn default_log_dir() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .map(|dir| dir.join(APP_NAME).join("logs"))
}

fn default_log_file_path(log_dir: &Path, started_at: DateTime<Utc>, execution_id: &str) -> PathBuf {
    let timestamp = started_at.format("%Y%m%dT%H%M%SZ");
    log_dir.join(format!("{}-{}.log", timestamp, execution_id))
}

fn resolve_cache_dir(
    cache_dir_override: Option<PathBuf>,
    disk_cache_required: bool,
    default_cache_dir: Option<PathBuf>,
) -> Result<Option<PathBuf>> {
    match cache_dir_override.or(default_cache_dir) {
        Some(path) => Ok(Some(path)),
        None if disk_cache_required => Err(GhrgError::MissingRuntimePath {
            kind: "cache directory",
        }),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_is_optional_when_disk_cache_is_disabled() {
        let cache_dir = resolve_cache_dir(None, false, None).unwrap();

        assert!(cache_dir.is_none());
    }

    #[test]
    fn cache_dir_is_required_when_disk_cache_is_enabled() {
        let error = resolve_cache_dir(None, true, None).unwrap_err();

        assert!(matches!(
            error,
            GhrgError::MissingRuntimePath {
                kind: "cache directory"
            }
        ));
    }
}
