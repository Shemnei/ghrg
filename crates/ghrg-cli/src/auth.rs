use ghrg_core::github::{GitHubAppCredentials, GitHubCredentials};
use miette::{IntoDiagnostic, Result, miette};
use serde::Serialize;
use std::fs;
use std::process::Command;

use crate::cli::{AuthMethod, AuthSource, Cli};

const GHRG_SECRET_SERVICE: &str = "ghrg";
const SECRET_GITHUB_TOKEN: &str = "github-token";
const SECRET_GITHUB_APP_ID: &str = "github-app-id";
const SECRET_GITHUB_INSTALLATION_ID: &str = "github-installation-id";
const SECRET_GITHUB_PRIVATE_KEY: &str = "github-private-key";

const ENV_GITHUB_TOKEN: &str = "GITHUB_TOKEN";
const ENV_GH_TOKEN: &str = "GH_TOKEN";
const ENV_GITHUB_APP_ID: &str = "GHRG_GITHUB_APP_ID";
const ENV_GITHUB_INSTALLATION_ID: &str = "GHRG_GITHUB_INSTALLATION_ID";
const ENV_GITHUB_PRIVATE_KEY: &str = "GHRG_GITHUB_PRIVATE_KEY";
const ENV_GITHUB_PRIVATE_KEY_FILE: &str = "GHRG_GITHUB_PRIVATE_KEY_FILE";

#[derive(Debug, Clone, Serialize)]
pub struct AuthLookupInfo {
    pub auth_method: String,
    pub auth_source: String,
    pub personal_token_lookup: Vec<String>,
    pub github_app_lookup: Vec<String>,
    pub gh_cli_lookup: String,
}

pub fn resolve_credentials(cli: &Cli) -> Result<GitHubCredentials> {
    match cli.auth {
        Some(AuthMethod::GhCli) => gh_cli_token().map(GitHubCredentials::PersonalToken),
        Some(AuthMethod::GhApp) => {
            resolve_github_app_credentials(cli.auth_source).map(GitHubCredentials::AppInstallation)
        }
        None => resolve_default_credentials(cli.auth_source),
    }
}

pub fn auth_lookup_info(cli: &Cli) -> AuthLookupInfo {
    AuthLookupInfo {
        auth_method: cli
            .auth
            .as_ref()
            .map(AuthMethod::label)
            .unwrap_or("auto")
            .to_string(),
        auth_source: cli.auth_source.label().to_string(),
        personal_token_lookup: personal_token_lookup(cli.auth_source)
            .into_iter()
            .map(str::to_string)
            .collect(),
        github_app_lookup: github_app_lookup(cli.auth_source)
            .into_iter()
            .map(str::to_string)
            .collect(),
        gh_cli_lookup: "`gh auth token`".to_string(),
    }
}

fn resolve_default_credentials(source: AuthSource) -> Result<GitHubCredentials> {
    if let Some(token) = resolve_personal_token(source)? {
        return Ok(GitHubCredentials::PersonalToken(token));
    }

    gh_cli_token()
        .map(GitHubCredentials::PersonalToken)
        .or(Ok(GitHubCredentials::None))
}

fn resolve_personal_token(source: AuthSource) -> Result<Option<String>> {
    match source {
        AuthSource::Env => Ok([ENV_GITHUB_TOKEN, ENV_GH_TOKEN]
            .into_iter()
            .find_map(non_empty_env)),
        AuthSource::SecretService => secret_service_entry(SECRET_GITHUB_TOKEN),
    }
}

fn resolve_github_app_credentials(source: AuthSource) -> Result<GitHubAppCredentials> {
    match source {
        AuthSource::Env => resolve_github_app_credentials_from_env(),
        AuthSource::SecretService => resolve_github_app_credentials_from_secret_service(),
    }
}

fn resolve_github_app_credentials_from_env() -> Result<GitHubAppCredentials> {
    let app_id = required_env_u64(ENV_GITHUB_APP_ID)?;
    let installation_id = required_env_u64(ENV_GITHUB_INSTALLATION_ID)?;
    let private_key = resolve_private_key_from_env()?;

    Ok(GitHubAppCredentials {
        app_id,
        installation_id,
        private_key,
    })
}

fn resolve_github_app_credentials_from_secret_service() -> Result<GitHubAppCredentials> {
    let app_id = required_secret_service_u64(SECRET_GITHUB_APP_ID)?;
    let installation_id = required_secret_service_u64(SECRET_GITHUB_INSTALLATION_ID)?;
    let private_key = required_secret_service_value(SECRET_GITHUB_PRIVATE_KEY)?.into_bytes();

    Ok(GitHubAppCredentials {
        app_id,
        installation_id,
        private_key,
    })
}

fn resolve_private_key_from_env() -> Result<Vec<u8>> {
    let private_key = non_empty_env(ENV_GITHUB_PRIVATE_KEY);
    let private_key_file = non_empty_env(ENV_GITHUB_PRIVATE_KEY_FILE);

    match (private_key, private_key_file) {
        (Some(_), Some(_)) => Err(miette!(
            "GitHub App private key must be provided by exactly one source: `{ENV_GITHUB_PRIVATE_KEY}` or `{ENV_GITHUB_PRIVATE_KEY_FILE}`"
        )),
        (Some(value), None) => Ok(value.into_bytes()),
        (None, Some(path)) => fs::read(&path)
            .into_diagnostic()
            .map_err(|_| miette!("failed to read GitHub App private key from `{path}`")),
        (None, None) => Err(miette!(
            "missing required GitHub App value: set `{ENV_GITHUB_PRIVATE_KEY}` or `{ENV_GITHUB_PRIVATE_KEY_FILE}`"
        )),
    }
}

fn required_env_value(var: &'static str) -> Result<String> {
    non_empty_env(var).ok_or_else(|| miette!("missing required environment variable `{var}`"))
}

fn required_env_u64(var: &'static str) -> Result<u64> {
    let value = required_env_value(var)?;
    value
        .parse()
        .map_err(|error| miette!("invalid value for `{var}`: {error}"))
}

fn non_empty_env(var: &'static str) -> Option<String> {
    std::env::var(var).ok().filter(|value| !value.is_empty())
}

fn required_secret_service_value(entry: &'static str) -> Result<String> {
    secret_service_entry(entry)?.ok_or_else(|| {
        miette!(
            "missing required Secret Service entry `{entry}` in service `{GHRG_SECRET_SERVICE}`"
        )
    })
}

fn required_secret_service_u64(entry: &'static str) -> Result<u64> {
    let value = required_secret_service_value(entry)?;
    value
        .parse()
        .map_err(|error| miette!("invalid value in Secret Service entry `{entry}`: {error}"))
}

fn secret_service_entry(entry: &'static str) -> Result<Option<String>> {
    secret_service::entry(entry)
}

fn gh_cli_token() -> Result<String> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .into_diagnostic()
        .map_err(|error| miette!("failed to run `gh auth token`: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            "GitHub CLI did not return a token".to_string()
        } else {
            stderr
        };
        return Err(miette!("failed to acquire GitHub auth token: {message}"));
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err(miette!(
            "failed to acquire GitHub auth token: `gh auth token` returned an empty token"
        ));
    }

    Ok(token)
}

fn personal_token_lookup(source: AuthSource) -> Vec<&'static str> {
    match source {
        AuthSource::Env => vec![ENV_GITHUB_TOKEN, ENV_GH_TOKEN],
        AuthSource::SecretService => vec!["service=ghrg entry=github-token"],
    }
}

fn github_app_lookup(source: AuthSource) -> Vec<&'static str> {
    match source {
        AuthSource::Env => vec![
            ENV_GITHUB_APP_ID,
            ENV_GITHUB_INSTALLATION_ID,
            ENV_GITHUB_PRIVATE_KEY,
            ENV_GITHUB_PRIVATE_KEY_FILE,
        ],
        AuthSource::SecretService => vec![
            "service=ghrg entry=github-app-id",
            "service=ghrg entry=github-installation-id",
            "service=ghrg entry=github-private-key",
        ],
    }
}

#[cfg(all(feature = "secret-service", target_os = "linux", target_env = "gnu"))]
mod secret_service {
    use super::*;
    use dbus_secret_service_keyring_store::Store;
    use keyring_core::{Entry, Error as KeyringError, set_default_store};

    fn set_secret_service_store() -> Result<()> {
        let store = Store::new()
            .into_diagnostic()
            .map_err(|_| miette!("failed to initialize Secret Service store"))?;
        set_default_store(store);
        Ok(())
    }

    pub(super) fn entry(entry: &'static str) -> Result<Option<String>> {
        set_secret_service_store()?;
        let secret_entry = Entry::new(GHRG_SECRET_SERVICE, entry)
            .into_diagnostic()
            .map_err(|_| miette!("failed to open Secret Service entry `{entry}`"))?;
        match secret_entry.get_password() {
            Ok(value) if value.is_empty() => Ok(None),
            Ok(value) => Ok(Some(value)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(miette!(
                "failed to read Secret Service entry `{entry}` from service `{GHRG_SECRET_SERVICE}`: {error}"
            )),
        }
    }
}

#[cfg(not(all(feature = "secret-service", target_os = "linux", target_env = "gnu")))]
mod secret_service {
    use super::*;

    pub(super) fn entry(_entry: &'static str) -> Result<Option<String>> {
        Err(miette!(
            "`--auth-source secret-service` is not supported by this build; use environment variables or a Linux GNU build with the `secret-service` feature enabled"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_lookup_info_uses_selected_source() {
        let cli = Cli {
            auth: Some(AuthMethod::GhApp),
            auth_source: AuthSource::SecretService,
            config: None,
            cache_dir: None,
            no_disk_cache: false,
            cache_ttl: std::time::Duration::from_secs(60),
            force_refetch: false,
            log_dir: None,
            log_file: None,
            log_level: "info".to_string(),
            trace: false,
            command: crate::cli::Command::Info(crate::commands::info::Args {
                format: crate::cli::OutputFormatArg::Pretty,
            }),
        };

        let info = auth_lookup_info(&cli);

        assert_eq!(info.auth_method, "gh-app");
        assert_eq!(info.auth_source, "secret-service");
        assert!(
            info.github_app_lookup
                .contains(&"service=ghrg entry=github-private-key".to_string())
        );
    }

    #[test]
    fn env_lookup_lists_expected_keys() {
        assert_eq!(
            github_app_lookup(AuthSource::Env),
            vec![
                ENV_GITHUB_APP_ID,
                ENV_GITHUB_INSTALLATION_ID,
                ENV_GITHUB_PRIVATE_KEY,
                ENV_GITHUB_PRIVATE_KEY_FILE,
            ]
        );
    }
}
