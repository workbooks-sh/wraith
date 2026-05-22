use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Progress reporter for analysis stages.
pub struct AnalysisProgress {
    multi: MultiProgress,
    enabled: bool,
}

impl AnalysisProgress {
    /// Create a new progress reporter.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            multi: MultiProgress::new(),
            enabled,
        }
    }

    /// Create a spinner for a stage.
    ///
    /// # Panics
    ///
    /// Panics if the progress template string is invalid (compile-time constant).
    #[must_use]
    pub fn stage_spinner(&self, message: &str) -> ProgressBar {
        if !self.enabled {
            return ProgressBar::hidden();
        }

        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .expect("valid progress template")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        pb
    }

    /// Finish all progress bars.
    pub fn finish(&self) {
        let _ = self.multi.clear();
    }
}

impl Default for AnalysisProgress {
    fn default() -> Self {
        Self::new(false)
    }
}
