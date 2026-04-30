mod repos;

use clap::{Args as ClapArgs, Subcommand};
use miette::Result;

use crate::cli::Cli;
use crate::runtime::RuntimeInfo;

#[derive(Debug, ClapArgs)]
#[command(
    about = "List supported context kinds and inspect their shapes",
    long_about = "Explore the context kinds that `ghrg` can attach during policy evaluation. Use this to discover supported repo contexts, inspect sample shapes, and copy example metadata snippets without opening the docs.",
    after_help = "Examples:\n  ghrg contexts repos list\n  ghrg contexts repos show properties\n  ghrg contexts repos show files --format json"
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "List and inspect repository context kinds")]
    Repos(repos::Args),
}

pub async fn run(_cli: &Cli, _runtime: &RuntimeInfo, args: &Args) -> Result<()> {
    match &args.command {
        Command::Repos(args) => repos::run(args).await,
    }
}
