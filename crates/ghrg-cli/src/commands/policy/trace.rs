use clap::Args as ClapArgs;
use comfy_table::{
    Attribute, Cell, Color, ContentArrangement, Row, Table, modifiers::UTF8_ROUND_CORNERS,
    presets::UTF8_FULL,
};
use console::style;
use ghrg_core::policy::{RunOutcome, RunStep};
use miette::{IntoDiagnostic, Result};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tracing::debug;

use crate::output::{CommandOutput, OutputFormat, OutputRecord};
use crate::ui::Ui;

#[derive(Debug, ClapArgs)]
#[command(
    about = "Trace each evaluation step in a local policy chain",
    long_about = "Run a policy chain against local JSON input and print each step's decision, metadata, requested contexts, elapsed time, and visible output. Use this when a policy chain keeps or drops data in an unexpected way.",
    after_help = "Example:\n  ghrg policy trace --policy examples/policies/filter-active.rego --policy examples/policies/project-summary.rego --input examples/inputs/repo.json"
)]
pub struct Args {
    #[arg(
        long,
        required = true,
        help = "Policy file to apply; pass multiple times to trace a chain in order"
    )]
    pub policy: Vec<PathBuf>,

    #[arg(long, help = "Path to local JSON input")]
    pub input: PathBuf,

    #[arg(long, help = "Write the rendered trace to a file instead of stdout")]
    pub output: Option<PathBuf>,

    #[arg(
        long,
        default_value = "pretty",
        help = "Render the trace as human-readable output or JSON"
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
    let spinner = ui.spinner(format!(
        "Tracing {} polic{}",
        args.policy.len(),
        if args.policy.len() == 1 { "y" } else { "ies" }
    ));
    debug!(input = %args.input.display(), "policy trace uses local input; cache is not consulted");

    let input = super::read_json_input(&args.input)?;
    let compiled = super::compile_policies(&args.policy)?;
    let resolver = super::LocalSampleResolver::from_input(&input);
    let outcome = compiled
        .engine
        .run(&input, &resolver, ghrg_core::policy::OutcomeVisitor)
        .await?;
    let rendered = match args.format {
        Format::Pretty => render_pretty_trace(&outcome).into_diagnostic()?,
        Format::Json => trace_output(outcome)?.format(OutputFormat::Json)?,
    };

    if let Some(path) = &args.output {
        fs::write(path, rendered).into_diagnostic()?;
        ui.finish(spinner, format!("Wrote policy trace to {}", path.display()));
    } else {
        ui.finish(
            spinner,
            format!("Traced policy input {}", args.input.display()),
        );
        println!("{rendered}");
    }

    Ok(())
}

fn trace_output(outcome: RunOutcome) -> Result<CommandOutput> {
    #[derive(Serialize)]
    struct TraceStepView {
        policy: String,
        package: String,
        metadata_source: String,
        contexts: Vec<super::ContextInspectView>,
        keep: bool,
        elapsed_ms: u128,
        output: Value,
    }

    #[derive(Serialize)]
    struct PolicyTraceView {
        keep: bool,
        dropped_by: Option<String>,
        policy_order: Vec<String>,
        final_output: Value,
        evaluations: Vec<TraceStepView>,
    }

    let evaluations = outcome
        .steps
        .iter()
        .map(|step| {
            let metadata = step.metadata.as_ref();

            Ok(TraceStepView {
                policy: step.policy.display().to_string(),
                package: step.package.clone(),
                metadata_source: super::metadata_source_label(metadata),
                contexts: super::inspect_contexts(metadata)?,
                keep: step.keep,
                elapsed_ms: step.elapsed_ms,
                output: step.output.object.json_value(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let view = PolicyTraceView {
        keep: outcome.keep,
        dropped_by: outcome
            .dropped_by
            .as_ref()
            .map(|path| path.display().to_string()),
        policy_order: outcome
            .steps
            .iter()
            .map(|step| step.policy.display().to_string())
            .collect(),
        final_output: outcome.result.object.json_value(),
        evaluations,
    };

    let record = OutputRecord::from_serializable(&view)?
        .with_meta(serde_json::to_value(&view).into_diagnostic()?);

    Ok(CommandOutput::record(
        "policy trace",
        Some("Policy Trace".to_string()),
        record,
    ))
}

fn render_pretty_trace(outcome: &RunOutcome) -> std::result::Result<String, serde_json::Error> {
    let mut sections = Vec::new();
    sections.push(style("Policy Trace").bold().underlined().to_string());
    sections.push(render_trace_summary(outcome));

    if !outcome.steps.is_empty() {
        sections.push(render_policy_order(outcome));
    }

    for (index, step) in outcome.steps.iter().enumerate() {
        sections.push(render_trace_step(index + 1, step)?);
    }

    sections.push(style("Final Output").bold().green().to_string());
    sections.push(render_json_panel(&outcome.result.object.json_value())?);

    Ok(sections.join("\n\n"))
}

fn indent_block(value: &str, spaces: usize) -> Vec<String> {
    let indent = " ".repeat(spaces);
    value
        .lines()
        .map(|line| format!("{indent}{line}"))
        .collect()
}

fn render_trace_summary(outcome: &RunOutcome) -> String {
    let mut table = base_table();
    table.set_header(vec![header_cell("Result"), header_cell("Value")]);
    table.add_row(Row::from(vec![
        key_cell("keep"),
        decision_cell(outcome.keep),
    ]));
    table.add_row(Row::from(vec![
        key_cell("dropped by"),
        Cell::new(
            outcome
                .dropped_by
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string()),
        ),
    ]));
    table.add_row(Row::from(vec![
        key_cell("policy count"),
        Cell::new(outcome.steps.len().to_string()).fg(Color::Cyan),
    ]));
    table.to_string()
}

fn render_policy_order(outcome: &RunOutcome) -> String {
    let mut table = base_table();
    table.set_header(vec![header_cell("#"), header_cell("Policy")]);
    for (index, step) in outcome.steps.iter().enumerate() {
        table.add_row(Row::from(vec![
            Cell::new((index + 1).to_string()).fg(Color::Cyan),
            Cell::new(step.policy.display().to_string()),
        ]));
    }
    format!("{}\n{}", style("Policy Order").bold().yellow(), table)
}

fn render_trace_step(
    index: usize,
    step: &RunStep,
) -> std::result::Result<String, serde_json::Error> {
    let mut table = base_table();
    table.set_header(vec![header_cell("Field"), header_cell("Value")]);
    table.add_row(Row::from(vec![
        key_cell("package"),
        Cell::new(&step.package),
    ]));
    table.add_row(Row::from(vec![
        key_cell("decision"),
        decision_cell(step.keep),
    ]));
    table.add_row(Row::from(vec![
        key_cell("elapsed"),
        Cell::new(format!("{} ms", step.elapsed_ms)).fg(Color::Cyan),
    ]));
    table.add_row(Row::from(vec![
        key_cell("metadata"),
        Cell::new(super::metadata_source_label(step.metadata.as_ref())),
    ]));

    let contexts = step
        .metadata
        .as_ref()
        .map(|loaded| {
            loaded
                .metadata
                .contexts
                .iter()
                .map(render_trace_context)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "none".to_string());
    table.add_row(Row::from(vec![key_cell("contexts"), Cell::new(contexts)]));

    let mut lines = Vec::new();
    let border = style("═".repeat(88)).dim().to_string();
    lines.push(border.clone());
    lines.push(format!(
        "{} {}",
        style(format!("Step {index}")).bold().yellow(),
        style(step.policy.display()).bold().blue()
    ));
    lines.push(border);
    lines.push(table.to_string());
    lines.push(String::new());
    lines.push(style("Output").bold().green().to_string());
    lines.push(render_json_panel(&step.output.object.json_value())?);
    lines.push(
        style(if step.keep {
            "policy kept the object"
        } else {
            "policy dropped the object"
        })
        .dim()
        .to_string(),
    );
    Ok(lines.join("\n"))
}

fn render_trace_context(context: &ghrg_core::policy::ContextSpec) -> String {
    match super::inspect_context(context) {
        Ok(view) => {
            let mut lines = vec![format!("{} ({})", view.input_key, view.kind)];
            if let Some(summary) = view.summary {
                lines.push(format!("  summary: {summary}"));
            }
            if !view.validation_rules.is_empty() {
                lines.push(format!("  rules: {}", view.validation_rules.join("; ")));
            }
            lines.join("\n")
        }
        Err(_) => context.render(),
    }
}

fn render_json_panel(value: &Value) -> std::result::Result<String, serde_json::Error> {
    let rendered = serde_json::to_string_pretty(value)?;
    let mut lines = Vec::new();
    lines.push(style("┌ JSON").dim().to_string());
    lines.extend(indent_block(&rendered, 2));
    lines.push(style("└").dim().to_string());
    Ok(lines.join("\n"))
}

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

fn header_cell(label: &str) -> Cell {
    Cell::new(label)
        .fg(Color::Cyan)
        .add_attribute(Attribute::Bold)
}

fn key_cell(label: &str) -> Cell {
    Cell::new(label).add_attribute(Attribute::Bold)
}

fn decision_cell(keep: bool) -> Cell {
    if keep {
        Cell::new("keep")
            .fg(Color::Green)
            .add_attribute(Attribute::Bold)
    } else {
        Cell::new("drop")
            .fg(Color::Red)
            .add_attribute(Attribute::Bold)
    }
}
