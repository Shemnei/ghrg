use clap::Args as ClapArgs;
use ghrg_core::policy::{OutcomeVisitor, RunOutcome};
use miette::{IntoDiagnostic, Result};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use tracing::debug;

use crate::output::{CommandOutput, OutputFormat, OutputRecord};
use crate::ui::Ui;

#[derive(Debug, ClapArgs)]
#[command(
    about = "Evaluate one or more policies against local JSON input",
    long_about = "Run a policy chain entirely locally using JSON input from disk. This is the main policy authoring loop before scanning real repositories from GitHub.",
    after_help = "Example:\n  ghrg policy test --policy examples/policies/filter-active.rego --policy examples/policies/project-summary.rego --input examples/inputs/repo.json --format json"
)]
pub struct Args {
    #[arg(
        long,
        required = true,
        help = "Policy file to apply; pass multiple times to evaluate a chain in order"
    )]
    pub policy: Vec<PathBuf>,

    #[arg(long, help = "Path to local JSON input")]
    pub input: PathBuf,

    #[arg(
        long,
        default_value = "pretty",
        help = "Render the final result as pretty output, JSON, or raw debug output"
    )]
    pub format: Format,

    #[arg(
        long,
        help = "Show the dropped object and dropping policy when evaluation fails"
    )]
    pub show_dropped: bool,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Format {
    Pretty,
    Json,
    Raw,
}

pub async fn run(args: &Args) -> Result<()> {
    let ui = Ui::new();
    let spinner = ui.spinner(format!(
        "Testing {} polic{}",
        args.policy.len(),
        if args.policy.len() == 1 { "y" } else { "ies" }
    ));
    debug!(input = %args.input.display(), "policy test uses local input; cache is not consulted");

    let input = super::read_json_input(&args.input)?;
    let compiled = super::compile_policies(&args.policy)?;
    let resolver = super::LocalSampleResolver::from_input(&input);
    let outcome = compiled
        .engine
        .run(&input, &resolver, OutcomeVisitor)
        .await?;
    let output = test_output(outcome, args.show_dropped)?;
    let format = match args.format {
        Format::Pretty => OutputFormat::Pretty,
        Format::Json => OutputFormat::Json,
        Format::Raw => OutputFormat::Raw,
    };

    let rendered = output.format(format)?;
    ui.finish(
        spinner,
        format!("Evaluated policy test input {}", args.input.display()),
    );
    println!("{rendered}");
    Ok(())
}

fn test_output(outcome: RunOutcome, show_dropped: bool) -> Result<CommandOutput> {
    #[derive(Serialize)]
    struct PolicyTestView {
        keep: bool,
        dropped_by: Option<String>,
        final_output: Value,
    }

    let dropped_by = if outcome.keep || show_dropped {
        outcome
            .dropped_by
            .as_ref()
            .map(|path| path.display().to_string())
    } else {
        None
    };

    let visible_output = if outcome.keep || show_dropped {
        outcome.result.object.json_value()
    } else {
        Value::Null
    };

    let view = PolicyTestView {
        keep: outcome.keep,
        dropped_by,
        final_output: visible_output,
    };

    let record = OutputRecord::from_serializable(&view)?
        .with_meta(serde_json::to_value(&view).into_diagnostic()?);

    Ok(CommandOutput::record(
        "policy test",
        Some("Policy Test".to_string()),
        record,
    ))
}
