use ghrg_core::GhrgError;
use miette::{GraphicalReportHandler, GraphicalTheme};
use std::path::PathBuf;

const SNAPSHOT_RENDER_WIDTH: usize = 512;

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub fn render_diagnostic(error: &GhrgError) -> String {
    let mut output = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::none())
        .with_width(SNAPSHOT_RENDER_WIDTH)
        .render_report(&mut output, error)
        .unwrap();

    normalize_diagnostic_output(output)
}

fn normalize_diagnostic_output(output: String) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    output
        .replace(manifest_dir, "$CARGO_MANIFEST_DIR")
        .replace("\r\n", "\n")
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::normalize_diagnostic_output;

    #[test]
    fn normalizes_manifest_paths_and_line_endings() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let input = format!(
            "path: {manifest_dir}\\tests\\fixtures\\bad.rego\r\nsource: $CARGO_MANIFEST_DIR\\tests\\fixtures\\bad.rego\r\n"
        );

        let output = normalize_diagnostic_output(input);

        assert_eq!(
            output,
            "path: $CARGO_MANIFEST_DIR/tests/fixtures/bad.rego\nsource: $CARGO_MANIFEST_DIR/tests/fixtures/bad.rego\n"
        );
    }
}
