//! wraith-lsp — Language Server Protocol surface for wraith.
//!
//! On didOpen / didChange (debounced 500ms) we re-run wraith analysis
//! for the workspace containing the edited file and publish findings
//! that touch that file as diagnostics. Code actions offer "remove
//! dead item" (delegated to wraith-core fix planner) and a stubbed
//! "extract function" for complexity hotspots.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use wraith_core::analyze::{analyze_root, find_dead_code, find_unused_deps};
use wraith_core::boundaries::find_boundary_violations;
use wraith_core::circular::{find_crate_cycles, find_module_cycles};
use wraith_core::config::Config;
use wraith_core::dupes::find_duplicates;
use wraith_core::fix;
use wraith_core::health::find_complexity_hotspots;
use wraith_core::report::{Finding, FindingKind, Severity};
use wraith_core::workspace::Workspace;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Accept --stdio for parity with rust-analyzer / other LSPs.
    let _stdio = args.iter().any(|a| a == "--stdio");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        state: Arc::new(Mutex::new(BackendState::default())),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[derive(Default)]
struct BackendState {
    /// Workspace root resolved on initialize (or first didOpen).
    root: Option<PathBuf>,
    /// Latest published findings per file path (used by codeAction).
    findings_by_file: HashMap<PathBuf, Vec<Finding>>,
    /// Debounce: bump on every change, only the latest tick re-runs.
    tick: u64,
}

struct Backend {
    client: Client,
    state: Arc<Mutex<BackendState>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        if let Some(folders) = params.workspace_folders {
            if let Some(first) = folders.into_iter().next() {
                if let Ok(p) = first.uri.to_file_path() {
                    self.state.lock().await.root = Some(p);
                }
            }
        }
        #[allow(deprecated)]
        if self.state.lock().await.root.is_none() {
            if let Some(uri) = params.root_uri {
                if let Ok(p) = uri.to_file_path() {
                    self.state.lock().await.root = Some(p);
                }
            }
        }

        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(
                TextDocumentSyncKind::FULL,
            )),
            diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                DiagnosticOptions {
                    identifier: Some("wraith".into()),
                    inter_file_dependencies: true,
                    workspace_diagnostics: false,
                    work_done_progress_options: Default::default(),
                },
            )),
            code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
            ..Default::default()
        };

        Ok(InitializeResult {
            capabilities,
            server_info: Some(ServerInfo {
                name: "wraith-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "wraith-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.schedule_analysis(params.text_document.uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.schedule_analysis(params.text_document.uri).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.schedule_analysis(params.text_document.uri).await;
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.clone();
        let Ok(path) = uri.to_file_path() else {
            return Ok(None);
        };
        let state = self.state.lock().await;
        let Some(findings) = state.findings_by_file.get(&path) else {
            return Ok(None);
        };

        let mut actions: Vec<CodeActionOrCommand> = Vec::new();
        for f in findings {
            if !line_in_range(f.line, &params.range) {
                continue;
            }
            match &f.kind {
                FindingKind::DeadCode { symbol, .. } => {
                    let plan = fix::plan(std::slice::from_ref(f));
                    let title = format!("wraith: remove dead item `{}`", symbol);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title,
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: None,
                        edit: None,
                        command: Some(Command {
                            title: "wraith.applyFix".into(),
                            command: "wraith.applyFix".into(),
                            arguments: Some(vec![
                                serde_json::to_value(&plan).unwrap_or(Value::Null),
                            ]),
                        }),
                        is_preferred: Some(true),
                        disabled: None,
                        data: None,
                    }));
                }
                FindingKind::Complexity { symbol, .. } => {
                    // wb-5lgj.23 (extract-function refactor) ships separately.
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!(
                            "wraith: extract function from `{}` (not yet implemented)",
                            symbol
                        ),
                        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
                        diagnostics: None,
                        edit: None,
                        command: Some(Command {
                            title: "wraith.notImplemented".into(),
                            command: "wraith.notImplemented".into(),
                            arguments: Some(vec![Value::String("extract-function".into())]),
                        }),
                        is_preferred: Some(false),
                        disabled: Some(CodeActionDisabled {
                            reason: "extract-function (wb-5lgj.23) not yet shipped".into(),
                        }),
                        data: None,
                    }));
                }
                _ => {}
            }
        }
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}

impl Backend {
    async fn schedule_analysis(&self, uri: Url) {
        let Ok(path) = uri.to_file_path() else { return };
        let tick = {
            let mut s = self.state.lock().await;
            s.tick = s.tick.wrapping_add(1);
            if s.root.is_none() {
                s.root = workspace_root_for(&path);
            }
            s.tick
        };
        let state = self.state.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            {
                let s = state.lock().await;
                if s.tick != tick {
                    return; // a newer edit superseded us
                }
            }
            let root = {
                let s = state.lock().await;
                s.root.clone()
            };
            let Some(root) = root else { return };
            let findings = match tokio::task::spawn_blocking(move || run_full_analysis(&root))
                .await
            {
                Ok(Ok(f)) => f,
                Ok(Err(e)) => {
                    client
                        .log_message(
                            MessageType::ERROR,
                            format!("wraith analysis failed: {e:#}"),
                        )
                        .await;
                    return;
                }
                Err(e) => {
                    client
                        .log_message(MessageType::ERROR, format!("wraith join error: {e}"))
                        .await;
                    return;
                }
            };
            let mut grouped: HashMap<PathBuf, Vec<Finding>> = HashMap::new();
            for f in findings {
                grouped.entry(f.file.clone()).or_default().push(f);
            }
            let previously: Vec<PathBuf> = {
                let s = state.lock().await;
                s.findings_by_file.keys().cloned().collect()
            };
            {
                let mut s = state.lock().await;
                s.findings_by_file = grouped.clone();
            }
            for (file, fs) in &grouped {
                let diagnostics = fs.iter().map(finding_to_diagnostic).collect::<Vec<_>>();
                if let Ok(url) = Url::from_file_path(file) {
                    client.publish_diagnostics(url, diagnostics, None).await;
                }
            }
            for old in previously {
                if !grouped.contains_key(&old) {
                    if let Ok(url) = Url::from_file_path(&old) {
                        client.publish_diagnostics(url, vec![], None).await;
                    }
                }
            }
        });
    }
}

fn line_in_range(line: usize, range: &Range) -> bool {
    let line0 = line.saturating_sub(1) as u32;
    line0 >= range.start.line && line0 <= range.end.line
}

fn workspace_root_for(file: &Path) -> Option<PathBuf> {
    let mut cur = file.parent();
    let mut best: Option<PathBuf> = None;
    while let Some(d) = cur {
        if d.join("Cargo.toml").exists() {
            best = Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    best
}

fn run_full_analysis(root: &Path) -> anyhow::Result<Vec<Finding>> {
    let cfg = Config::load(root)?;
    let (ws, graph) = analyze_root(root, &cfg)?;
    let mut findings = find_dead_code(&graph, &cfg);
    findings.extend(find_unused_deps(&ws, &graph, &cfg));
    findings.extend(find_crate_cycles(&ws));
    findings.extend(find_module_cycles(&ws, &graph));
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();
    findings.extend(find_complexity_hotspots(&crate_files, &cfg.complexity));
    findings.extend(find_duplicates(&crate_files, &cfg.duplicates));
    findings.extend(find_boundary_violations(&ws, &graph, &cfg.boundaries));
    let _ = Workspace::load(root)?;
    Ok(findings)
}

fn finding_to_diagnostic(f: &Finding) -> Diagnostic {
    let severity = match &f.kind {
        FindingKind::DeadCode { .. } | FindingKind::UnusedDep { .. } => {
            DiagnosticSeverity::WARNING
        }
        FindingKind::Complexity { .. } => DiagnosticSeverity::INFORMATION,
        FindingKind::Duplicate { .. }
        | FindingKind::DuplicateCluster { .. }
        | FindingKind::CircularDep { .. } => DiagnosticSeverity::HINT,
        FindingKind::BoundaryViolation { .. } => DiagnosticSeverity::ERROR,
        FindingKind::External { .. } => match f.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Info => DiagnosticSeverity::INFORMATION,
        },
    };
    let line0 = f.line.saturating_sub(1) as u32;
    let col0 = f.col.saturating_sub(1) as u32;
    let range = Range {
        start: Position {
            line: line0,
            character: col0,
        },
        end: Position {
            line: line0,
            character: col0.saturating_add(1),
        },
    };
    let tags = if matches!(
        f.kind,
        FindingKind::DeadCode { .. } | FindingKind::UnusedDep { .. }
    ) {
        Some(vec![DiagnosticTag::UNNECESSARY])
    } else {
        None
    };
    Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(finding_code(&f.kind).into())),
        code_description: None,
        source: Some("wraith".into()),
        message: f.render_human(),
        related_information: None,
        tags,
        data: None,
    }
}

fn finding_code(k: &FindingKind) -> &'static str {
    match k {
        FindingKind::DeadCode { .. } => "wraith/dead-code",
        FindingKind::UnusedDep { .. } => "wraith/unused-dep",
        FindingKind::CircularDep { .. } => "wraith/circular-dep",
        FindingKind::Duplicate { .. } => "wraith/duplicate",
        FindingKind::DuplicateCluster { .. } => "wraith/duplicate-cluster",
        FindingKind::Complexity { .. } => "wraith/complexity",
        FindingKind::BoundaryViolation { .. } => "wraith/boundary-violation",
        FindingKind::External { .. } => "wraith/external",
    }
}
