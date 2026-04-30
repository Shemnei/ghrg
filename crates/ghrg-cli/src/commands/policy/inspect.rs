use clap::Args as ClapArgs;
use miette::{IntoDiagnostic, Result};
use serde::Serialize;
use std::path::PathBuf;

use crate::output::{CommandOutput, OutputFormat, OutputRecord};
use crate::ui::Ui;

#[derive(Debug, ClapArgs)]
#[command(
    about = "Show a policy's package, metadata, and requested contexts",
    long_about = "Compile a single policy file and print the package name, metadata source, metadata fields, and any declared contexts. Use this to confirm that sidecar or embedded metadata is being picked up correctly.",
    after_help = "Example:\n  ghrg policy inspect --policy examples/policies/project-summary.rego --format json"
)]
pub struct Args {
    #[arg(long, help = "Path to the Rego policy file to inspect")]
    pub policy: PathBuf,

    #[arg(
        long,
        default_value = "pretty",
        help = "Render the inspection result as a table or JSON"
    )]
    pub format: Format,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Format {
    Pretty,
    Json,
}

pub async fn run(args: &Args) -> Result<()> {
    let ui = Ui::new();
    let spinner = ui.spinner(format!("Inspecting policy {}", args.policy.display()));
    let compiled = super::compile_policies(std::slice::from_ref(&args.policy))?;
    let output = inspect_output(&compiled.engine)?;
    let format = match args.format {
        Format::Pretty => OutputFormat::Pretty,
        Format::Json => OutputFormat::Json,
    };

    let rendered = output.format(format)?;
    ui.finish(
        spinner,
        format!("Inspected policy {}", args.policy.display()),
    );
    println!("{rendered}");
    Ok(())
}

fn inspect_output(
    engine: &ghrg_core::policy::Engine<ghrg_core::policy::Finished>,
) -> Result<CommandOutput> {
    #[derive(Serialize)]
    struct PolicyInspectView {
        path: String,
        package: String,
        metadata_present: bool,
        metadata_source: String,
        metadata_name: Option<String>,
        metadata_description: Option<String>,
        contexts: Vec<super::ContextInspectView>,
    }
    let policy = engine
        .policies()
        .first()
        .expect("inspect engine should contain one policy");
    let metadata = policy.metadata.as_ref();

    let metadata_name = metadata.and_then(|loaded| loaded.metadata.name.clone());

    let metadata_description = metadata.and_then(|loaded| loaded.metadata.description.clone());
    let contexts = super::inspect_contexts(metadata)?;

    let view = PolicyInspectView {
        path: policy.path.display().to_string(),
        package: policy.package.clone(),
        metadata_present: metadata.is_some(),
        metadata_source: super::metadata_source_label(metadata),
        metadata_name,
        metadata_description,
        contexts,
    };

    let record = OutputRecord::from_serializable(&view)?
        .with_meta(serde_json::to_value(&view).into_diagnostic()?);

    Ok(CommandOutput::record(
        "policy inspect",
        Some("Policy Inspect".to_string()),
        record,
    ))
}
