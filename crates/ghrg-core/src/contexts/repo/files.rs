use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{ContextBase, ContextProvider, ContextSpec};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, SampleRepoSeed};

pub const KIND: &str = "files";

pub const CATALOG_ENTRY: RepoContextCatalogEntry = RepoContextCatalogEntry {
    kind: KIND,
    summary: "Fetch repository file entries, optionally filtered by glob, ref, and content",
    fields: &[
        RepoContextFieldDoc {
            name: "name",
            description: "Optional custom key under input.contexts",
            required: false,
        },
        RepoContextFieldDoc {
            name: "glob",
            description: "Glob or path prefix used to narrow file lookups",
            required: false,
        },
        RepoContextFieldDoc {
            name: "limit",
            description: "Maximum number of file entries to fetch",
            required: false,
        },
        RepoContextFieldDoc {
            name: "ref",
            description: "Git ref or branch to inspect",
            required: false,
        },
        RepoContextFieldDoc {
            name: "include_content",
            description: "Include blob text content for matched files",
            required: false,
        },
    ],
    validation_rules: &[
        "`glob` and `ref` must be non-empty when present",
        "`limit` must be positive",
        "Live requests clamp `limit` to 500",
        "Omitted `ref` defaults to the repo default branch",
        "`include_content` fetches blob bodies only for matched file entries",
    ],
    example_rego: "count(input.contexts.workflow_files) > 0",
    performance_note: "Often one of the more expensive contexts; always narrow by `glob` and `limit`, especially when `include_content` is enabled.",
    example_spec,
    explicit_spec,
};

pub fn example_spec(default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some("workflow_files".to_string()),
        },
        provider: ContextProvider::Files(RepoFilesContext {
            glob: Some(".github/workflows/*.yml".to_string()),
            limit: Some(5),
            reference: Some(default_branch.to_string()),
            include_content: false,
        }),
    }
}

pub fn explicit_spec(default_branch: &str) -> ContextSpec {
    ContextSpec {
        base: ContextBase {
            name: Some(KIND.to_string()),
        },
        provider: ContextProvider::Files(RepoFilesContext {
            glob: Some("src/**".to_string()),
            limit: Some(5),
            reference: Some(default_branch.to_string()),
            include_content: false,
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoFilesQuery {
    pub limit: Option<u16>,
    pub glob: Option<String>,
    pub reference: Option<String>,
    pub include_content: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RepoFilesContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_content: bool,
}

impl RepoFilesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.glob.as_deref().is_some_and(str::is_empty) {
            return Err("`files.glob` must be a non-empty string".to_string());
        }
        if self.reference.as_deref().is_some_and(str::is_empty) {
            return Err("`files.ref` must be a non-empty string".to_string());
        }
        if self.limit.is_some_and(|value| value == 0) {
            return Err("`files.limit` must be a positive integer".to_string());
        }
        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.glob.as_ref().map(|value| format!("glob={value}")),
            self.limit.map(|value| format!("limit={value}")),
            self.reference.as_ref().map(|value| format!("ref={value}")),
            self.include_content
                .then(|| "include_content=true".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }

    pub fn sample_value(&self, seed: &SampleRepoSeed) -> Value {
        let count = self.limit.unwrap_or(3).clamp(1, 5) as usize;
        let paths = sample_paths(self.glob.as_deref(), count);

        Value::Array(
            paths
                .into_iter()
                .enumerate()
                .map(|(index, path)| {
                    let content = self.include_content.then(|| sample_file_content(&path));

                    serde_json::json!({
                        "name": path.rsplit('/').next().unwrap_or(seed.name.as_str()),
                        "path": path,
                        "type": "blob",
                        "mode": "100644",
                        "sha": format!("{:040x}", index + 101),
                        "size": 200 + (index as u64 * 17),
                        "reference": self.reference.clone().unwrap_or_else(|| seed.default_branch.clone()),
                        "glob": self.glob.clone().unwrap_or_else(|| "**".to_string()),
                        "content": content,
                    })
                })
                .collect(),
        )
    }

    pub async fn resolve<T>(&self, client: &T, repo: &RepositoryBase) -> Result<Value>
    where
        T: ResolveRepoFiles + Sync,
    {
        client.resolve_repo_files(repo, self).await
    }
}

#[async_trait]
pub trait ResolveRepoFiles {
    async fn resolve_repo_files(
        &self,
        repo: &RepositoryBase,
        context: &RepoFilesContext,
    ) -> Result<Value>;
}

#[async_trait]
impl<T> ResolveRepoFiles for T
where
    T: RepoDataSource + Sync,
{
    async fn resolve_repo_files(
        &self,
        repo: &RepositoryBase,
        context: &RepoFilesContext,
    ) -> Result<Value> {
        let mut query = RepoFilesQuery {
            limit: context.limit.map(|value| value.clamp(1, 500) as u16),
            glob: context.glob.clone(),
            reference: context.reference.clone(),
            include_content: context.include_content,
        };
        if query.reference.is_none() {
            query.reference = Some(repo.default_branch.clone());
        }
        let rows = self
            .fetch_repo_files(&repo.owner, &repo.name, &query)
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn sample_paths(glob: Option<&str>, count: usize) -> Vec<String> {
    let pattern = glob.map(str::trim).filter(|value| !value.is_empty());

    let candidate_paths = sample_candidate_paths();

    if let Some(pattern) = pattern
        && let Ok(glob) = globset::Glob::new(pattern)
    {
        let matcher = glob.compile_matcher();
        let matches = candidate_paths
            .iter()
            .filter(|path| matcher.is_match(path))
            .cloned()
            .take(count)
            .collect::<Vec<_>>();

        if !matches.is_empty() {
            return matches;
        }
    }

    let paths = match pattern {
        Some("*") => vec![
            "renovate.json5".to_string(),
            ".renovaterc.json".to_string(),
            "README.md".to_string(),
        ],
        Some(".github/*") => vec![
            ".github/renovate.json5".to_string(),
            ".github/workflows/check.yml".to_string(),
        ],
        Some(".gitlab/*") => vec![".gitlab/renovate.json5".to_string()],
        Some(pattern)
            if !pattern.contains('*')
                && !pattern.contains('?')
                && !pattern.contains('[')
                && !pattern.contains('{') =>
        {
            vec![pattern.to_string()]
        }
        Some(pattern) if pattern.starts_with("docs/") => (0..count)
            .map(|index| format!("docs/page-{}.md", index + 1))
            .collect(),
        Some(pattern) if pattern.starts_with(".github/") => (0..count)
            .map(|index| format!(".github/workflows/check-{}.yml", index + 1))
            .collect(),
        Some(pattern) if pattern.starts_with(".gitlab/") => (0..count)
            .map(|index| format!(".gitlab/ci-{}.yml", index + 1))
            .collect(),
        _ => (0..count)
            .map(|index| format!("src/module_{}/lib.rs", index + 1))
            .collect(),
    };

    paths.into_iter().take(count).collect()
}

fn sample_candidate_paths() -> Vec<String> {
    vec![
        "renovate.json".to_string(),
        "renovate.json5".to_string(),
        ".github/renovate.json".to_string(),
        ".github/renovate.json5".to_string(),
        ".gitlab/renovate.json".to_string(),
        ".gitlab/renovate.json5".to_string(),
        ".renovaterc".to_string(),
        ".renovaterc.json".to_string(),
        ".renovaterc.json5".to_string(),
        ".github/workflows/check.yml".to_string(),
        ".gitlab/ci.yml".to_string(),
        "docs/page-1.md".to_string(),
        "docs/page-2.md".to_string(),
        "src/module_1/lib.rs".to_string(),
        "src/module_2/lib.rs".to_string(),
        "README.md".to_string(),
    ]
}

fn sample_file_content(path: &str) -> String {
    match path {
        "package.json" => r#"{
  "name": "sample-repo",
  "version": "1.0.0",
  "renovate": {
    "extends": ["config:recommended"],
    "minimumReleaseAge": "14 days"
  }
}"#
        .to_string(),
        "renovate.json"
        | "renovate.json5"
        | ".github/renovate.json"
        | ".github/renovate.json5"
        | ".gitlab/renovate.json"
        | ".gitlab/renovate.json5"
        | ".renovaterc"
        | ".renovaterc.json"
        | ".renovaterc.json5" => r#"{
  extends: ["config:recommended"],
  minimumReleaseAge: "14 days"
}"#
        .to_string(),
        _ => format!("sample content for {path}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_params_includes_content_flag_when_enabled() {
        let context = RepoFilesContext {
            glob: Some("*".to_string()),
            limit: Some(5),
            reference: Some("main".to_string()),
            include_content: true,
        };

        assert_eq!(
            context.render_params(),
            "glob=*, limit=5, ref=main, include_content=true"
        );
    }

    #[test]
    fn sample_value_includes_file_content_when_requested() {
        let context = RepoFilesContext {
            glob: Some("*".to_string()),
            limit: Some(1),
            reference: Some("main".to_string()),
            include_content: true,
        };
        let seed = SampleRepoSeed {
            name: "api".to_string(),
            full_name: "acme/api".to_string(),
            default_branch: "main".to_string(),
        };

        let value = context.sample_value(&seed);
        let first = value.as_array().and_then(|rows| rows.first()).unwrap();

        assert_eq!(
            first.get("path"),
            Some(&Value::String("renovate.json".to_string()))
        );
        assert!(
            first
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.contains("minimumReleaseAge"))
        );
    }
}
