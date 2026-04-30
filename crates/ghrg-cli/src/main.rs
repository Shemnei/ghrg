#![allow(clippy::result_large_err)]
#![allow(clippy::large_enum_variant)]

mod auth;
mod cli;
mod commands;
mod output;
mod runtime;
mod ui;

use clap::Parser;
use miette::IntoDiagnostic;

#[tokio::main]
async fn main() -> miette::Result<()> {
    miette::set_panic_hook();
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .build(),
        )
    }))
    .into_diagnostic()?;

    let cli = cli::Cli::parse();
    commands::run(cli).await
}
