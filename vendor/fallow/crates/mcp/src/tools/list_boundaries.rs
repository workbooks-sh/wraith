use crate::params::ListBoundariesParams;

use super::push_global;

pub fn build_list_boundaries_args(params: &ListBoundariesParams) -> Vec<String> {
    let mut args = vec![
        "list".to_string(),
        "--boundaries".to_string(),
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

    args
}
