//! `diff-cluster` refactor — structural divergence analysis for
//! similar-but-not-identical fn clusters (wb-5lgj.30 part A).
//!
//! Reads every member of a cluster and emits a list of token-level
//! divergences. The agent driving wraith picks the unified signature
//! based on this report, then calls `extract-shared` (part B) to do
//! the mechanical rewrite.

use std::collections::BTreeMap;
use std::path::PathBuf;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;
use serde::Serialize;

use crate::dedupe::DedupeError;
use crate::report::{ClusterMember, Finding, FindingKind};

/// A flattened-token position label. We don't try to give a
/// human-grammar name to the position — just a stable index plus
/// nesting breadcrumb so the agent can correlate divergences across
/// members.
#[derive(Debug, Clone, Serialize)]
pub struct Position {
    /// Linear index into the flattened token sequence.
    pub index: usize,
    /// Slash-delimited breadcrumb of delimiter nesting (e.g. `{/(`),
    /// suitable for human eyes when scanning the report.
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum DivergenceKind {
    /// Each member has a different string literal at this position.
    StringLiteral,
    /// Each member has a different integer/float literal here.
    Numeric,
    /// Each member has a different identifier here. Blocking — the
    /// agent must redesign the call shape.
    Identifier,
    /// Members' subtrees disagree on structure (different token kinds
    /// at the same position). Blocking.
    Structural,
}

#[derive(Debug, Clone, Serialize)]
pub struct Divergence {
    pub position: Position,
    #[serde(flatten)]
    pub kind: DivergenceKind,
    /// Per-member value at this position (rendered).
    pub values_by_member: BTreeMap<String, String>,
    /// Wraith's suggestion for the parameter type if non-blocking.
    pub suggested_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffReport {
    pub cluster_id: String,
    pub members: Vec<ClusterMember>,
    pub divergences: Vec<Divergence>,
    pub common_skeleton_lines: usize,
    pub ready_for_extract_shared: bool,
    pub blockers: Vec<String>,
}

pub fn diff_cluster_from_finding(
    cluster_id: &str,
    finding: &Finding,
) -> Result<DiffReport, DedupeError> {
    let members = match &finding.kind {
        FindingKind::DuplicateCluster { members, .. } => members.clone(),
        _ => return Err(DedupeError::NotACluster),
    };
    diff_members(cluster_id, &members)
}

pub fn diff_members(
    cluster_id: &str,
    members: &[ClusterMember],
) -> Result<DiffReport, DedupeError> {
    let resolved: Vec<ResolvedFn> = members
        .iter()
        .map(resolve_member)
        .collect::<Result<_, _>>()?;

    let token_lists: Vec<Vec<FlatTok>> = resolved
        .iter()
        .map(|r| flatten_tokens(&r.body_tokens))
        .collect();

    let divergences = walk_and_classify(members, &token_lists);
    let common_skeleton_lines = estimate_common_skeleton_lines(&resolved, &divergences);
    let blockers: Vec<String> = divergences
        .iter()
        .filter(|d| matches!(d.kind, DivergenceKind::Identifier | DivergenceKind::Structural))
        .map(|d| d.position.path.clone())
        .collect();
    let ready_for_extract_shared = blockers.is_empty();

    Ok(DiffReport {
        cluster_id: cluster_id.to_string(),
        members: members.to_vec(),
        divergences,
        common_skeleton_lines,
        ready_for_extract_shared,
        blockers,
    })
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ResolvedFn {
    pub member: ClusterMember,
    pub file: PathBuf,
    pub item: syn::ItemFn,
    /// Token stream of the fn body (block contents only).
    pub body_tokens: TokenStream,
}

pub(crate) fn resolve_member(m: &ClusterMember) -> Result<ResolvedFn, DedupeError> {
    let file = PathBuf::from(&m.file);
    let src = std::fs::read_to_string(&file).map_err(|e| DedupeError::Io {
        path: file.clone(),
        source: e,
    })?;
    let ast = syn::parse_file(&src).map_err(|e| DedupeError::Parse {
        path: file.clone(),
        source: e,
    })?;
    let leaf = m.symbol.rsplit("::").next().unwrap_or(&m.symbol).to_string();
    let item = find_fn(&ast, &leaf, m.line).ok_or_else(|| DedupeError::MemberNotFound {
        symbol: m.symbol.clone(),
        file: file.clone(),
    })?;
    let mut ts = TokenStream::new();
    item.block.to_tokens(&mut ts);
    Ok(ResolvedFn {
        member: m.clone(),
        file,
        item: item.clone(),
        body_tokens: ts,
    })
}

fn find_fn(file: &syn::File, leaf: &str, line: usize) -> Option<syn::ItemFn> {
    let mut found: Option<syn::ItemFn> = None;
    walk_fns(&file.items, &mut |f| {
        if f.sig.ident == leaf {
            let l = f.sig.ident.span().start().line;
            if l == line {
                found = Some(f.clone());
            } else if found.is_none() {
                found = Some(f.clone());
            }
        }
    });
    found
}

fn walk_fns<F: FnMut(&syn::ItemFn)>(items: &[syn::Item], cb: &mut F) {
    for it in items {
        match it {
            syn::Item::Fn(f) => cb(f),
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    walk_fns(items, cb);
                }
            }
            _ => {}
        }
    }
}

/// Flattened token tree entry: a single TokenTree plus a breadcrumb
/// describing the delimiter nesting it lives inside.
#[derive(Debug, Clone)]
pub(crate) struct FlatTok {
    pub path: String,
    pub tt: TokenTree,
}

pub(crate) fn flatten_tokens(ts: &TokenStream) -> Vec<FlatTok> {
    let mut out = Vec::new();
    flatten_inner(ts, "", &mut out);
    out
}

fn flatten_inner(ts: &TokenStream, path_prefix: &str, out: &mut Vec<FlatTok>) {
    for tt in ts.clone() {
        match &tt {
            TokenTree::Group(g) => {
                let open = match g.delimiter() {
                    Delimiter::Parenthesis => "(",
                    Delimiter::Brace => "{",
                    Delimiter::Bracket => "[",
                    Delimiter::None => "_",
                };
                out.push(FlatTok {
                    path: format!("{}{}open", path_prefix, open),
                    tt: tt.clone(),
                });
                let nested = format!("{}{}/", path_prefix, open);
                flatten_inner(&g.stream(), &nested, out);
                let close = match g.delimiter() {
                    Delimiter::Parenthesis => ")",
                    Delimiter::Brace => "}",
                    Delimiter::Bracket => "]",
                    Delimiter::None => "_",
                };
                out.push(FlatTok {
                    path: format!("{}{}close", path_prefix, close),
                    tt: tt.clone(),
                });
            }
            _ => out.push(FlatTok {
                path: path_prefix.to_string(),
                tt: tt.clone(),
            }),
        }
    }
}

fn walk_and_classify(
    members: &[ClusterMember],
    lists: &[Vec<FlatTok>],
) -> Vec<Divergence> {
    let mut out = Vec::new();
    if lists.is_empty() {
        return out;
    }
    let max_len = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let min_len = lists.iter().map(|l| l.len()).min().unwrap_or(0);

    for i in 0..min_len {
        let toks: Vec<&TokenTree> = lists.iter().map(|l| &l[i].tt).collect();
        if all_identical(&toks) {
            continue;
        }
        let kind = classify(&toks);
        let values_by_member: BTreeMap<String, String> = members
            .iter()
            .zip(lists.iter())
            .map(|(m, l)| (m.symbol.clone(), slot_text(&l[i].tt)))
            .collect();
        let suggested_type = match &kind {
            DivergenceKind::StringLiteral => Some("&'static str".to_string()),
            DivergenceKind::Numeric => Some(infer_numeric_type(&toks)),
            _ => None,
        };
        out.push(Divergence {
            position: Position {
                index: i,
                path: lists[0][i].path.clone(),
            },
            kind,
            values_by_member,
            suggested_type,
        });
    }

    if max_len != min_len {
        // Length mismatch is a structural divergence — log it once at
        // the first index past the shortest list.
        let values_by_member: BTreeMap<String, String> = members
            .iter()
            .zip(lists.iter())
            .map(|(m, l)| (m.symbol.clone(), format!("<{} tokens>", l.len())))
            .collect();
        out.push(Divergence {
            position: Position {
                index: min_len,
                path: "<length>".to_string(),
            },
            kind: DivergenceKind::Structural,
            values_by_member,
            suggested_type: None,
        });
    }

    out
}

fn all_identical(toks: &[&TokenTree]) -> bool {
    if toks.is_empty() {
        return true;
    }
    let first = slot_text(toks[0]);
    toks.iter().all(|t| slot_text(t) == first)
}

/// Render a token as the SLOT-identity used to compare across members.
/// For groups, we treat the slot as just the delimiter (the open/close
/// boundary); the inner contents are walked as nested slots.
fn slot_text(t: &TokenTree) -> String {
    match t {
        TokenTree::Group(g) => match g.delimiter() {
            Delimiter::Parenthesis => "<group:()>".into(),
            Delimiter::Brace => "<group:{}>".into(),
            Delimiter::Bracket => "<group:[]>".into(),
            Delimiter::None => "<group:_>".into(),
        },
        TokenTree::Ident(i) => i.to_string(),
        TokenTree::Literal(l) => l.to_string(),
        TokenTree::Punct(p) => p.as_char().to_string(),
    }
}

fn classify(toks: &[&TokenTree]) -> DivergenceKind {
    let kinds: Vec<TokKind> = toks.iter().map(|t| tok_kind(t)).collect();
    let first = &kinds[0];
    if !kinds.iter().all(|k| std::mem::discriminant(k) == std::mem::discriminant(first)) {
        return DivergenceKind::Structural;
    }
    match first {
        TokKind::LitStr => DivergenceKind::StringLiteral,
        TokKind::LitNum => DivergenceKind::Numeric,
        TokKind::Ident => DivergenceKind::Identifier,
        _ => DivergenceKind::Structural,
    }
}

#[derive(Debug)]
enum TokKind {
    Ident,
    LitStr,
    LitNum,
    Punct,
    Group,
}

fn tok_kind(t: &TokenTree) -> TokKind {
    match t {
        TokenTree::Ident(_) => TokKind::Ident,
        TokenTree::Literal(l) => {
            let s = l.to_string();
            if s.starts_with('"') || s.starts_with("r\"") || s.starts_with("r#") || s.starts_with("b\"") {
                TokKind::LitStr
            } else {
                TokKind::LitNum
            }
        }
        TokenTree::Punct(_) => TokKind::Punct,
        TokenTree::Group(_) => TokKind::Group,
    }
}

pub(crate) fn token_text(t: &TokenTree) -> String {
    match t {
        TokenTree::Group(g) => g.to_string(),
        TokenTree::Ident(i) => i.to_string(),
        TokenTree::Literal(l) => l.to_string(),
        TokenTree::Punct(p) => p.as_char().to_string(),
    }
}

fn infer_numeric_type(toks: &[&TokenTree]) -> String {
    let mut has_float = false;
    let mut max_bits = 32;
    for t in toks {
        let s = token_text(t);
        let raw = s.split(|c: char| c == 'i' || c == 'u' || c == 'f').next().unwrap_or(&s);
        if s.contains('.') || s.ends_with("f32") || s.ends_with("f64") {
            has_float = true;
        }
        if let Ok(n) = raw.replace('_', "").parse::<i128>() {
            if n.unsigned_abs() > u32::MAX as u128 {
                max_bits = 64;
            }
        }
    }
    if has_float {
        if max_bits >= 64 { "f64".to_string() } else { "f32".to_string() }
    } else if max_bits >= 64 {
        "i64".to_string()
    } else {
        "i32".to_string()
    }
}

fn estimate_common_skeleton_lines(resolved: &[ResolvedFn], _divs: &[Divergence]) -> usize {
    if resolved.is_empty() {
        return 0;
    }
    let m = &resolved[0];
    let start = m.item.sig.fn_token.span.start().line;
    let end = m.item.block.brace_token.span.close().end().line;
    (end - start) + 1
}

/// Render the diff report as markdown for terminal consumption.
pub fn render_markdown(rep: &DiffReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("# cluster {}\n\n", rep.cluster_id));
    out.push_str(&format!("**Members ({}):**\n\n", rep.members.len()));
    for m in &rep.members {
        out.push_str(&format!("- `{}` ({}:{})\n", m.symbol, m.file, m.line));
    }
    out.push_str(&format!(
        "\n**ready_for_extract_shared:** {}\n",
        rep.ready_for_extract_shared
    ));
    if !rep.blockers.is_empty() {
        out.push_str("\n**Blockers:**\n");
        for b in &rep.blockers {
            out.push_str(&format!("- {}\n", b));
        }
    }
    out.push_str(&format!(
        "\n**Common skeleton lines:** {}\n",
        rep.common_skeleton_lines
    ));
    out.push_str(&format!(
        "\n## Divergences ({})\n\n",
        rep.divergences.len()
    ));
    if !rep.divergences.is_empty() {
        out.push_str("| position | kind | suggested_type | values |\n");
        out.push_str("|---|---|---|---|\n");
        for d in &rep.divergences {
            let kind = match d.kind {
                DivergenceKind::StringLiteral => "string-literal",
                DivergenceKind::Numeric => "numeric",
                DivergenceKind::Identifier => "identifier",
                DivergenceKind::Structural => "structural",
            };
            let suggested = d
                .suggested_type
                .clone()
                .unwrap_or_else(|| "—".to_string());
            let values = d
                .values_by_member
                .iter()
                .map(|(k, v)| format!("`{}`=`{}`", k, v))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "| `{}` (#{}) | {} | `{}` | {} |\n",
                d.position.path, d.position.index, kind, suggested, values
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_string_literal_divergence() {
        let a: TokenStream = "\"jpg\"".parse().unwrap();
        let b: TokenStream = "\"png\"".parse().unwrap();
        let la = flatten_tokens(&a);
        let lb = flatten_tokens(&b);
        let toks = vec![&la[0].tt, &lb[0].tt];
        match classify(&toks) {
            DivergenceKind::StringLiteral => {}
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn classifies_numeric_divergence() {
        let a: TokenStream = "100".parse().unwrap();
        let b: TokenStream = "200".parse().unwrap();
        let la = flatten_tokens(&a);
        let lb = flatten_tokens(&b);
        let toks = vec![&la[0].tt, &lb[0].tt];
        assert!(matches!(classify(&toks), DivergenceKind::Numeric));
    }
}
