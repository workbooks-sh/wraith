//! Spawns `wraith-mcp --stdio`, sends a `tools/list` JSON-RPC request,
//! and asserts the six wraith analysis tools are advertised.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn read_one_line(stdout: &mut impl BufRead) -> String {
    let mut s = String::new();
    stdout.read_line(&mut s).unwrap();
    s
}

fn rpc(bin: &str, body: &str) -> serde_json::Value {
    let mut child = Command::new(bin)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn wraith-mcp");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(body.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
        // Close stdin so the server's read loop ends after responding.
        drop(child.stdin.take());
    }

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let (tx, rx) = std::sync::mpsc::channel();
    let h = std::thread::spawn(move || {
        let line = read_one_line(&mut reader);
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("response within timeout");
    let _ = h.join();
    let _ = child.kill();
    let _ = child.wait();
    serde_json::from_str(line.trim()).expect("valid JSON response")
}

#[test]
fn tools_list_returns_six_tools() {
    let bin = env!("CARGO_BIN_EXE_wraith-mcp");
    let resp = rpc(
        bin,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
    );
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools is an array");
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    let expected = [
        "wraith_dead_code",
        "wraith_unused_deps",
        "wraith_circular_deps",
        "wraith_health",
        "wraith_dupes",
        "wraith_audit",
    ];
    for e in expected {
        assert!(names.contains(&e), "expected tool `{}` in {:?}", e, names);
    }
    assert_eq!(tools.len(), 6, "exactly six tools");
}

#[test]
fn initialize_returns_protocol_version() {
    let bin = env!("CARGO_BIN_EXE_wraith-mcp");
    let resp = rpc(
        bin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    );
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["protocolVersion"].is_string());
    assert_eq!(resp["result"]["serverInfo"]["name"], "wraith-mcp");
    assert!(resp["result"]["capabilities"]["tools"].is_object());
}
