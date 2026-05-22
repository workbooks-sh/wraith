use crate::params::ProjectInfoParams;

use super::push_global;

/// Build CLI arguments for the `project_info` tool.
pub fn build_project_info_args(params: &ProjectInfoParams) -> Vec<String> {
    let mut args = vec![
        "list".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    if params.entry_points == Some(true) {
        args.push("--entry-points".to_string());
    }
    if params.files == Some(true) {
        args.push("--files".to_string());
    }
    if params.plugins == Some(true) {
        args.push("--plugins".to_string());
    }
    if params.boundaries == Some(true) {
        args.push("--boundaries".to_string());
    }

    args
}
