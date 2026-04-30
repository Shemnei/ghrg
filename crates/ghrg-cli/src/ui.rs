use console::style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Ui;

impl Ui {
    pub fn new() -> Self {
        Self
    }

    pub fn spinner(&self, message: impl Into<String>) -> ProgressBar {
        let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
        bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan.bold} {msg}")
                .expect("spinner template should be valid")
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.set_message(message.into());
        bar.enable_steady_tick(Duration::from_millis(100));
        bar
    }

    pub fn progress(&self, length: u64, message: impl Into<String>) -> ProgressBar {
        let bar = ProgressBar::with_draw_target(Some(length), ProgressDrawTarget::stderr());
        bar.set_style(
            ProgressStyle::with_template("{msg:.bold} {bar:32.cyan/blue} {pos}/{len} {wide_msg}")
                .expect("progress template should be valid")
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        bar.set_message(message.into());
        bar
    }

    pub fn finish(&self, bar: ProgressBar, message: impl Into<String>) {
        bar.finish_and_clear();
        eprintln!("{} {}", style("done").bold().green(), style(message.into()));
    }
}
