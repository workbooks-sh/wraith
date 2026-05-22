# wraith-mcp

Model Context Protocol JSON-RPC server. Exposes [wraith](../..) analysis as tools an MCP-enabled agent (Claude Code, Cursor, etc.) can call.

## Build + run

```bash
cargo build --bin wraith-mcp --release
./target/release/wraith-mcp --stdio
```

Wire format is newline-delimited JSON over stdio (one JSON-RPC envelope per line, no `Content-Length` framing).

## Tools

| Tool                    | Arguments                                              |
|-------------------------|--------------------------------------------------------|
| `wraith_dead_code`      | `crate_path: string`                                   |
| `wraith_unused_deps`    | `crate_path: string`                                   |
| `wraith_circular_deps`  | `crate_path: string`                                   |
| `wraith_health`         | `crate_path: string`, `threshold_cyclo?: u32`, `threshold_cog?: u32` |
| `wraith_dupes`          | `crate_path: string`, `threshold?: f32`                |
| `wraith_audit`          | `crate_path: string` (omnibus)                         |

Each call returns the analysis findings as both a textual content block (pretty-printed JSON) and a `structuredContent` field carrying the raw `Vec<Finding>`. Finding schema matches `wraith_core::report::Finding`.

## Resources

| URI                                 | Body                                                      |
|-------------------------------------|-----------------------------------------------------------|
| `wraith://workspace-summary`        | Last analyzed workspace path + dead-code finding count.   |
| `wraith://findings/dead_code`       | Most recent dead-code findings (JSON array).              |

## Claude Code registration

Add to `~/.config/claude-code/mcp.json` (or per-project config):

```json
{
  "mcpServers": {
    "wraith": { "command": "wraith-mcp", "args": ["--stdio"] }
  }
}
```

Restart Claude Code; the six wraith tools become available to the agent.

## Smoke test

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | wraith-mcp --stdio
```

## Status

Ships `wb-5lgj.13`. See `bd show wb-5lgj.13` for full context.
