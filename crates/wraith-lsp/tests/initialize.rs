//! Spawns `wraith-lsp --stdio`, sends an `initialize` request with proper
//! LSP framing, and asserts the InitializeResult shape.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn lsp_frame(payload: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", payload.len()).as_bytes());
    out.extend_from_slice(payload.as_bytes());
    out
}

fn read_message(stdout: &mut impl Read) -> String {
    // Read header
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stdout.read(&mut byte).unwrap_or(0) == 0 {
            break;
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = String::from_utf8_lossy(&buf).to_string();
    let len: usize = header
        .lines()
        .find_map(|l| l.strip_prefix("Content-Length:"))
        .map(|s| s.trim().parse().unwrap())
        .expect("Content-Length header");
    let mut body = vec![0u8; len];
    stdout.read_exact(&mut body).unwrap();
    String::from_utf8(body).unwrap()
}

#[test]
fn initialize_returns_capabilities() {
    let bin = env!("CARGO_BIN_EXE_wraith-lsp");
    let mut child = Command::new(bin)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn wraith-lsp");

    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(&lsp_frame(init)).unwrap();
        stdin.flush().unwrap();
    }

    let mut stdout = child.stdout.take().unwrap();
    // Read with a timeout via a thread.
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let msg = read_message(&mut stdout);
        let _ = tx.send(msg);
    });
    let msg = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("response within timeout");
    let v: serde_json::Value = serde_json::from_str(&msg).expect("valid JSON");

    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 1);
    let caps = &v["result"]["capabilities"];
    assert!(caps.is_object(), "result.capabilities is an object");
    assert!(
        !caps["textDocumentSync"].is_null(),
        "textDocumentSync capability set"
    );
    assert!(
        !caps["diagnosticProvider"].is_null(),
        "diagnosticProvider capability set"
    );
    assert!(
        !caps["codeActionProvider"].is_null(),
        "codeActionProvider capability set"
    );
    assert_eq!(v["result"]["serverInfo"]["name"], "wraith-lsp");

    let _ = handle.join();
    let _ = child.kill();
    let _ = child.wait();
}
