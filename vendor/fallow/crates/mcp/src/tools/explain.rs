use crate::params::ExplainParams;

/// Build CLI arguments for the `fallow_explain` tool.
pub fn build_explain_args(params: &ExplainParams) -> Vec<String> {
    vec![
        "explain".to_string(),
        params.issue_type.clone(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ]
}
