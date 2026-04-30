use clap::{Args as ClapArgs, Subcommand};
use ghrg_core::contexts::repo::{SampleRepoSeed, repo_context_catalog, repo_context_catalog_entry};
use miette::{IntoDiagnostic, Result, miette};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::OutputFormatArg;
use crate::output::{CommandOutput, OutputFormat, OutputRecord};
use crate::ui::Ui;

#[derive(Debug, ClapArgs)]
#[command(
    about = "List and inspect repository context kinds",
    long_about = "Show the repository context kinds supported by `ghrg`, along with their configuration fields, sample shapes, and example policy usage.",
    after_help = "Examples:\n  ghrg contexts repos list\n  ghrg contexts repos show properties\n  ghrg contexts repos show files --format json"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "List supported repository context kinds")]
    List(ListArgs),
    #[command(about = "Show details for one repository context kind")]
    Show(ShowArgs),
}

#[derive(Debug, ClapArgs)]
pub struct ListArgs {
    #[arg(
        long,
        default_value = "pretty",
        help = "Render the context catalog as pretty output or JSON"
    )]
    pub format: OutputFormatArg,
}

#[derive(Debug, ClapArgs)]
pub struct ShowArgs {
    #[arg(value_parser = parse_repo_context_kind, help = "Repository context kind to inspect")]
    pub kind: String,

    #[arg(
        long,
        default_value = "pretty",
        help = "Render the context details as pretty output or JSON"
    )]
    pub format: OutputFormatArg,
}

fn parse_repo_context_kind(value: &str) -> std::result::Result<String, String> {
    repo_context_catalog_entry(value)
        .map(|doc| doc.kind.to_string())
        .ok_or_else(|| {
            format!(
                "invalid context kind `{value}`; supported kinds: {}",
                repo_context_catalog()
                    .iter()
                    .map(|doc| doc.kind)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn repo_context_doc(
    kind: &str,
) -> Result<&'static ghrg_core::contexts::repo::RepoContextCatalogEntry> {
    repo_context_catalog_entry(kind).ok_or_else(|| {
        miette!(
            "invalid context kind `{kind}`; supported kinds: {}",
            repo_context_catalog()
                .iter()
                .map(|doc| doc.kind)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

#[derive(Serialize)]
struct RepoContextListItem {
    kind: String,
    summary: String,
    default_input_key: String,
    supports_named_key: bool,
    config_fields: String,
}

#[derive(Serialize)]
struct RepoContextShowView {
    kind: String,
    summary: String,
    default_input_key: String,
    named_key_supported: bool,
    config_fields: Vec<String>,
    validation_rules: Vec<String>,
    example_metadata: Value,
    example_rego: String,
    sample_value: Value,
    sample_input_key: String,
    performance_note: String,
}

pub async fn run(args: &Args) -> Result<()> {
    match &args.command {
        Command::List(args) => run_list(args).await,
        Command::Show(args) => run_show(args).await,
    }
}

async fn run_list(args: &ListArgs) -> Result<()> {
    let ui = Ui::new();
    let spinner = ui.spinner("Listing repository context kinds");
    let records = repo_context_catalog()
        .iter()
        .map(|doc| {
            OutputRecord::from_serializable(&RepoContextListItem {
                kind: doc.kind.to_string(),
                summary: doc.summary.to_string(),
                default_input_key: doc.kind.to_string(),
                supports_named_key: true,
                config_fields: doc
                    .fields
                    .iter()
                    .map(|field| {
                        format!(
                            "{} ({})",
                            field.name,
                            if field.required {
                                "required"
                            } else {
                                "optional"
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            })
            .into_diagnostic()
        })
        .collect::<Result<Vec<_>>>()?;
    let rendered = CommandOutput::collection(
        "contexts repos list",
        Some("Repository Contexts".to_string()),
        records,
    )
    .format(to_output_format(args.format.clone()))?;
    ui.finish(spinner, "Listed repository context kinds");
    println!("{rendered}");
    Ok(())
}

async fn run_show(args: &ShowArgs) -> Result<()> {
    let ui = Ui::new();
    let spinner = ui.spinner(format!("Inspecting repository context {}", args.kind));
    let doc = repo_context_doc(&args.kind)?;
    let spec = (doc.example_spec)("main");
    let sample_input_key = spec.input_key().to_string();
    let sample_value = spec.sample_value(&SampleRepoSeed {
        name: "example-repo".to_string(),
        full_name: "example-org/example-repo".to_string(),
        default_branch: "main".to_string(),
    });
    let view = RepoContextShowView {
        kind: doc.kind.to_string(),
        summary: doc.summary.to_string(),
        default_input_key: doc.kind.to_string(),
        named_key_supported: true,
        config_fields: doc
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{}: {}{}",
                    field.name,
                    field.description,
                    if field.required { " (required)" } else { "" }
                )
            })
            .collect(),
        validation_rules: doc
            .validation_rules
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        example_metadata: example_metadata(doc.kind),
        example_rego: doc.example_rego.to_string(),
        sample_value,
        sample_input_key,
        performance_note: doc.performance_note.to_string(),
    };
    let rendered = CommandOutput::record(
        "contexts repos show",
        Some(format!("Repository Context: {}", doc.kind)),
        OutputRecord::from_serializable(&view)
            .into_diagnostic()?
            .with_meta(serde_json::to_value(&view).into_diagnostic()?),
    )
    .format(to_output_format(args.format.clone()))?;
    ui.finish(
        spinner,
        format!("Inspected repository context {}", doc.kind),
    );
    println!("{rendered}");
    Ok(())
}

fn to_output_format(format: OutputFormatArg) -> OutputFormat {
    match format {
        OutputFormatArg::Pretty => OutputFormat::Pretty,
        OutputFormatArg::Json => OutputFormat::Json,
    }
}

fn example_metadata(kind: &str) -> Value {
    let doc = repo_context_doc(kind).expect("repo context kind docs should exist");
    let spec = (doc.example_spec)("main");
    json!({ "contexts": [serde_json::to_value(spec).expect("context spec should serialize")] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghrg_core::contexts::repo::{
        branches, commits, contributors, files, languages, properties, workflow_runs,
    };

    #[test]
    fn list_includes_all_repo_context_kinds() {
        let kinds = repo_context_catalog()
            .iter()
            .map(|doc| doc.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                properties::KIND,
                languages::KIND,
                branches::KIND,
                commits::KIND,
                files::KIND,
                contributors::KIND,
                workflow_runs::KIND,
            ]
        );
    }

    #[test]
    fn show_examples_use_named_input_keys() {
        let properties = repo_context_catalog_entry(properties::KIND).unwrap();
        let files = repo_context_catalog_entry(files::KIND).unwrap();
        let workflow_runs = repo_context_catalog_entry(workflow_runs::KIND).unwrap();
        assert_eq!(
            (properties.example_spec)("main").input_key(),
            "repo_properties"
        );
        assert_eq!((files.example_spec)("main").input_key(), "workflow_files");
        assert_eq!(
            (workflow_runs.example_spec)("main").input_key(),
            "recent_workflow_runs"
        );
    }
}
