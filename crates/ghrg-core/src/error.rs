use miette::{Diagnostic, NamedSource, SourceSpan};
use serde_json::Value;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, GhrgError>;

#[derive(Debug, Error, Diagnostic)]
pub enum GhrgError {
    #[error("failed to resolve runtime path for {kind}")]
    #[diagnostic(help("set an explicit path with `--cache-dir`, `--log-dir`, or `--log-file`"))]
    MissingRuntimePath { kind: &'static str },

    #[error("csv output requires scalar fields in the final policy-shaped output")]
    #[diagnostic(help("shape the final object in policy output so CSV only sees scalar fields"))]
    CsvRequiresScalarFields,

    #[error("failed to render csv output: {0}")]
    Csv(#[from] csv::Error),

    #[error("failed to render json output: {0}")]
    Json(#[from] serde_json::Error),

    #[error("output object helpers require a serialized JSON object")]
    #[diagnostic(help("pass a struct or map-like value instead of a scalar or array"))]
    OutputObjectRequiresJsonObject,

    #[error("failed to read policy file `{path}`: {source}")]
    #[diagnostic(help("check that the file exists and is readable"))]
    PolicyRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("cache I/O failed for `{path}`: {source}")]
    #[diagnostic(help(
        "check cache directory permissions or disable persistent cache with `--no-disk-cache`"
    ))]
    CacheIo {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "policy metadata conflict: both embedded metadata and sidecar metadata were found for {path}"
    )]
    #[diagnostic(help("keep either embedded metadata or the sidecar file, but not both"))]
    PolicyMetadataConflict { path: String },

    #[error("embedded metadata must appear before the package declaration in `{path}`")]
    #[diagnostic(help("move the `# ```ghrg` block to the top of the file before `package`"))]
    EmbeddedMetadataAfterPackage {
        path: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("embedded metadata starts here")]
        span: SourceSpan,
    },

    #[error("malformed embedded metadata fence in `{path}`")]
    #[diagnostic(help(
        "embedded metadata must start with `# ```ghrg` and end with `# ````, with only comment lines inside"
    ))]
    MalformedEmbeddedMetadataFence {
        path: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("malformed embedded metadata block")]
        span: SourceSpan,
    },

    #[error("failed to parse embedded metadata in `{path}`: {source}")]
    #[diagnostic(help("fix the YAML inside the `# ```ghrg` block"))]
    EmbeddedMetadataYaml {
        path: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("invalid YAML in embedded metadata")]
        span: SourceSpan,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("failed to parse sidecar metadata in `{path}`: {source}")]
    #[diagnostic(help("fix the YAML in the sidecar metadata file"))]
    SidecarMetadataYaml {
        path: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("invalid YAML in sidecar metadata")]
        span: SourceSpan,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("policy evaluation failed for `{path}` while evaluating `{rule}`: {message}")]
    #[diagnostic(help("check the Rego package and rule names, and validate the input shape"))]
    PolicyEvaluation {
        path: String,
        rule: String,
        message: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("policy involved in this failure")]
        span: SourceSpan,
    },

    #[error("policy package was not found in `{path}`")]
    #[diagnostic(help("add a `package ...` declaration to the policy file"))]
    MissingPolicyPackage {
        path: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("expected a package declaration near the top of the file")]
        span: SourceSpan,
    },

    #[error("policy metadata validation failed for `{path}`: {message}")]
    #[diagnostic(help("fix the metadata shape or context parameters before loading the policy"))]
    PolicyMetadataValidation {
        path: String,
        message: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("invalid policy metadata")]
        span: SourceSpan,
    },

    #[error("invalid repository selector `{value}`")]
    #[diagnostic(help("use the form `owner/name`"))]
    InvalidRepositorySelector { value: String },

    #[error("invalid context kind `{kind}`")]
    #[diagnostic(help("run `ghrg contexts repos list` to see supported kinds"))]
    InvalidContextKind { kind: String },

    #[error(
        "failed to resolve dynamic context field `{field}` from `{source_path}`: source value is missing"
    )]
    #[diagnostic(help(
        "set a `default` value in the context field or ensure the input/env/meta source exists"
    ))]
    ContextDynamicSourceMissing { field: String, source_path: String },

    #[error("failed to resolve dynamic context field `{field}` from `{source_path}`: {details}")]
    #[diagnostic(help(
        "ensure the source value type matches the context field type, or use a compatible `default`"
    ))]
    ContextDynamicTypeMismatch {
        field: String,
        source_path: String,
        value: Value,
        details: String,
    },

    #[error("conflicting context specifications for `{key}`")]
    #[diagnostic(help(
        "use a single context definition for that input key, or make the explicit and policy specs match"
    ))]
    ConflictingContextSpec { key: String },

    #[error("invalid `{kind}` context parameters: {details}")]
    #[diagnostic(help(
        "move context fields under `params` and ensure their shape matches the context kind schema"
    ))]
    InvalidContextParams { kind: String, details: String },

    #[error("failed to acquire GitHub auth token: {message}")]
    #[diagnostic(help("check `gh auth status`, `GH_TOKEN`, or `GITHUB_TOKEN`"))]
    GitHubAuthCommand {
        command: String,
        status: Option<i32>,
        stdout: String,
        stderr: String,
        message: String,
    },

    #[error("missing required GitHub App environment variable `{var}`")]
    #[diagnostic(help(
        "set `GHRG_GITHUB_APP_ID`, `GHRG_GITHUB_INSTALLATION_ID`, and one of `GHRG_GITHUB_PRIVATE_KEY` or `GHRG_GITHUB_PRIVATE_KEY_FILE`"
    ))]
    MissingGitHubAppEnv { var: &'static str },

    #[error("invalid GitHub App environment variable `{var}`: {message}")]
    #[diagnostic(help("check the variable value and try again"))]
    InvalidGitHubAppEnv {
        var: &'static str,
        value: String,
        message: String,
    },

    #[error("GitHub App private key must be provided by exactly one source")]
    #[diagnostic(help(
        "set either `GHRG_GITHUB_PRIVATE_KEY` or `GHRG_GITHUB_PRIVATE_KEY_FILE`, but not both"
    ))]
    GitHubAppKeyConflict,

    #[error("failed to read GitHub App private key from `{path}`: {source}")]
    #[diagnostic(help("check that the file exists and is readable"))]
    GitHubAppKeyRead {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse GitHub App private key: {message}")]
    #[diagnostic(help("provide a valid RSA private key in PEM or DER format"))]
    GitHubAppKeyInvalid { message: String },

    #[error("unsupported auth method `{method}`")]
    #[diagnostic(help("use `gh-cli` for now"))]
    UnsupportedAuthMethod { method: String },

    #[error("failed to build GitHub client: {message}")]
    GitHubClientBuild { message: String, details: String },

    #[error("GitHub request failed while trying to {operation}: {message}")]
    #[diagnostic(help("inspect the attached details for GitHub status and API context"))]
    GitHubRequest {
        operation: String,
        message: String,
        status: Option<u16>,
        body: Option<String>,
        details: String,
    },
}
