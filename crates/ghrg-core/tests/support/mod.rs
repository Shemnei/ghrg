use ghrg_core::GhrgError;
use miette::{GraphicalReportHandler, GraphicalTheme};
use std::path::PathBuf;

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn render_diagnostic(error: &GhrgError) -> String {
    let mut output = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::none())
        .render_report(&mut output, error)
        .unwrap();
    output
}
