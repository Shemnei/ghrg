use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contexts::{
    ContextSpec, ContextValue, DynamicContextData, resolve_optional_context_value,
};
use crate::error::Result;
use crate::github::{RepoDataSource, RepositoryBase};

use super::{RepoContextCatalogEntry, RepoContextFieldDoc, RepoContextResolver, SampleRepoSeed};

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
    spec(
        Some("workflow_files"),
        &RepoFilesContext {
            glob: Some(".github/workflows/*.yml".to_string().into()),
            limit: Some(5.into()),
            reference: Some(default_branch.to_string().into()),
            include_content: false.into(),
        },
    )
}

pub fn explicit_spec(default_branch: &str) -> ContextSpec {
    spec(
        Some(KIND),
        &RepoFilesContext {
            glob: Some("src/**".to_string().into()),
            limit: Some(5.into()),
            reference: Some(default_branch.to_string().into()),
            include_content: false.into(),
        },
    )
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
    pub glob: Option<ContextValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<ContextValue<u64>>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<ContextValue<String>>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_content: ContextValue<bool>,
}

impl RepoFilesContext {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self
            .glob
            .as_ref()
            .and_then(ContextValue::literal)
            .is_some_and(String::is_empty)
        {
            return Err("`files.glob` must be a non-empty string".to_string());
        }
        if self
            .glob
            .as_ref()
            .and_then(ContextValue::default_value)
            .is_some_and(String::is_empty)
        {
            return Err("`files.glob.default` must be a non-empty string".to_string());
        }
        if self
            .reference
            .as_ref()
            .and_then(ContextValue::literal)
            .is_some_and(String::is_empty)
        {
            return Err("`files.ref` must be a non-empty string".to_string());
        }
        if self
            .reference
            .as_ref()
            .and_then(ContextValue::default_value)
            .is_some_and(String::is_empty)
        {
            return Err("`files.ref.default` must be a non-empty string".to_string());
        }
        if self.limit.as_ref().and_then(ContextValue::literal) == Some(&0) {
            return Err("`files.limit` must be a positive integer".to_string());
        }
        if let Some(limit) = self.limit.as_ref().and_then(ContextValue::default_value)
            && *limit == 0
        {
            return Err("`files.limit.default` must be a positive integer".to_string());
        }

        if let Some(glob) = &self.glob {
            glob.validate_source("files.glob")?;
        }
        if let Some(limit) = &self.limit {
            limit.validate_source("files.limit")?;
        }
        if let Some(reference) = &self.reference {
            reference.validate_source("files.ref")?;
        }
        self.include_content
            .validate_source("files.include_content")?;

        Ok(())
    }

    pub fn render_params(&self) -> String {
        [
            self.glob.as_ref().map(render_param("glob")),
            self.limit.as_ref().map(render_param("limit")),
            self.reference.as_ref().map(render_param("ref")),
            Some(render_param("include_content")(&self.include_content))
                .filter(|value| value != "include_content=false"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }

    pub fn sample_value(&self, seed: &SampleRepoSeed) -> Value {
        let count = self
            .limit
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or(3)
            .clamp(1, 5) as usize;
        let glob = self.glob.as_ref().and_then(ContextValue::sample_or_default);
        let include_content = self.include_content.sample_or_default().unwrap_or(false);
        let reference = self
            .reference
            .as_ref()
            .and_then(ContextValue::sample_or_default)
            .unwrap_or_else(|| seed.default_branch.clone());

        let paths = sample_paths(glob.as_deref(), count);

        Value::Array(
            paths
                .into_iter()
                .enumerate()
                .map(|(index, path)| {
                    let content = include_content.then(|| sample_file_content(&path));

                    serde_json::json!({
                        "name": path.rsplit('/').next().unwrap_or(seed.name.as_str()),
                        "path": path,
                        "type": "blob",
                        "mode": "100644",
                        "sha": format!("{:040x}", index + 101),
                        "size": 200 + (index as u64 * 17),
                        "reference": reference.clone(),
                        "glob": glob.clone().unwrap_or_else(|| "**".to_string()),
                        "content": content,
                    })
                })
                .collect(),
        )
    }

    pub fn resolve_dynamic(&self, runtime: &DynamicContextData<'_>) -> Result<Self> {
        Ok(Self {
            glob: resolve_optional_context_value(&self.glob, runtime, "files.glob")?
                .map(ContextValue::from),
            limit: resolve_optional_context_value(&self.limit, runtime, "files.limit")?
                .map(ContextValue::from),
            reference: resolve_optional_context_value(&self.reference, runtime, "files.ref")?
                .map(ContextValue::from),
            include_content: self
                .include_content
                .resolve(runtime, "files.include_content")?
                .into(),
        })
    }

    pub async fn resolve(
        &self,
        client: &dyn RepoDataSource,
        repo: &RepositoryBase,
    ) -> Result<Value> {
        let mut query = RepoFilesQuery {
            limit: self
                .limit
                .as_ref()
                .and_then(ContextValue::literal)
                .copied()
                .map(|value| value.clamp(1, 500) as u16),
            glob: self.glob.as_ref().and_then(ContextValue::literal).cloned(),
            reference: self
                .reference
                .as_ref()
                .and_then(ContextValue::literal)
                .cloned(),
            include_content: self.include_content.literal().copied().unwrap_or(false),
        };
        if query.reference.is_none() {
            query.reference = Some(repo.default_branch.clone());
        }
        let rows = client
            .fetch_repo_files(&repo.owner, &repo.name, &query)
            .await?;
        serde_json::to_value(rows).map_err(Into::into)
    }
}

pub struct FilesResolver;
pub static RESOLVER: FilesResolver = FilesResolver;

#[async_trait]
impl RepoContextResolver for FilesResolver {
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

fn is_false(value: &ContextValue<bool>) -> bool {
    matches!(value, ContextValue::Literal(false))
}

fn render_param<T: std::fmt::Display>(name: &str) -> impl FnOnce(&ContextValue<T>) -> String + '_ {
    move |value| match value {
        ContextValue::Literal(value) => format!("{name}={value}"),
        ContextValue::Ref(reference) => format!("{name}<-{}", reference.from),
    }
}

fn parse_params(
    params: &serde_json::Map<String, serde_json::Value>,
) -> std::result::Result<RepoFilesContext, String> {
    serde_json::from_value(Value::Object(params.clone())).map_err(|error| error.to_string())
}

fn to_params(context: &RepoFilesContext) -> serde_json::Map<String, serde_json::Value> {
    serde_json::to_value(context)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn spec(name: Option<&str>, context: &RepoFilesContext) -> ContextSpec {
    ContextSpec {
        name: name.map(ToString::to_string),
        kind: KIND.to_string(),
        params: to_params(context),
    }
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
            .take(count)
            .cloned()
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
            glob: Some("*".to_string().into()),
            limit: Some(5.into()),
            reference: Some("main".to_string().into()),
            include_content: true.into(),
        };

        assert_eq!(
            context.render_params(),
            "glob=*, limit=5, ref=main, include_content=true"
        );
    }

    #[test]
    fn sample_value_includes_file_content_when_requested() {
        let context = RepoFilesContext {
            glob: Some("*".to_string().into()),
            limit: Some(1.into()),
            reference: Some("main".to_string().into()),
            include_content: true.into(),
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

    #[test]
    fn render_params_shows_dynamic_sources() {
        let context = RepoFilesContext {
            glob: Some(ContextValue::Ref(crate::contexts::ContextValueRef {
                from: "input.paths.workflow_glob".to_string(),
                default: Some(".github/workflows/*.yml".to_string()),
            })),
            limit: Some(ContextValue::Ref(crate::contexts::ContextValueRef {
                from: "meta.last.limit".to_string(),
                default: Some(10),
            })),
            reference: Some(ContextValue::Ref(crate::contexts::ContextValueRef {
                from: "env.GHRG_REF".to_string(),
                default: Some("main".to_string()),
            })),
            include_content: false.into(),
        };

        assert_eq!(
            context.render_params(),
            "glob<-input.paths.workflow_glob, limit<-meta.last.limit, ref<-env.GHRG_REF"
        );
    }
}
