use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use wraith_core::analyze::{analyze_root, find_dead_code, find_unused_deps};
use wraith_core::audit::run_audit;
use wraith_core::boundaries::find_boundary_violations;
use wraith_core::circular::{find_crate_cycles, find_module_cycles};
use wraith_core::config::Config;
use wraith_core::dupes::{find_duplicate_clusters, find_duplicates};
use wraith_core::fix;
use wraith_core::health::{
    find_complexity_hotspots, health_show_branches, health_suggest_extractions,
};
use wraith_core::queries;
use wraith_core::report::Finding;
use wraith_core::workspace::Workspace;

mod cargo_tools;
mod fallow;
mod hooks;
mod migrate;
mod report_md;
mod watch;

#[derive(Parser, Debug)]
#[command(
    name = "wraith",
    version,
    about = "Rust + TS/JS codebase analyzer — dead code, unused deps, cycles, dupes, complexity, boundaries."
)]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,

    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Format {
    Human,
    Json,
    Jsonl,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum CiKind {
    Github,
    Gitlab,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum MigrateFrom {
    Clippy,
    Deny,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Find pub items with no references in the workspace.
    DeadCode,
    /// Find dependencies in Cargo.toml that aren't imported.
    UnusedDeps,
    /// Detect circular dependencies between crates and inside modules.
    CircularDeps,
    /// Token-shingled clone detection at the fn-body level.
    Dupes {
        /// Emit raw pair findings instead of transitive clusters.
        #[arg(long)]
        pairs: bool,
    },
    /// Flag fns above cyclomatic / cognitive complexity thresholds.
    Health {
        /// Print a structured tree of decision points for a single fn
        /// instead of the workspace-wide hotspot list.
        #[arg(long = "fn", value_name = "FN_PATH")]
        fn_: Option<String>,
        /// Required alongside --fn: emit the branch tree.
        #[arg(long)]
        show_branches: bool,
        /// Rank extractable sub-trees inside the target fn for an
        /// AI agent to drive `wraith refactor extract-fn` against.
        #[arg(long)]
        suggest_extractions: bool,
        /// Cap on suggestions returned (default 10).
        #[arg(long, default_value_t = 10)]
        max_suggestions: usize,
    },
    /// Enforce module-import boundary rules from `.wraithrc.json`.
    Boundaries,
    /// Auto-remove dead pub items + unused deps (dry-run by default).
    Fix {
        #[arg(long)]
        apply: bool,
    },
    /// Run dead-code + unused-deps on git-changed files only.
    Audit {
        #[arg(long)]
        exit_zero: bool,
    },
    /// Generate a .wraithrc.json with defaults.
    Init {
        #[arg(long)]
        force: bool,
        #[arg(long, value_enum)]
        ci: Option<CiKind>,
    },
    /// Install git hook + Claude Code hook for incremental audits.
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
    /// Translate other tools' configs into `.wraithrc.json`.
    Migrate {
        #[arg(long, value_enum)]
        from: MigrateFrom,
    },
    /// Re-run analysis on file save. Emits jsonl on stdout.
    Watch {
        #[arg(long, default_value = "audit")]
        target: String,
    },
    /// Source-level refactors that operate on a single Rust file.
    Refactor {
        #[command(subcommand)]
        action: RefactorAction,
    },
    /// Run all detectors and emit a markdown report (qualitative +
    /// quantitative summary suitable for pasting into a README).
    Report {
        /// Diff mode: compare current state against findings at <ref>
        /// (any git revision — sha, tag, branch, HEAD~N). Emits a
        /// before/after report with resolved + introduced findings,
        /// LOC delta, complexity changes.
        #[arg(long, value_name = "GIT_REF")]
        since: Option<String>,
    },
    /// Read-only views over the reference graph: callers, callees,
    /// blast-radius, crate-deps, reverse-deps. Designed as agent input.
    Graph {
        #[command(subcommand)]
        action: GraphAction,
    },
    /// Token-economy: smallest useful "context window" for a symbol.
    /// Returns its definition, the file's imports, top-N callers and
    /// callees — so an agent doesn't have to read whole files.
    Ctx {
        /// Qualified symbol path (e.g. `wavelet::run_image`) or leaf name.
        symbol: String,
        /// Omit the body — return signature + neighbors only.
        #[arg(long = "no-body")]
        no_body: bool,
        /// Cap on callers/callees returned per side (default 5).
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Token-economy: structured summary of a Rust file — pub interface,
    /// imports, per-fn complexity, module deps, LOC.
    Summarize {
        /// Path to a single `.rs` file.
        file: PathBuf,
        /// Reserved — bodies are deferred to `wraith ctx`.
        #[arg(long, default_value_t = false)]
        include_bodies: bool,
    },
    /// Token-economy: list symbols matching a glob pattern. Replaces
    /// `grep -rn fn foo` with structured, kind-filtered results.
    Ls {
        /// Glob over qualified symbol path. `*` matches within a path
        /// segment, `**` across segments. Empty = all symbols.
        #[arg(default_value = "")]
        pattern: String,
        /// Restrict to one kind.
        #[arg(long)]
        kind: Option<String>,
    },
    /// Cargo dependency logistics: duplicates, security audit, unused
    /// features, binary-size attribution.
    Deps {
        #[command(subcommand)]
        action: DepsAction,
    },
    /// Suggest the tightest visibility for each pub item that still
    /// keeps every call-site working (`pub` → `pub(crate)` / private).
    Visibility {
        /// Rewrite the visibility tokens in place.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand, Debug)]
enum DepsAction {
    /// Crates resolved at multiple versions in Cargo.lock.
    Duplicates {
        #[arg(long, value_enum, default_value_t = DepsOutFormat::Human)]
        format: DepsOutFormat,
    },
    /// Security advisories (shells out to `cargo-audit`).
    Audit {
        #[arg(long, value_enum, default_value_t = DepsOutFormat::Human)]
        format: DepsOutFormat,
    },
    /// Heuristic flag for `features = [...]` entries with no usage.
    UnusedFeatures {
        #[arg(long, value_enum, default_value_t = DepsOutFormat::Human)]
        format: DepsOutFormat,
    },
    /// Binary-size attribution per crate (shells out to `cargo-bloat`).
    Size {
        #[arg(long, value_enum, default_value_t = DepsOutFormat::Human)]
        format: DepsOutFormat,
        /// Build in release mode (default debug).
        #[arg(long)]
        release: bool,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum GraphFormat {
    Json,
    Md,
    Dot,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum DepsOutFormat {
    Human,
    Json,
    Md,
}

impl From<DepsOutFormat> for cargo_tools::DepsFormat {
    fn from(f: DepsOutFormat) -> Self {
        match f {
            DepsOutFormat::Human => cargo_tools::DepsFormat::Human,
            DepsOutFormat::Json => cargo_tools::DepsFormat::Json,
            DepsOutFormat::Md => cargo_tools::DepsFormat::Md,
        }
    }
}

#[derive(Subcommand, Debug)]
enum GraphAction {
    /// Workspace crate dependency graph (which crate depends on which).
    CrateDeps {
        #[arg(long, value_enum, default_value_t = GraphFormat::Json)]
        format: GraphFormat,
    },
    /// Direct or transitive callers of <symbol>.
    Callers {
        symbol: String,
        #[arg(long)]
        transitive: bool,
        #[arg(long, value_enum, default_value_t = GraphFormat::Json)]
        format: GraphFormat,
    },
    /// Direct or transitive callees of <symbol>.
    Callees {
        symbol: String,
        #[arg(long)]
        transitive: bool,
        #[arg(long, value_enum, default_value_t = GraphFormat::Json)]
        format: GraphFormat,
    },
    /// Forward N-hop walk of dependents (transitive callers).
    BlastRadius {
        symbol: String,
        /// Max hops; omit for unlimited.
        #[arg(long)]
        depth: Option<usize>,
        #[arg(long, value_enum, default_value_t = GraphFormat::Json)]
        format: GraphFormat,
    },
    /// Crates (or modules) that depend on <target>.
    ReverseDeps {
        target: String,
        #[arg(long, value_enum, default_value_t = GraphFormat::Json)]
        format: GraphFormat,
    },
}

#[derive(Subcommand, Debug)]
enum RefactorAction {
    /// Extract a contiguous line range from an enclosing fn into a new fn.
    ///
    /// Usage: `wraith refactor extract-fn <file>:<start>..<end> --name <new_fn>`
    ExtractFn {
        /// `<file>:<line_start>..<line_end>` selector.
        selector: String,
        #[arg(long)]
        name: String,
        /// Print the rewritten file to stdout instead of editing in place.
        #[arg(long)]
        dry_run: bool,
    },
    /// Collapse a byte-identical duplicate cluster onto a single canonical
    /// definition.
    ///
    /// Usage: `wraith refactor dedupe-cluster <cluster> [--canonical=<spec>] [--dry-run]`
    /// where `<cluster>` is either the 0-based cluster index from
    /// `wraith dupes` or the qualified symbol of any member.
    DedupeCluster {
        /// Cluster index (0-based) OR qualified symbol of any member.
        cluster: String,
        /// Canonical selector: qualified symbol, leaf name, crate name,
        /// file path, or `<file>:<line>`.
        #[arg(long)]
        canonical: Option<String>,
        /// Print planned edits to stdout; do not write to disk.
        #[arg(long)]
        dry_run: bool,
        /// Refuse to auto-elevate the canonical's visibility. If the
        /// canonical is too restrictive for consumers, exit 64 instead.
        #[arg(long)]
        no_elevate: bool,
    },
    /// Structural divergence report for a similar-but-not-identical
    /// cluster. Agent reads this to pick a unified fn signature.
    ///
    /// Usage: `wraith refactor diff-cluster <cluster> [--format=json|md]`
    DiffCluster {
        cluster: String,
        #[arg(long, value_enum, default_value_t = DiffFormat::Json)]
        format: DiffFormat,
    },
    /// Agent-driven cluster unification — generate a shared fn from a
    /// supplied signature + param-mapping, delete the member defs, and
    /// rewrite call sites.
    ///
    /// Usage: `wraith refactor extract-shared <cluster>
    ///   --signature='fn name(p: &str) -> &str'
    ///   --param-mapping='{"member::sym": {"p": "\"value\""}}'
    ///   --extract-to=crate::util [--dry-run]`
    ExtractShared {
        cluster: String,
        #[arg(long)]
        signature: String,
        /// JSON map `{ "<member-symbol>": { "<param>": "<rust-expr>" } }`.
        /// Prefix with `@` to read from a file (`@path/to/mapping.json`).
        #[arg(long = "param-mapping")]
        param_mapping: String,
        #[arg(long = "extract-to")]
        extract_to: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Workspace-wide fn relocation (wb-5lgj.31).
    ///
    /// Usage: `wraith refactor move-fn <file>:<fn-name> --to <crate::module> [--cross-crate] [--dry-run]`
    MoveFn {
        /// `<src-file>:<fn-name>` selector.
        selector: String,
        /// Destination module path, e.g. `crate_b::shared`.
        #[arg(long = "to")]
        to: String,
        /// Allow a cross-crate move (refused by default).
        #[arg(long = "cross-crate", default_value_t = false)]
        cross_crate: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Workspace-wide symbol rename (wb-5lgj.31).
    ///
    /// Usage: `wraith refactor rename <symbol> --to <new-name> [--dry-run]`
    Rename {
        /// Symbol path or leaf name, e.g. `crate::foo` or `foo`.
        symbol: String,
        #[arg(long = "to")]
        to: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Replace every call site with the fn body and delete the fn
    /// (wb-5lgj.31).
    ///
    /// Usage: `wraith refactor inline <file>:<fn-name> [--dry-run]`
    Inline {
        /// `<src-file>:<fn-name>` selector.
        selector: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Split a fn into two at a statement boundary (wb-5lgj.31).
    ///
    /// Usage: `wraith refactor split-fn <file>:<fn-name>
    ///   --at-line <N> --names <first>,<second> [--dry-run]`
    SplitFn {
        /// `<src-file>:<fn-name>` selector.
        selector: String,
        #[arg(long = "at-line")]
        at_line: usize,
        /// Comma-separated pair of names for the two halves.
        #[arg(long)]
        names: String,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum DiffFormat {
    Json,
    Md,
}

#[derive(Subcommand, Debug)]
enum HooksAction {
    Install,
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("wraith: error: {:#}", e);
        std::process::exit(2);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.canonicalize().unwrap_or(cli.root.clone());
    let cfg = Config::load(&root)?;

    match cli.cmd {
        Cmd::DeadCode => {
            let (_ws, graph) = analyze_root(&root, &cfg)?;
            let mut findings = find_dead_code(&graph, &cfg);
            findings.extend(fallow::run(&root, "dead-code").unwrap_or_default());
            emit(&findings, cli.format);
            if !findings.is_empty() {
                std::process::exit(1);
            }
        }
        Cmd::UnusedDeps => {
            let (ws, graph) = analyze_root(&root, &cfg)?;
            let mut findings = find_unused_deps(&ws, &graph, &cfg);
            findings.extend(fallow::run(&root, "unused-deps").unwrap_or_default());
            emit(&findings, cli.format);
            if !findings.is_empty() {
                std::process::exit(1);
            }
        }
        Cmd::CircularDeps => {
            let (ws, graph) = analyze_root(&root, &cfg)?;
            let mut findings = find_crate_cycles(&ws);
            findings.extend(find_module_cycles(&ws, &graph));
            emit(&findings, cli.format);
            if !findings.is_empty() {
                std::process::exit(1);
            }
        }
        Cmd::Dupes { pairs } => {
            let ws = Workspace::load(&root)?;
            let crate_files: Vec<(String, Vec<PathBuf>)> = ws
                .crates
                .iter()
                .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
                .collect();
            let findings = if pairs {
                find_duplicates(&crate_files, &cfg.duplicates)
            } else {
                find_duplicate_clusters(&crate_files, &cfg.duplicates)
            };
            emit(&findings, cli.format);
            if !findings.is_empty() {
                std::process::exit(1);
            }
        }
        Cmd::Health {
            fn_,
            show_branches,
            suggest_extractions,
            max_suggestions,
        } => {
            let ws = Workspace::load(&root)?;
            let crate_files: Vec<(String, Vec<PathBuf>)> = ws
                .crates
                .iter()
                .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
                .collect();
            if suggest_extractions {
                let target = match fn_ {
                    Some(t) => t,
                    None => {
                        eprintln!(
                            "wraith: --suggest-extractions requires --fn <fn-path>"
                        );
                        std::process::exit(2);
                    }
                };
                match health_suggest_extractions(&crate_files, &target, max_suggestions) {
                    Some(s) => match cli.format {
                        Format::Json => {
                            println!("{}", serde_json::to_string_pretty(&s).unwrap());
                        }
                        Format::Jsonl => {
                            for sug in &s.suggestions {
                                println!("{}", serde_json::to_string(sug).unwrap());
                            }
                        }
                        Format::Human => {
                            print!("{}", s.render_markdown());
                        }
                    },
                    None => {
                        eprintln!("wraith: no fn matching `{}` found", target);
                        std::process::exit(1);
                    }
                }
            } else if show_branches || fn_.is_some() {
                let target = match fn_ {
                    Some(t) => t,
                    None => {
                        eprintln!("wraith: --show-branches requires --fn <fn-path>");
                        std::process::exit(2);
                    }
                };
                match health_show_branches(&crate_files, &target) {
                    Some(tree) => {
                        print!("{}", tree.render());
                    }
                    None => {
                        eprintln!("wraith: no fn matching `{}` found", target);
                        std::process::exit(1);
                    }
                }
            } else {
                let findings = find_complexity_hotspots(&crate_files, &cfg.complexity);
                emit(&findings, cli.format);
                if !findings.is_empty() {
                    std::process::exit(1);
                }
            }
        }
        Cmd::Boundaries => {
            let (ws, graph) = analyze_root(&root, &cfg)?;
            let findings = find_boundary_violations(&ws, &graph, &cfg.boundaries);
            emit(&findings, cli.format);
            if !findings.is_empty() {
                std::process::exit(1);
            }
        }
        Cmd::Fix { apply } => {
            let (ws, graph) = analyze_root(&root, &cfg)?;
            let mut findings = find_dead_code(&graph, &cfg);
            findings.extend(find_unused_deps(&ws, &graph, &cfg));
            let plan = fix::plan(&findings);
            if apply {
                let n = fix::apply(&plan)?;
                println!("applied {} edit(s)", n);
            } else {
                let s = serde_json::to_string_pretty(&plan)?;
                println!("{}", s);
                eprintln!(
                    "wraith: dry-run; pass --apply to write changes ({} edits planned)",
                    plan.edits.len()
                );
            }
        }
        Cmd::Audit { exit_zero } => {
            let ws = Workspace::load(&root)?;
            let mut findings = run_audit(&ws, &cfg)?;
            findings.extend(fallow::run(&root, "audit").unwrap_or_default());
            emit(&findings, cli.format);
            if !findings.is_empty() && !exit_zero {
                std::process::exit(1);
            }
        }
        Cmd::Init { force, ci } => {
            let path = root.join(".wraithrc.json");
            if !path.exists() || force {
                let cfg = Config::default_for_workspace();
                cfg.write(&root)?;
                println!("wrote {}", path.display());
            } else {
                eprintln!("wraith: .wraithrc.json already exists; pass --force to overwrite.");
            }
            ensure_gitignore_entry(&root, ".wraithrc.cache")?;
            if let Some(kind) = ci {
                hooks::write_ci_template(&root, kind)?;
            }
        }
        Cmd::Hooks { action } => match action {
            HooksAction::Install => {
                hooks::install_git_hook(&root)?;
                hooks::install_claude_hook(&root)?;
                println!("installed pre-commit + Claude Code hooks");
            }
        },
        Cmd::Migrate { from } => {
            migrate::run(&root, from)?;
        }
        Cmd::Watch { target } => {
            watch::run(&root, cfg, target)?;
        }
        Cmd::Refactor { action } => match action {
            RefactorAction::ExtractFn {
                selector,
                name,
                dry_run,
            } => run_extract_fn(&selector, &name, dry_run)?,
            RefactorAction::DedupeCluster {
                cluster,
                canonical,
                dry_run,
                no_elevate,
            } => run_dedupe_cluster(
                &root,
                &cfg,
                &cluster,
                canonical.as_deref(),
                dry_run,
                no_elevate,
            )?,
            RefactorAction::DiffCluster { cluster, format } => {
                run_diff_cluster(&root, &cfg, &cluster, format)?
            }
            RefactorAction::ExtractShared {
                cluster,
                signature,
                param_mapping,
                extract_to,
                dry_run,
            } => run_extract_shared(
                &root,
                &cfg,
                &cluster,
                &signature,
                &param_mapping,
                &extract_to,
                dry_run,
            )?,
            RefactorAction::MoveFn {
                selector,
                to,
                cross_crate,
                dry_run,
            } => run_move_fn(&root, &selector, &to, cross_crate, dry_run)?,
            RefactorAction::Rename {
                symbol,
                to,
                dry_run,
            } => run_rename(&root, &symbol, &to, dry_run)?,
            RefactorAction::Inline { selector, dry_run } => {
                run_inline(&root, &selector, dry_run)?
            }
            RefactorAction::SplitFn {
                selector,
                at_line,
                names,
                dry_run,
            } => run_split_fn(&selector, at_line, &names, dry_run)?,
        },
        Cmd::Report { since } => match since {
            None => run_report(&root, &cfg)?,
            Some(r) => run_report_diff(&root, &cfg, &r)?,
        },
        Cmd::Graph { action } => run_graph(&root, &cfg, action)?,
        Cmd::Ctx {
            symbol,
            no_body,
            limit,
        } => run_ctx(&root, &cfg, &symbol, !no_body, limit, cli.format)?,
        Cmd::Summarize {
            file,
            include_bodies,
        } => run_summarize(&file, include_bodies, cli.format)?,
        Cmd::Ls { pattern, kind } => {
            run_ls(&root, &cfg, &pattern, kind.as_deref(), cli.format)?
        }
        Cmd::Deps { action } => run_deps(&root, action)?,
        Cmd::Visibility { apply } => run_visibility(&root, &cfg, apply, cli.format)?,
    }
    Ok(())
}

fn run_deps(root: &Path, action: DepsAction) -> Result<()> {
    match action {
        DepsAction::Duplicates { format } => {
            let recs = cargo_tools::duplicates(root)?;
            cargo_tools::print_duplicates(&recs, format.into());
            if !recs.is_empty() {
                std::process::exit(1);
            }
        }
        DepsAction::Audit { format } => match cargo_tools::audit(root) {
            Ok(recs) => {
                cargo_tools::print_audit(&recs, format.into());
                if !recs.is_empty() {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("wraith: {}", e);
                std::process::exit(64);
            }
        },
        DepsAction::UnusedFeatures { format } => {
            let recs = cargo_tools::unused_features(root)?;
            cargo_tools::print_unused_features(&recs, format.into());
            if !recs.is_empty() {
                std::process::exit(1);
            }
        }
        DepsAction::Size { format, release } => match cargo_tools::size(root, release) {
            Ok(recs) => {
                cargo_tools::print_size(&recs, format.into());
            }
            Err(e) => {
                eprintln!("wraith: {}", e);
                std::process::exit(64);
            }
        },
    }
    Ok(())
}

fn run_ctx(
    root: &Path,
    cfg: &Config,
    symbol: &str,
    include_body: bool,
    limit: usize,
    format: Format,
) -> Result<()> {
    let (_ws, graph) = analyze_root(root, cfg)?;
    let limit = if limit == 0 { 5 } else { limit };
    match queries::ctx(&graph, symbol, include_body, limit) {
        Some(c) => {
            match format {
                Format::Json | Format::Jsonl => {
                    println!("{}", serde_json::to_string_pretty(&c)?);
                }
                Format::Human => print!("{}", c.render_markdown()),
            }
            Ok(())
        }
        None => {
            eprintln!("wraith: no symbol matching `{}`", symbol);
            std::process::exit(1);
        }
    }
}

fn run_summarize(file: &Path, include_bodies: bool, format: Format) -> Result<()> {
    match queries::summarize(file, include_bodies) {
        Some(s) => {
            match format {
                Format::Json | Format::Jsonl => {
                    println!("{}", serde_json::to_string_pretty(&s)?);
                }
                Format::Human => print!("{}", s.render_markdown()),
            }
            Ok(())
        }
        None => {
            eprintln!("wraith: could not parse `{}`", file.display());
            std::process::exit(1);
        }
    }
}

fn run_ls(
    root: &Path,
    cfg: &Config,
    pattern: &str,
    kind: Option<&str>,
    format: Format,
) -> Result<()> {
    let (_ws, graph) = analyze_root(root, cfg)?;
    let kind_filter = match kind {
        Some(k) => match queries::parse_kind(k) {
            Some(sk) => Some(sk),
            None => {
                eprintln!(
                    "wraith: unknown --kind `{}` (expected fn|struct|enum|trait|type|const|static|mod)",
                    k
                );
                std::process::exit(2);
            }
        },
        None => None,
    };
    let res = queries::ls(&graph, pattern, kind_filter);
    match format {
        Format::Json | Format::Jsonl => {
            println!("{}", serde_json::to_string_pretty(&res)?);
        }
        Format::Human => print!("{}", res.render_markdown()),
    }
    Ok(())
}

fn run_graph(root: &Path, cfg: &Config, action: GraphAction) -> Result<()> {
    use wraith_core::workspace::DepKind;

    match action {
        GraphAction::CrateDeps { format } => {
            let ws = Workspace::load(root)?;
            let names: std::collections::HashSet<String> =
                ws.crates.iter().map(|c| c.name.clone()).collect();
            #[derive(serde::Serialize)]
            struct Edge {
                from: String,
                to: String,
                kind: &'static str,
            }
            let mut edges: Vec<Edge> = Vec::new();
            for c in &ws.crates {
                for dep in &c.deps {
                    if !names.contains(&dep.name) {
                        continue;
                    }
                    let kind = match dep.kind {
                        DepKind::Normal => "normal",
                        DepKind::Dev => "dev",
                        DepKind::Build => "build",
                    };
                    edges.push(Edge {
                        from: c.name.clone(),
                        to: dep.name.clone(),
                        kind,
                    });
                }
            }
            let nodes: Vec<String> = ws.crates.iter().map(|c| c.name.clone()).collect();
            match format {
                GraphFormat::Json => {
                    let payload = serde_json::json!({
                        "query": "crate-deps",
                        "nodes": nodes,
                        "edges": edges,
                        "total": edges.len(),
                    });
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
                GraphFormat::Md => {
                    println!("# crate-deps\n");
                    println!("Crates: {}\n", nodes.len());
                    for n in &nodes {
                        println!("- {}", n);
                    }
                    println!("\n## edges ({})\n", edges.len());
                    for e in &edges {
                        println!("- `{}` → `{}` ({})", e.from, e.to, e.kind);
                    }
                }
                GraphFormat::Dot => {
                    println!("digraph crate_deps {{");
                    for n in &nodes {
                        println!("  \"{}\";", n);
                    }
                    for e in &edges {
                        println!("  \"{}\" -> \"{}\" [label=\"{}\"];", e.from, e.to, e.kind);
                    }
                    println!("}}");
                }
            }
        }
        GraphAction::Callers { symbol, transitive, format } => {
            let (_ws, graph) = analyze_root(root, cfg)?;
            let Some(idx) = graph.resolve_symbol(&symbol) else {
                emit_unresolved("callers", &symbol, &graph, format);
                std::process::exit(1);
            };
            let results: Vec<(usize, usize)> = if transitive {
                graph.blast_radius(idx, None)
            } else {
                graph.direct_callers(idx).into_iter().map(|i| (i, 1)).collect()
            };
            emit_symbol_query("callers", &symbol, &graph, &results, format);
        }
        GraphAction::Callees { symbol, transitive, format } => {
            let (_ws, graph) = analyze_root(root, cfg)?;
            let Some(idx) = graph.resolve_symbol(&symbol) else {
                emit_unresolved("callees", &symbol, &graph, format);
                std::process::exit(1);
            };
            let results: Vec<(usize, usize)> = if transitive {
                graph.transitive_callees(idx, None)
            } else {
                graph.direct_callees(idx).into_iter().map(|i| (i, 1)).collect()
            };
            emit_symbol_query("callees", &symbol, &graph, &results, format);
        }
        GraphAction::BlastRadius { symbol, depth, format } => {
            let (_ws, graph) = analyze_root(root, cfg)?;
            let Some(idx) = graph.resolve_symbol(&symbol) else {
                emit_unresolved("blast-radius", &symbol, &graph, format);
                std::process::exit(1);
            };
            let results = graph.blast_radius(idx, depth);
            emit_symbol_query("blast-radius", &symbol, &graph, &results, format);
        }
        GraphAction::ReverseDeps { target, format } => {
            let (ws, graph) = analyze_root(root, cfg)?;
            let crate_names: std::collections::HashSet<String> =
                ws.crates.iter().map(|c| c.name.clone()).collect();
            let crate_keys: std::collections::HashSet<String> =
                ws.crates.iter().map(|c| c.name.replace('-', "_")).collect();
            let is_crate = crate_names.contains(&target) || crate_keys.contains(&target);
            let crate_key = target.replace('-', "_");
            let dependents = if is_crate {
                graph.reverse_crate_deps(&crate_key)
            } else {
                graph.reverse_module_deps(&target)
            };
            #[derive(serde::Serialize)]
            struct Result {
                crate_name: String,
            }
            let results: Vec<Result> = dependents
                .iter()
                .map(|c| Result { crate_name: c.clone() })
                .collect();
            match format {
                GraphFormat::Json => {
                    let payload = serde_json::json!({
                        "query": "reverse-deps",
                        "target": target,
                        "target_kind": if is_crate { "crate" } else { "module" },
                        "results": results,
                        "total": results.len(),
                    });
                    println!("{}", serde_json::to_string_pretty(&payload)?);
                }
                GraphFormat::Md => {
                    println!("# reverse-deps — `{}`\n", target);
                    if results.is_empty() {
                        println!("_no dependents_");
                    } else {
                        for r in &results {
                            println!("- `{}`", r.crate_name);
                        }
                    }
                }
                GraphFormat::Dot => {
                    println!("digraph reverse_deps {{");
                    println!("  \"{}\";", target);
                    for r in &results {
                        println!("  \"{}\" -> \"{}\";", r.crate_name, target);
                    }
                    println!("}}");
                }
            }
        }
    }
    Ok(())
}

fn run_visibility(root: &Path, cfg: &Config, apply: bool, format: Format) -> Result<()> {
    use wraith_core::visibility::{
        apply_suggestions, compute_suggestions, render_markdown, scan_pub_use_reexports,
    };

    let (ws, graph) = analyze_root(root, cfg)?;

    // Treat names re-exported via `pub use` from any lib.rs / main.rs /
    // mod.rs in the workspace as intentional public API surface; skip
    // them in the suggestion list.
    let mut reexports: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut skip_files: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for c in &ws.crates {
        for f in ws.crate_rs_files(c) {
            let name = f.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let is_root = matches!(name, "lib.rs" | "main.rs" | "mod.rs");
            let Ok(text) = std::fs::read_to_string(&f) else {
                continue;
            };
            if is_root {
                for n in scan_pub_use_reexports(&text) {
                    reexports.insert(n);
                }
            }
            if has_refusal_marker(&text) {
                skip_files.insert(f);
            }
        }
    }

    let report = compute_suggestions(&graph, &reexports, &skip_files);

    if apply {
        let n = apply_suggestions(&report.suggestions)?;
        println!("applied {} edit(s)", n);
        return Ok(());
    }

    match format {
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Format::Jsonl => {
            for s in &report.suggestions {
                println!("{}", serde_json::to_string(s)?);
            }
        }
        Format::Human => {
            print!("{}", render_markdown(&report));
        }
    }
    Ok(())
}

fn emit_unresolved(query: &str, target: &str, graph: &wraith_core::graph::ReferenceGraph, format: GraphFormat) {
    let candidates: Vec<String> = graph
        .resolve_symbol_all(target)
        .into_iter()
        .map(|i| graph.symbols[i].symbol.qualified())
        .collect();
    let payload = serde_json::json!({
        "query": query,
        "target": target,
        "error": "unresolved or ambiguous symbol",
        "candidates": candidates,
        "results": [],
        "total": 0,
    });
    match format {
        GraphFormat::Json | GraphFormat::Dot => {
            eprintln!("{}", serde_json::to_string_pretty(&payload).unwrap());
        }
        GraphFormat::Md => {
            eprintln!("# {} — `{}`\n", query, target);
            eprintln!("**unresolved symbol**\n");
            if !candidates.is_empty() {
                eprintln!("Candidates:\n");
                for c in &candidates {
                    eprintln!("- `{}`", c);
                }
            }
        }
    }
}

fn emit_symbol_query(
    query: &str,
    target: &str,
    graph: &wraith_core::graph::ReferenceGraph,
    results: &[(usize, usize)],
    format: GraphFormat,
) {
    #[derive(serde::Serialize)]
    struct Row {
        symbol: String,
        file: String,
        line: usize,
        distance: usize,
    }
    let rows: Vec<Row> = results
        .iter()
        .map(|&(idx, distance)| {
            let s = &graph.symbols[idx];
            Row {
                symbol: s.symbol.qualified(),
                file: s.file.display().to_string(),
                line: s.line,
                distance,
            }
        })
        .collect();
    match format {
        GraphFormat::Json => {
            let payload = serde_json::json!({
                "query": query,
                "target": target,
                "results": rows,
                "total": rows.len(),
            });
            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        }
        GraphFormat::Md => {
            println!("# {} — `{}`\n", query, target);
            if rows.is_empty() {
                println!("_no results_");
            } else {
                println!("| distance | symbol | file:line |");
                println!("|---|---|---|");
                for r in &rows {
                    println!("| {} | `{}` | `{}:{}` |", r.distance, r.symbol, r.file, r.line);
                }
                println!("\n{} result(s).", rows.len());
            }
        }
        GraphFormat::Dot => {
            println!("digraph {} {{", query.replace('-', "_"));
            println!("  \"{}\";", target);
            for r in &rows {
                println!("  \"{}\" -> \"{}\";", r.symbol, target);
            }
            println!("}}");
        }
    }
}

/// Coarse, file-level marker scan: any `#[doc(hidden)]` /
/// `#[deprecated]` / `#[allow(dead_code)]` anywhere in the file makes
/// us skip every pub item in it. Conservative but cheap.
fn has_refusal_marker(text: &str) -> bool {
    text.contains("#[doc(hidden)]")
        || text.contains("#[deprecated")
        || text.contains("#[allow(dead_code)]")
}

struct Snapshot {
    findings: Vec<Finding>,
    crates_count: usize,
    files_count: usize,
    total_loc: usize,
}

fn collect_snapshot(root: &Path, cfg: &Config) -> Result<Snapshot> {
    let (ws, graph) = analyze_root(root, cfg)?;
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();

    let mut findings: Vec<Finding> = Vec::new();
    findings.extend(find_dead_code(&graph, cfg));
    findings.extend(find_unused_deps(&ws, &graph, cfg));
    findings.extend(find_crate_cycles(&ws));
    findings.extend(find_module_cycles(&ws, &graph));
    findings.extend(find_duplicate_clusters(&crate_files, &cfg.duplicates));
    findings.extend(find_complexity_hotspots(&crate_files, &cfg.complexity));
    findings.extend(find_boundary_violations(&ws, &graph, &cfg.boundaries));
    findings.extend(fallow::run(root, "audit").unwrap_or_default());

    let mut files_count = 0usize;
    let mut total_loc = 0usize;
    for (_, files) in &crate_files {
        for f in files {
            files_count += 1;
            if let Ok(text) = std::fs::read_to_string(f) {
                total_loc += text.lines().count();
            }
        }
    }

    Ok(Snapshot {
        findings,
        crates_count: ws.crates.len(),
        files_count,
        total_loc,
    })
}

fn run_report(root: &Path, cfg: &Config) -> Result<()> {
    let start = std::time::Instant::now();
    let snap = collect_snapshot(root, cfg)?;
    let md = report_md::render(&report_md::Inputs {
        workspace_root: root,
        findings: &snap.findings,
        crates_count: snap.crates_count,
        files_count: snap.files_count,
        total_loc: snap.total_loc,
        elapsed_ms: start.elapsed().as_millis(),
    });
    print!("{}", md);
    Ok(())
}

/// Diff-mode report: snapshot at <base_ref> + snapshot at HEAD, render
/// before/after narrative. Uses a temporary git worktree at the baseline
/// ref so the live working tree isn't disturbed.
fn run_report_diff(root: &Path, cfg: &Config, base_ref: &str) -> Result<()> {
    use std::process::Command;
    let start = std::time::Instant::now();

    // Resolve refs to SHAs for the report header.
    let resolve_sha = |r: &str| -> Result<String> {
        let out = Command::new("git")
            .args(["rev-parse", "--short=12", r])
            .current_dir(root)
            .output()
            .map_err(|e| anyhow::anyhow!("git rev-parse: {e}"))?;
        if !out.status.success() {
            anyhow::bail!(
                "git rev-parse {r}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let base_sha = resolve_sha(base_ref)?;
    let head_sha = resolve_sha("HEAD")?;

    eprintln!(
        "wraith report --since={} (resolved {}..{})",
        base_ref, base_sha, head_sha
    );

    // Snapshot HEAD first (the live working tree's analysis).
    eprintln!("  capturing HEAD snapshot…");
    let head = collect_snapshot(root, cfg)?;

    // Create a temp worktree at the base ref for the baseline snapshot.
    // We use a stable per-ref directory under .claude/wraith-diff/ so
    // re-runs against the same ref hit the warm cache.
    let monorepo_root = find_monorepo_root(root).unwrap_or_else(|| root.to_path_buf());
    let worktree_dir = monorepo_root
        .join(".claude")
        .join("wraith-diff")
        .join(&base_sha);
    let _ = std::fs::create_dir_all(worktree_dir.parent().unwrap());

    eprintln!("  creating worktree at {} …", worktree_dir.display());
    // Remove stale worktree if present (git won't reuse).
    if worktree_dir.exists() {
        let _ = Command::new("git")
            .args([
                "worktree",
                "remove",
                worktree_dir.to_string_lossy().as_ref(),
                "--force",
            ])
            .current_dir(&monorepo_root)
            .output();
    }
    let wt_out = Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            worktree_dir.to_string_lossy().as_ref(),
            &base_sha,
        ])
        .current_dir(&monorepo_root)
        .output()
        .map_err(|e| anyhow::anyhow!("git worktree add: {e}"))?;
    if !wt_out.status.success() {
        anyhow::bail!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&wt_out.stderr).trim()
        );
    }

    // Translate root path (inside the live tree) to the equivalent path
    // inside the worktree. We compute the path relative to monorepo_root
    // and apply that under worktree_dir.
    let rel_root = root.strip_prefix(&monorepo_root).unwrap_or(Path::new(""));
    let base_root = worktree_dir.join(rel_root);

    eprintln!("  capturing baseline snapshot at {} …", base_root.display());
    let base_cfg = Config::load(&base_root).unwrap_or_else(|_| cfg.clone());
    let base = collect_snapshot(&base_root, &base_cfg)?;

    // LOC delta via git diff --shortstat for the root path.
    let diff_out = Command::new("git")
        .args([
            "diff",
            "--shortstat",
            &format!("{}..{}", base_sha, head_sha),
            "--",
            rel_root.to_string_lossy().as_ref(),
        ])
        .current_dir(&monorepo_root)
        .output()
        .map_err(|e| anyhow::anyhow!("git diff: {e}"))?;
    let diff_summary = String::from_utf8_lossy(&diff_out.stdout)
        .trim()
        .to_string();
    let (loc_added, loc_removed, files_changed) = parse_shortstat(&diff_summary);

    // Cleanup the worktree (cache stays warm via .wraithrc.cache, but the
    // checkout itself is disposable).
    let _ = Command::new("git")
        .args([
            "worktree",
            "remove",
            worktree_dir.to_string_lossy().as_ref(),
            "--force",
        ])
        .current_dir(&monorepo_root)
        .output();

    let md = report_md::render_diff(&report_md::DiffInputs {
        workspace_root: root,
        base_ref: base_ref.to_string(),
        base_sha,
        head_sha,
        base_findings: &base.findings,
        head_findings: &head.findings,
        base_loc: base.total_loc,
        head_loc: head.total_loc,
        base_files: base.files_count,
        head_files: head.files_count,
        loc_added,
        loc_removed,
        files_changed,
        elapsed_ms: start.elapsed().as_millis(),
    });
    print!("{}", md);
    Ok(())
}

fn parse_shortstat(s: &str) -> (usize, usize, usize) {
    // Format: " N files changed, M insertions(+), K deletions(-)"
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut files = 0usize;
    for part in s.split(',').map(str::trim) {
        let num: usize = part
            .split_whitespace()
            .next()
            .and_then(|t| t.parse().ok())
            .unwrap_or(0);
        if part.contains("insertion") {
            added = num;
        } else if part.contains("deletion") {
            removed = num;
        } else if part.contains("file") {
            files = num;
        }
    }
    (added, removed, files)
}

fn find_monorepo_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start;
    loop {
        if cur.join(".git").exists() {
            return Some(cur.to_path_buf());
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return None,
        }
    }
}

fn run_extract_fn(selector: &str, new_name: &str, dry_run: bool) -> Result<()> {
    use wraith_core::refactor::{extract_fn, ExtractError, ExtractOptions};

    let (file_str, range) = selector
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("selector must be <file>:<start>..<end>, got {selector}"))?;
    let (s, e) = range
        .split_once("..")
        .ok_or_else(|| anyhow::anyhow!("range must be <start>..<end>, got {range}"))?;
    let line_start: usize = s
        .parse()
        .map_err(|_| anyhow::anyhow!("bad start line: {s}"))?;
    let line_end: usize = e
        .parse()
        .map_err(|_| anyhow::anyhow!("bad end line: {e}"))?;

    let file = PathBuf::from(file_str);
    let opts = ExtractOptions {
        file: file.clone(),
        line_start,
        line_end,
        new_fn_name: new_name.to_string(),
    };
    match extract_fn(&opts) {
        Ok(res) => {
            if dry_run {
                print!("{}", res.new_source);
            } else {
                std::fs::write(&file, &res.new_source)?;
                eprintln!(
                    "wraith: extracted {} ({} param(s), {} return(s)) into {}",
                    new_name,
                    res.call_args.len(),
                    res.returns.len(),
                    file.display()
                );
            }
            Ok(())
        }
        Err(e @ ExtractError::UnsupportedPattern { .. }) => {
            eprintln!("error: {}", e);
            std::process::exit(64);
        }
        Err(other) => Err(other.into()),
    }
}

fn run_dedupe_cluster(
    root: &Path,
    cfg: &Config,
    cluster_spec: &str,
    canonical: Option<&str>,
    dry_run: bool,
    no_elevate: bool,
) -> Result<()> {
    use wraith_core::dedupe::{
        apply_edits, dedupe_cluster_from_finding, DedupeError, DedupeOptions,
    };

    let (ws, graph) = analyze_root(root, cfg)?;
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();
    let clusters = find_duplicate_clusters(&crate_files, &cfg.duplicates);

    if clusters.is_empty() {
        eprintln!("wraith: no duplicate clusters found");
        std::process::exit(1);
    }

    let target = locate_cluster(&clusters, cluster_spec).ok_or_else(|| {
        anyhow::anyhow!(
            "no cluster matched `{}` (found {} cluster(s))",
            cluster_spec,
            clusters.len()
        )
    })?;

    let opts = DedupeOptions {
        canonical: canonical.map(|s| s.to_string()),
        no_elevate,
    };
    match dedupe_cluster_from_finding(target, &opts, &graph) {
        Ok(result) => {
            for n in &result.notices {
                println!("{}", n.message);
            }
            let removed_count = result.removed.len();
            if dry_run {
                println!(
                    "wraith: dry-run — would dedupe cluster, keeping `{}` and removing {} member(s):",
                    result.canonical.symbol, removed_count
                );
                for m in &result.removed {
                    println!("  - {} @ {}:{}", m.symbol, m.file, m.line);
                }
                println!("\nplanned edits ({}):", result.edits.len());
                for e in &result.edits {
                    println!("--- {} ---", e.path.display());
                    println!("{}", e.new_contents);
                }
            } else {
                let n = apply_edits(&result.edits)?;
                println!(
                    "wraith: deduped cluster — kept `{}`, removed {} duplicate fn(s), wrote {} file(s)",
                    result.canonical.symbol, removed_count, n
                );
            }
            Ok(())
        }
        Err(
            e @ (DedupeError::NotByteIdentical
            | DedupeError::ScopeDivergence { .. }
            | DedupeError::CanonicalInsufficientVisibility { .. }),
        ) => {
            eprintln!("error: {}", e);
            std::process::exit(64);
        }
        Err(other) => Err(other.into()),
    }
}

fn run_diff_cluster(
    root: &Path,
    cfg: &Config,
    cluster_spec: &str,
    format: DiffFormat,
) -> Result<()> {
    use wraith_core::diff_cluster::{diff_cluster_from_finding, render_markdown};

    let ws = Workspace::load(root)?;
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();
    let clusters = find_duplicate_clusters(&crate_files, &cfg.duplicates);
    if clusters.is_empty() {
        eprintln!("wraith: no duplicate clusters found");
        std::process::exit(1);
    }
    let target = locate_cluster(&clusters, cluster_spec).ok_or_else(|| {
        anyhow::anyhow!("no cluster matched `{}` (found {} cluster(s))", cluster_spec, clusters.len())
    })?;

    let report = diff_cluster_from_finding(cluster_spec, target)?;
    match format {
        DiffFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        DiffFormat::Md => {
            print!("{}", render_markdown(&report));
        }
    }
    Ok(())
}

fn run_extract_shared(
    root: &Path,
    cfg: &Config,
    cluster_spec: &str,
    signature: &str,
    param_mapping_arg: &str,
    extract_to: &str,
    dry_run: bool,
) -> Result<()> {
    use wraith_core::extract_shared::{
        extract_shared, ExtractSharedError, ExtractSharedOptions, ExtractSharedPlan,
    };

    let ws = Workspace::load(root)?;
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();
    let clusters = find_duplicate_clusters(&crate_files, &cfg.duplicates);
    if clusters.is_empty() {
        eprintln!("wraith: no duplicate clusters found");
        std::process::exit(1);
    }
    let target = locate_cluster(&clusters, cluster_spec).ok_or_else(|| {
        anyhow::anyhow!(
            "no cluster matched `{}` (found {} cluster(s))",
            cluster_spec,
            clusters.len()
        )
    })?;

    let param_mapping_text = if let Some(p) = param_mapping_arg.strip_prefix('@') {
        std::fs::read_to_string(p)?
    } else {
        param_mapping_arg.to_string()
    };
    let param_mapping: serde_json::Value = serde_json::from_str(&param_mapping_text)
        .map_err(|e| anyhow::anyhow!("invalid --param-mapping JSON: {e}"))?;

    let opts = ExtractSharedOptions {
        root: root.to_path_buf(),
        signature: signature.to_string(),
        param_mapping,
        extract_to: extract_to.to_string(),
        dry_run,
    };
    match extract_shared(target, &opts) {
        Ok(ExtractSharedPlan {
            shared_fn_text,
            edits,
            extract_to_path,
            verified,
        }) => {
            if dry_run {
                println!("wraith: dry-run extract-shared");
                println!("--- new fn (target: {}) ---", extract_to_path.display());
                println!("{}", shared_fn_text);
                for e in &edits {
                    println!("--- {} ---", e.path.display());
                    println!("{}", e.new_contents);
                }
            } else {
                println!(
                    "wraith: extracted shared fn into {} ({} file(s) rewritten, cargo-check={})",
                    extract_to_path.display(),
                    edits.len(),
                    if verified { "ok" } else { "skipped" }
                );
            }
            Ok(())
        }
        Err(ExtractSharedError::MissingMapping { missing }) => {
            eprintln!("error: param-mapping missing keys: [{}]", missing.join(", "));
            std::process::exit(64);
        }
        Err(ExtractSharedError::BuildFailed { stderr }) => {
            eprintln!("error: cargo check failed after extract-shared; rolled back.\n{}", stderr);
            std::process::exit(65);
        }
        Err(other) => Err(other.into()),
    }
}

fn run_move_fn(
    root: &Path,
    selector: &str,
    to: &str,
    cross_crate: bool,
    dry_run: bool,
) -> Result<()> {
    use wraith_core::move_fn::{self, MoveFnError, MoveFnOptions};
    use wraith_core::refactor_shared::parse_file_fn_selector;

    let (src_file, fn_name) = parse_file_fn_selector(selector)?;
    let src_file = if src_file.is_absolute() {
        src_file
    } else {
        root.join(&src_file)
    };
    let ws = Workspace::load(root)?;
    let opts = MoveFnOptions {
        src_file,
        fn_name,
        dst_module_path: to.to_string(),
        allow_cross_crate: cross_crate,
    };
    match move_fn::move_fn(&ws, &opts) {
        Ok(res) => {
            if dry_run {
                println!(
                    "wraith: dry-run move-fn — would move into {} ({} crate)",
                    res.dst_file.display(),
                    res.dst_crate
                );
                for e in &res.edits {
                    println!("--- {} ---", e.path.display());
                    println!("{}", e.new_contents);
                }
            } else {
                let n = move_fn::apply(&res.edits)?;
                for notice in &res.notices {
                    println!("{notice}");
                }
                println!(
                    "wraith: moved fn into {} ({} file(s) written, {} created)",
                    res.dst_file.display(),
                    n,
                    res.created_files.len()
                );
            }
            Ok(())
        }
        Err(e @ MoveFnError::CrossCrateDenied { .. })
        | Err(e @ MoveFnError::DestCollision(_, _))
        | Err(e @ MoveFnError::NotFound(_, _))
        | Err(e @ MoveFnError::DestCrateNotFound(_)) => {
            eprintln!("error: {}", e);
            std::process::exit(64);
        }
        Err(other) => Err(other.into()),
    }
}

fn run_split_fn(selector: &str, at_line: usize, names: &str, dry_run: bool) -> Result<()> {
    use wraith_core::refactor_shared::parse_file_fn_selector;
    use wraith_core::split_fn::{self, SplitFnError, SplitFnOptions};

    let (file, fn_name) = parse_file_fn_selector(selector)?;
    let pair: Vec<&str> = names.split(',').collect();
    if pair.len() != 2 {
        eprintln!("error: --names must be `<first>,<second>`");
        std::process::exit(64);
    }
    let opts = SplitFnOptions {
        file: file.clone(),
        fn_name,
        at_line,
        first_name: pair[0].trim().to_string(),
        second_name: pair[1].trim().to_string(),
    };
    match split_fn::split_fn(&opts) {
        Ok(res) => {
            if dry_run {
                println!(
                    "wraith: dry-run split-fn — would split at line {} into `{}` / `{}` ({} intermediates: [{}])",
                    at_line,
                    pair[0].trim(),
                    pair[1].trim(),
                    res.intermediates.len(),
                    res.intermediates.join(", ")
                );
                for e in &res.edits {
                    println!("--- {} ---", e.path.display());
                    println!("{}", e.new_contents);
                }
            } else {
                let n = split_fn::apply(&res.edits)?;
                println!(
                    "wraith: split fn at line {} ({} intermediates, {} file(s) written)",
                    at_line,
                    res.intermediates.len(),
                    n
                );
            }
            Ok(())
        }
        Err(
            e @ (SplitFnError::NotFound(_, _)
            | SplitFnError::NotAtStatementBoundary(_, _)
            | SplitFnError::EmptyHalf
            | SplitFnError::BadNames),
        ) => {
            eprintln!("error: {}", e);
            std::process::exit(64);
        }
        Err(other) => Err(other.into()),
    }
}

fn run_inline(root: &Path, selector: &str, dry_run: bool) -> Result<()> {
    use wraith_core::inline::{self, InlineError, InlineOptions};
    use wraith_core::refactor_shared::parse_file_fn_selector;

    let (file, fn_name) = parse_file_fn_selector(selector)?;
    let file = if file.is_absolute() { file } else { root.join(&file) };
    let ws = Workspace::load(root)?;
    let opts = InlineOptions { file, fn_name };
    match inline::inline(&ws, &opts) {
        Ok(res) => {
            if dry_run {
                println!(
                    "wraith: dry-run inline — would inline at {} call site(s)",
                    res.call_sites_replaced
                );
                for e in &res.edits {
                    println!("--- {} ---", e.path.display());
                    println!("{}", e.new_contents);
                }
            } else {
                let n = inline::apply(&res.edits)?;
                println!(
                    "wraith: inlined at {} call site(s); {} file(s) written",
                    res.call_sites_replaced, n
                );
            }
            Ok(())
        }
        Err(e @ (InlineError::NotFound(_, _) | InlineError::Unsupported(_))) => {
            eprintln!("error: {}", e);
            std::process::exit(64);
        }
        Err(other) => Err(other.into()),
    }
}

fn run_rename(root: &Path, symbol: &str, to: &str, dry_run: bool) -> Result<()> {
    use wraith_core::rename::{self, RenameError, RenameOptions};

    let ws = Workspace::load(root)?;
    let opts = RenameOptions {
        symbol: symbol.to_string(),
        new_name: to.to_string(),
    };
    match rename::rename(&ws, &opts) {
        Ok(res) => {
            if dry_run {
                println!(
                    "wraith: dry-run rename — would rename `{}` → `{}` ({} renames across {} file(s))",
                    res.leaf_name, to, res.renames_applied, res.files_touched
                );
                for e in &res.edits {
                    println!("--- {} ---", e.path.display());
                    println!("{}", e.new_contents);
                }
            } else {
                let n = rename::apply(&res.edits)?;
                println!(
                    "wraith: renamed `{}` → `{}` ({} renames, {} file(s) written)",
                    res.leaf_name, to, res.renames_applied, n
                );
            }
            Ok(())
        }
        Err(
            e @ (RenameError::NotFound(_)
            | RenameError::Collision(_)
            | RenameError::TraitMethod),
        ) => {
            eprintln!("error: {}", e);
            std::process::exit(64);
        }
        Err(other) => Err(other.into()),
    }
}

fn locate_cluster<'a>(clusters: &'a [Finding], spec: &str) -> Option<&'a Finding> {
    use wraith_core::report::FindingKind;
    // Try as integer index first.
    if let Ok(idx) = spec.parse::<usize>() {
        return clusters.get(idx);
    }
    // Otherwise match by member symbol.
    for c in clusters {
        if let FindingKind::DuplicateCluster { members, .. } = &c.kind {
            if members.iter().any(|m| m.symbol == spec) {
                return Some(c);
            }
        }
    }
    None
}

/// Append `entry` to `<root>/.gitignore` (creating it if missing).
/// No-op when the entry already appears as a standalone line.
fn ensure_gitignore_entry(root: &Path, entry: &str) -> Result<()> {
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let has = existing
        .lines()
        .any(|l| l.trim() == entry);
    if has {
        return Ok(());
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(entry);
    next.push('\n');
    std::fs::write(&path, next)?;
    Ok(())
}

fn emit(findings: &[Finding], format: Format) {
    match format {
        Format::Human => {
            if findings.is_empty() {
                println!("no findings.");
                return;
            }
            for f in findings {
                println!("{}", f.render_human());
            }
            println!("\n{} finding(s).", findings.len());
        }
        Format::Json => {
            let s = serde_json::to_string_pretty(findings).unwrap();
            println!("{}", s);
        }
        Format::Jsonl => {
            for f in findings {
                println!("{}", serde_json::to_string(f).unwrap());
            }
        }
    }
}

