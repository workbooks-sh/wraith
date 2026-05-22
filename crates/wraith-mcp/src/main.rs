//! wraith-mcp — Model Context Protocol JSON-RPC server.
//!
//! Hand-rolled MCP over stdio. Exposes six analysis tools backed by
//! wraith-core, plus two read-only resources. Protocol surface stays
//! intentionally small (initialize, tools/list, tools/call,
//! resources/list, resources/read).
//!
//! Wire format: one JSON object per line on stdin / stdout (LSP-style
//! Content-Length framing is *not* used here — MCP stdio transport
//! uses newline-delimited JSON).

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use wraith_core::analyze::{analyze_root, find_dead_code, find_unused_deps};
use wraith_core::audit::run_audit;
use wraith_core::boundaries::find_boundary_violations;
use wraith_core::circular::{find_crate_cycles, find_module_cycles};
use wraith_core::config::{ComplexityConfig, Config, DuplicateConfig};
use wraith_core::dupes::find_duplicates;
use wraith_core::health::find_complexity_hotspots;
use wraith_core::report::Finding;
use wraith_core::workspace::Workspace;

const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Default)]
struct Server {
    /// Cached last-audit findings, for the `wraith://findings/dead_code` resource.
    last_dead_code: Mutex<Vec<Finding>>,
    last_workspace: Mutex<Option<PathBuf>>,
}

fn main() {
    let _args: Vec<String> = std::env::args().collect(); // --stdio is the only mode
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let server = Server::default();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(req) => server.handle(req),
            Err(e) => Some(error_response(Value::Null, -32700, format!("parse error: {e}"))),
        };
        if let Some(resp) = response {
            let s = serde_json::to_string(&resp).unwrap();
            writeln!(out, "{s}").ok();
            out.flush().ok();
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

fn ok_response(id: Value, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_response(id: Value, code: i32, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError { code, message }),
    }
}

impl Server {
    fn handle(&self, req: JsonRpcRequest) -> Option<JsonRpcResponse> {
        // Notifications carry no id and expect no response.
        let is_notification = req.id.is_null();
        let id = req.id.clone();
        let result = match req.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false },
                },
                "serverInfo": {
                    "name": "wraith-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            })),
            "initialized" | "notifications/initialized" => return None,
            "ping" => Ok(json!({})),
            "tools/list" => Ok(tools_list()),
            "tools/call" => self.tools_call(&req.params),
            "resources/list" => Ok(resources_list()),
            "resources/read" => self.resources_read(&req.params),
            "shutdown" => Ok(json!(null)),
            other => Err(format!("unknown method: {other}")),
        };
        if is_notification {
            return None;
        }
        Some(match result {
            Ok(v) => ok_response(id, v),
            Err(e) => error_response(id, -32601, e),
        })
    }

    fn tools_call(&self, params: &Value) -> Result<Value, String> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing tool name".to_string())?;
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);
        let crate_path = args
            .get("crate_path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| "missing crate_path".to_string())?;
        let canonical = crate_path.canonicalize().unwrap_or(crate_path.clone());
        *self.last_workspace.lock().unwrap() = Some(canonical.clone());

        let findings = match name {
            "wraith_dead_code" => tool_dead_code(&canonical),
            "wraith_unused_deps" => tool_unused_deps(&canonical),
            "wraith_circular_deps" => tool_circular(&canonical),
            "wraith_health" => {
                let t_cyclo = args
                    .get("threshold_cyclo")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32);
                let t_cog = args
                    .get("threshold_cog")
                    .and_then(Value::as_u64)
                    .map(|v| v as u32);
                tool_health(&canonical, t_cyclo, t_cog)
            }
            "wraith_dupes" => {
                let threshold = args
                    .get("threshold")
                    .and_then(Value::as_f64)
                    .map(|v| v as f32);
                tool_dupes(&canonical, threshold)
            }
            "wraith_audit" => tool_audit(&canonical),
            other => return Err(format!("unknown tool: {other}")),
        }
        .map_err(|e| format!("{e:#}"))?;

        if name == "wraith_dead_code" || name == "wraith_audit" {
            let dead: Vec<Finding> = findings
                .iter()
                .filter(|f| matches!(f.kind, wraith_core::report::FindingKind::DeadCode { .. }))
                .cloned()
                .collect();
            *self.last_dead_code.lock().unwrap() = dead;
        }

        let findings_json = serde_json::to_value(&findings).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(&findings_json).unwrap();
        Ok(json!({
            "content": [
                { "type": "text", "text": text }
            ],
            "isError": false,
            "structuredContent": findings_json,
        }))
    }

    fn resources_read(&self, params: &Value) -> Result<Value, String> {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing uri".to_string())?;
        match uri {
            "wraith://workspace-summary" => {
                let ws_path = self.last_workspace.lock().unwrap().clone();
                let last_dead = self.last_dead_code.lock().unwrap().len();
                let summary = json!({
                    "workspace": ws_path,
                    "last_dead_code_count": last_dead,
                });
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&summary).unwrap(),
                    }],
                }))
            }
            "wraith://findings/dead_code" => {
                let dead = self.last_dead_code.lock().unwrap().clone();
                let body = serde_json::to_string_pretty(&dead).unwrap();
                Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": body,
                    }],
                }))
            }
            other => Err(format!("unknown resource: {other}")),
        }
    }
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "wraith_dead_code",
                "description": "Find pub items with no references in the given Rust crate/workspace.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "crate_path": { "type": "string", "description": "Absolute or relative path to a Rust crate or workspace root." }
                    },
                    "required": ["crate_path"]
                }
            },
            {
                "name": "wraith_unused_deps",
                "description": "Find dependencies declared in Cargo.toml that are never imported.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "crate_path": { "type": "string" } },
                    "required": ["crate_path"]
                }
            },
            {
                "name": "wraith_circular_deps",
                "description": "Detect circular dependencies between crates and inside modules.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "crate_path": { "type": "string" } },
                    "required": ["crate_path"]
                }
            },
            {
                "name": "wraith_health",
                "description": "Flag functions above cyclomatic / cognitive complexity thresholds.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "crate_path": { "type": "string" },
                        "threshold_cyclo": { "type": "integer", "minimum": 1 },
                        "threshold_cog": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["crate_path"]
                }
            },
            {
                "name": "wraith_dupes",
                "description": "Token-shingled clone detection at the function-body level.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "crate_path": { "type": "string" },
                        "threshold": { "type": "number", "minimum": 0, "maximum": 1, "description": "Jaccard similarity threshold (0..1)." }
                    },
                    "required": ["crate_path"]
                }
            },
            {
                "name": "wraith_audit",
                "description": "Omnibus: run dead-code, unused-deps, circular, complexity, dupes, and boundary checks on the workspace.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "crate_path": { "type": "string" } },
                    "required": ["crate_path"]
                }
            }
        ]
    })
}

fn resources_list() -> Value {
    json!({
        "resources": [
            {
                "uri": "wraith://workspace-summary",
                "name": "Workspace summary",
                "description": "Crate metadata for the most recently analyzed workspace plus last-audit finding counts.",
                "mimeType": "application/json"
            },
            {
                "uri": "wraith://findings/dead_code",
                "name": "Last dead-code findings",
                "description": "Most recent dead-code findings as a JSON array of wraith Finding records.",
                "mimeType": "application/json"
            }
        ]
    })
}

fn tool_dead_code(root: &Path) -> anyhow::Result<Vec<Finding>> {
    let cfg = Config::load(root)?;
    let (_ws, graph) = analyze_root(root, &cfg)?;
    Ok(find_dead_code(&graph, &cfg))
}

fn tool_unused_deps(root: &Path) -> anyhow::Result<Vec<Finding>> {
    let cfg = Config::load(root)?;
    let (ws, graph) = analyze_root(root, &cfg)?;
    Ok(find_unused_deps(&ws, &graph, &cfg))
}

fn tool_circular(root: &Path) -> anyhow::Result<Vec<Finding>> {
    let cfg = Config::load(root)?;
    let (ws, graph) = analyze_root(root, &cfg)?;
    let mut out = find_crate_cycles(&ws);
    out.extend(find_module_cycles(&ws, &graph));
    Ok(out)
}

fn tool_health(
    root: &Path,
    t_cyclo: Option<u32>,
    t_cog: Option<u32>,
) -> anyhow::Result<Vec<Finding>> {
    let mut cfg = Config::load(root)?;
    let base = ComplexityConfig::default();
    cfg.complexity = ComplexityConfig {
        cyclomatic: t_cyclo.unwrap_or(cfg.complexity.cyclomatic.max(base.cyclomatic)),
        cognitive: t_cog.unwrap_or(cfg.complexity.cognitive.max(base.cognitive)),
    };
    let ws = Workspace::load(root)?;
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();
    Ok(find_complexity_hotspots(&crate_files, &cfg.complexity))
}

fn tool_dupes(root: &Path, threshold: Option<f32>) -> anyhow::Result<Vec<Finding>> {
    let mut cfg = Config::load(root)?;
    if let Some(t) = threshold {
        cfg.duplicates = DuplicateConfig {
            min_tokens: cfg.duplicates.min_tokens,
            similarity_threshold: t,
        };
    }
    let ws = Workspace::load(root)?;
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();
    Ok(find_duplicates(&crate_files, &cfg.duplicates))
}

fn tool_audit(root: &Path) -> anyhow::Result<Vec<Finding>> {
    let cfg = Config::load(root)?;
    let ws = Workspace::load(root)?;
    let mut all = run_audit(&ws, &cfg)?;
    let (_ws2, graph) = analyze_root(root, &cfg)?;
    all.extend(find_crate_cycles(&ws));
    all.extend(find_module_cycles(&ws, &graph));
    let crate_files: Vec<(String, Vec<PathBuf>)> = ws
        .crates
        .iter()
        .map(|c| (c.name.replace('-', "_"), ws.crate_rs_files(c)))
        .collect();
    all.extend(find_complexity_hotspots(&crate_files, &cfg.complexity));
    all.extend(find_duplicates(&crate_files, &cfg.duplicates));
    all.extend(find_boundary_violations(&ws, &graph, &cfg.boundaries));
    Ok(all)
}
