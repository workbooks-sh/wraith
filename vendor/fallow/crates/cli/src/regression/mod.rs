mod baseline;
mod counts;
mod outcome;
mod tolerance;

// Re-exports for the public library API (lib.rs). The binary target does not
// use every re-exported symbol directly, so we suppress unused-import warnings
// that only fire when this module is compiled as part of the bin crate.
#[allow(unused_imports, reason = "re-exports for lib.rs public API")]
pub use baseline::load_regression_baseline;
pub use baseline::{
    RegressionOpts, SaveRegressionTarget, compare_check_regression, save_baseline_to_config,
    save_regression_baseline,
};
pub use counts::CheckCounts;
#[allow(unused_imports, reason = "re-exports for lib.rs public API")]
pub use counts::{DupesCounts, RegressionBaseline};
pub use outcome::{RegressionOutcome, print_regression_outcome};
pub use tolerance::Tolerance;
