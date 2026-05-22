use crate::params::CheckChangedParams;

use super::{push_baseline, push_global, push_regression, push_scope};

/// Build CLI arguments for the `check_changed` tool.
pub fn build_check_changed_args(params: CheckChangedParams) -> Vec<String> {
    let mut args = vec![
        "dead-code".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
        "--changed-since".to_string(),
        params.since,
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_scope(&mut args, params.production, params.workspace.as_deref());
    push_baseline(
        &mut args,
        params.baseline.as_deref(),
        params.save_baseline.as_deref(),
    );
    push_regression(
        &mut args,
        params.fail_on_regression,
        params.tolerance.as_deref(),
        params.regression_baseline.as_deref(),
        params.save_regression_baseline.as_deref(),
    );

    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }

    args
}
