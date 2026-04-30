use ghrg_core::contexts::repo::SampleRepoSeed;
use ghrg_core::github::RepositoryBase;

pub(crate) fn fallback_repo_base() -> RepositoryBase {
    RepositoryBase {
        name: "example-repo".to_string(),
        owner: "example-org".to_string(),
        full_name: "example-org/example-repo".to_string(),
        archived: false,
        fork: false,
        visibility: "public".to_string(),
        default_branch: "main".to_string(),
        topics: vec!["governance".to_string()],
        github: serde_json::Map::from_iter([
            ("id".to_string(), serde_json::json!(1)),
            (
                "url".to_string(),
                serde_json::json!("https://api.github.com/repos/example-org/example-repo"),
            ),
            (
                "html_url".to_string(),
                serde_json::json!("https://github.com/example-org/example-repo"),
            ),
            ("private".to_string(), serde_json::json!(false)),
            (
                "description".to_string(),
                serde_json::json!("Example repository sample"),
            ),
            ("disabled".to_string(), serde_json::json!(false)),
            ("has_issues".to_string(), serde_json::json!(true)),
            ("has_projects".to_string(), serde_json::json!(false)),
            ("has_wiki".to_string(), serde_json::json!(false)),
            ("has_pages".to_string(), serde_json::json!(false)),
            ("has_downloads".to_string(), serde_json::json!(false)),
            (
                "pushed_at".to_string(),
                serde_json::json!("2024-01-01T00:00:00Z"),
            ),
            (
                "created_at".to_string(),
                serde_json::json!("2023-01-01T00:00:00Z"),
            ),
            (
                "updated_at".to_string(),
                serde_json::json!("2024-01-15T00:00:00Z"),
            ),
            (
                "homepage".to_string(),
                serde_json::json!("https://example.com"),
            ),
            ("language".to_string(), serde_json::json!("Rust")),
            ("forks_count".to_string(), serde_json::json!(2)),
            ("stargazers_count".to_string(), serde_json::json!(5)),
            ("watchers_count".to_string(), serde_json::json!(5)),
            ("size".to_string(), serde_json::json!(128)),
            ("open_issues_count".to_string(), serde_json::json!(1)),
            ("is_template".to_string(), serde_json::json!(false)),
        ]),
    }
}

pub(crate) fn fallback_repo_seed() -> SampleRepoSeed {
    SampleRepoSeed::from_repo(&fallback_repo_base())
}
