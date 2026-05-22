//! V8 `ScriptCoverage` JSON parser and Istanbul-compatible normalizer.
//!
//! This is the open-source layer of fallow's runtime-coverage pipeline.
//! It performs the mechanical conversion from V8's byte-offset-based coverage
//! format (as emitted by `node --experimental-test-coverage`, `c8`, the
//! Inspector protocol, or any V8 isolate) into the line/column-based
//! [`IstanbulFileCoverage`] shape that fallow's CRAP scoring already
//! consumes.
//!
//! The closed-source three-state cross-reference, combined scoring, hot-path
//! heuristics and verdict generation live in `fallow-cov` (private) and
//! consume this crate's normalized output via the `fallow-cov-protocol`
//! envelope.

#![forbid(unsafe_code)]

use serde::{Deserialize, Deserializer, Serialize};

// -- V8 input types ---------------------------------------------------------

/// Top-level shape emitted by Node's `NODE_V8_COVERAGE` directory: one file
/// per worker / process containing a `result` array of [`ScriptCoverage`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V8CoverageDump {
    /// Per-script coverage entries.
    pub result: Vec<ScriptCoverage>,
    /// Optional source-map cache emitted by Node 13+.
    #[serde(default, rename = "source-map-cache")]
    pub source_map_cache: Option<serde_json::Value>,
}

/// V8's per-script coverage record. Field names mirror the V8 inspector
/// protocol verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptCoverage {
    /// V8 script identifier.
    #[serde(rename = "scriptId")]
    pub script_id: String,
    /// File URL — typically `file:///abs/path` for Node, `https://…` for
    /// browsers. Callers normalize to absolute paths before merging.
    pub url: String,
    /// One entry per function (including the implicit module-level function).
    pub functions: Vec<FunctionCoverage>,
}

/// V8 per-function coverage. Each function carries one or more
/// [`CoverageRange`]s — block-level for instrumented coverage, function-level
/// for `--coverage=best-effort`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCoverage {
    /// Source-as-written function name. Empty for the module-level wrapper
    /// and anonymous functions.
    #[serde(rename = "functionName")]
    pub function_name: String,
    /// Coverage ranges, byte-offsets relative to the script's source text.
    pub ranges: Vec<CoverageRange>,
    /// True when V8 emitted block-level data for this function (instrumented
    /// coverage). False when only the outer function range is reliable
    /// (best-effort / runtime coverage).
    #[serde(rename = "isBlockCoverage", default)]
    pub is_block_coverage: bool,
}

/// A single coverage range. `count == 0` means the byte range was never hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageRange {
    /// Inclusive byte offset into the script's source.
    #[serde(rename = "startOffset")]
    pub start_offset: u32,
    /// Exclusive byte offset into the script's source.
    #[serde(rename = "endOffset")]
    pub end_offset: u32,
    /// Number of times the range was executed.
    pub count: u64,
}

// -- Istanbul output types --------------------------------------------------

/// Subset of the Istanbul `FileCoverage` shape that fallow needs for CRAP
/// scoring. We do not emit statement / branch maps because fallow only needs
/// per-function call counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IstanbulFileCoverage {
    /// Absolute path of the source file.
    pub path: String,
    /// Per-function records keyed by stable index (`f0`, `f1`, …).
    #[serde(rename = "fnMap")]
    pub fn_map: std::collections::BTreeMap<String, IstanbulFunction>,
    /// Per-function hit counts, keyed identically to `fn_map`.
    pub f: std::collections::BTreeMap<String, u64>,
}

/// Istanbul function descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IstanbulFunction {
    /// Source-as-written function name (matches V8's `functionName`).
    pub name: String,
    /// Declaration position. Matches Istanbul's `decl`.
    pub decl: IstanbulRange,
    /// Full body position. Matches Istanbul's `loc`.
    pub loc: IstanbulRange,
    /// 1-indexed line of the function declaration's start.
    pub line: u32,
}

/// 1-indexed line/column range matching Istanbul's `Range`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IstanbulRange {
    /// Inclusive start position.
    pub start: IstanbulPosition,
    /// Exclusive end position.
    pub end: IstanbulPosition,
}

/// 1-indexed line + 0-indexed column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IstanbulPosition {
    /// 1-indexed line number.
    pub line: u32,
    /// 0-indexed column within the line.
    ///
    /// Some real Istanbul producers (including Vitest in certain transforms)
    /// emit `null` for end columns. We normalize those to `0` at parse time
    /// so downstream CRAP/prod-coverage consumers can still ingest the file.
    #[serde(deserialize_with = "deserialize_nullable_u32")]
    pub column: u32,
}

fn deserialize_nullable_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<u32>::deserialize(deserializer)?.unwrap_or(0))
}

// -- V8 offset to line/column mapper ---------------------------------------

/// Pre-computed line-start table for converting V8 source offsets into
/// Istanbul line/column positions in O(log n) per lookup.
///
/// V8 reports offsets in JavaScript source positions: UTF-16 code units, not
/// UTF-8 bytes. Istanbul columns use the same 0-indexed source-position model,
/// so this table stores line starts in UTF-16 units.
///
/// The source is consumed once at construction; subsequent lookups are
/// allocation-free.
#[derive(Debug)]
pub struct LineOffsetTable {
    /// UTF-16 offset of the first character of each line. `line_starts[0]`
    /// is always `0` (the start of the file).
    line_starts: Vec<u32>,
}

impl LineOffsetTable {
    /// Build a table from the full source text. The source must be UTF-8 with
    /// LF, CRLF, or CR line endings (mixed endings are tolerated).
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let mut line_starts = Vec::with_capacity(source.lines().count() + 1);
        line_starts.push(0);
        let mut offset = 0u32;
        let mut chars = source.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\n' => {
                    offset = offset.saturating_add(1);
                    line_starts.push(offset);
                }
                '\r' => {
                    offset = offset.saturating_add(1);
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                        offset = offset.saturating_add(1);
                    }
                    line_starts.push(offset);
                }
                _ => offset = offset.saturating_add(ch.len_utf16() as u32),
            }
        }
        Self { line_starts }
    }

    /// Build a table from V8's `source-map-cache.lineLengths` data.
    ///
    /// `lineLengths` are already measured in JavaScript source positions. The
    /// cache does not carry line-ending widths, so this preserves the existing
    /// Node fallback behavior and advances one source position between lines.
    #[must_use]
    pub fn from_v8_line_lengths(line_lengths: &[u32]) -> Option<Self> {
        if line_lengths.is_empty() {
            return None;
        }

        let mut line_starts = Vec::with_capacity(line_lengths.len());
        line_starts.push(0);
        let mut offset = 0u32;
        for length in line_lengths
            .iter()
            .take(line_lengths.len().saturating_sub(1))
        {
            offset = offset.saturating_add(*length).saturating_add(1);
            line_starts.push(offset);
        }
        Some(Self { line_starts })
    }

    /// Convert a V8 source offset to a 1-indexed line + 0-indexed column.
    ///
    /// Offsets at or past the end of the source clamp to the last line +
    /// remaining column.
    #[must_use]
    pub fn position(&self, source_offset: u32) -> IstanbulPosition {
        // Binary search for the last line_start <= source_offset.
        let line_zero_indexed = match self.line_starts.binary_search(&source_offset) {
            Ok(exact) => exact,
            Err(insertion_point) => insertion_point.saturating_sub(1),
        };
        let line_start = self.line_starts[line_zero_indexed];
        IstanbulPosition {
            line: (line_zero_indexed as u32) + 1,
            column: source_offset.saturating_sub(line_start),
        }
    }
}

// -- Normalizer -------------------------------------------------------------

/// Input bundle to [`normalize_script`].
pub struct ScriptInput<'a> {
    /// Absolute path to the source file (already resolved from V8's `url`).
    pub path: &'a str,
    /// Full source text used to convert byte offsets.
    pub source: &'a str,
    /// V8 coverage entry for this script.
    pub script: &'a ScriptCoverage,
}

/// Convert one V8 [`ScriptCoverage`] entry into an [`IstanbulFileCoverage`].
///
/// Each V8 [`FunctionCoverage`] contributes one Istanbul function entry whose
/// hit count is taken from the function's first range (the outermost
/// `[startOffset, endOffset)`). Block-level sub-ranges are deliberately not
/// flattened into separate functions — that's the closed-source three-state
/// tracker's job.
#[must_use]
pub fn normalize_script(input: &ScriptInput<'_>) -> IstanbulFileCoverage {
    let table = LineOffsetTable::from_source(input.source);
    let mut fn_map = std::collections::BTreeMap::new();
    let mut hits = std::collections::BTreeMap::new();
    for (idx, function) in input.script.functions.iter().enumerate() {
        let key = format!("f{idx}");
        let outer = function.ranges.first().copied().unwrap_or(CoverageRange {
            start_offset: 0,
            end_offset: 0,
            count: 0,
        });
        let start_pos = table.position(outer.start_offset);
        let end_pos = table.position(outer.end_offset);
        fn_map.insert(
            key.clone(),
            IstanbulFunction {
                name: if function.function_name.is_empty() {
                    "(anonymous)".to_owned()
                } else {
                    function.function_name.clone()
                },
                decl: IstanbulRange {
                    start: start_pos,
                    end: start_pos,
                },
                loc: IstanbulRange {
                    start: start_pos,
                    end: end_pos,
                },
                line: start_pos.line,
            },
        );
        hits.insert(key, outer.count);
    }
    IstanbulFileCoverage {
        path: input.path.to_owned(),
        fn_map,
        f: hits,
    }
}

// Manual Copy for IstanbulPosition + CoverageRange to keep normalize_script cheap.
impl Copy for CoverageRange {}
impl Copy for IstanbulPosition {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_table_handles_lf() {
        let table = LineOffsetTable::from_source("a\nbb\nccc");
        assert_eq!(table.position(0).line, 1);
        assert_eq!(table.position(0).column, 0);
        assert_eq!(table.position(2).line, 2);
        assert_eq!(table.position(2).column, 0);
        assert_eq!(table.position(5).line, 3);
        assert_eq!(table.position(5).column, 0);
    }

    #[test]
    fn line_table_handles_crlf() {
        let table = LineOffsetTable::from_source("a\r\nbb\r\nccc");
        assert_eq!(table.position(3).line, 2);
        assert_eq!(table.position(3).column, 0);
    }

    #[test]
    fn line_table_handles_lone_cr() {
        let table = LineOffsetTable::from_source("a\rbb");
        assert_eq!(table.position(2).line, 2);
        assert_eq!(table.position(2).column, 0);
    }

    #[test]
    fn line_table_uses_utf16_offsets_for_non_ascii_source() {
        let source = "const smile = \"😀\";\nfunction after_emoji() {}\n";
        let function_byte_offset = source
            .find("function")
            .expect("test source should contain function");
        let function_v8_offset = source[..function_byte_offset].encode_utf16().count() as u32;

        assert_ne!(function_v8_offset, function_byte_offset as u32);

        let table = LineOffsetTable::from_source(source);
        let pos = table.position(function_v8_offset);

        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 0);
    }

    #[test]
    fn line_table_builds_from_v8_line_lengths() {
        let table = LineOffsetTable::from_v8_line_lengths(&[20, 12])
            .expect("line lengths should build table");

        assert_eq!(table.position(20).line, 1);
        assert_eq!(table.position(20).column, 20);
        assert_eq!(table.position(21).line, 2);
        assert_eq!(table.position(21).column, 0);
    }

    #[test]
    fn line_table_clamps_past_end() {
        let table = LineOffsetTable::from_source("abc");
        let pos = table.position(100);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 100);
    }

    #[test]
    fn normalize_round_trips_function_hits() {
        let source = "function alpha() {}\nfunction beta() {}\n";
        let script = ScriptCoverage {
            script_id: "1".into(),
            url: "file:///t/foo.js".into(),
            functions: vec![
                FunctionCoverage {
                    function_name: "alpha".into(),
                    ranges: vec![CoverageRange {
                        start_offset: 0,
                        end_offset: 19,
                        count: 7,
                    }],
                    is_block_coverage: false,
                },
                FunctionCoverage {
                    function_name: "beta".into(),
                    ranges: vec![CoverageRange {
                        start_offset: 20,
                        end_offset: 39,
                        count: 0,
                    }],
                    is_block_coverage: false,
                },
            ],
        };
        let normalized = normalize_script(&ScriptInput {
            path: "/t/foo.js",
            source,
            script: &script,
        });
        assert_eq!(normalized.f["f0"], 7);
        assert_eq!(normalized.f["f1"], 0);
        assert_eq!(normalized.fn_map["f0"].name, "alpha");
        assert_eq!(normalized.fn_map["f1"].line, 2);
    }

    #[test]
    fn anonymous_function_renamed() {
        let source = "() => {}";
        let script = ScriptCoverage {
            script_id: "1".into(),
            url: "file:///t/anon.js".into(),
            functions: vec![FunctionCoverage {
                function_name: String::new(),
                ranges: vec![CoverageRange {
                    start_offset: 0,
                    end_offset: 8,
                    count: 1,
                }],
                is_block_coverage: false,
            }],
        };
        let normalized = normalize_script(&ScriptInput {
            path: "/t/anon.js",
            source,
            script: &script,
        });
        assert_eq!(normalized.fn_map["f0"].name, "(anonymous)");
    }

    #[test]
    fn parse_node_v8_coverage_dump() {
        let raw = serde_json::json!({
            "result": [{
                "scriptId": "42",
                "url": "file:///t/x.js",
                "functions": [{
                    "functionName": "a",
                    "ranges": [{"startOffset": 0, "endOffset": 10, "count": 3}],
                    "isBlockCoverage": false
                }]
            }]
        });
        let dump: V8CoverageDump = serde_json::from_value(raw).unwrap();
        assert_eq!(dump.result.len(), 1);
        assert_eq!(dump.result[0].functions[0].function_name, "a");
    }

    #[test]
    fn parse_istanbul_coverage_with_null_columns() {
        let raw = serde_json::json!({
            "/t/linkUtils.ts": {
                "path": "/t/linkUtils.ts",
                "fnMap": {
                    "0": {
                        "name": "normalizeInternalLink",
                        "decl": {
                            "start": { "line": 66, "column": 0 },
                            "end": { "line": 66, "column": null }
                        },
                        "loc": {
                            "start": { "line": 66, "column": 0 },
                            "end": { "line": 76, "column": null }
                        },
                        "line": 66
                    }
                },
                "f": { "0": 9 }
            }
        });

        let dump: std::collections::BTreeMap<String, IstanbulFileCoverage> =
            serde_json::from_value(raw).unwrap();
        let file = &dump["/t/linkUtils.ts"];
        assert_eq!(file.fn_map["0"].decl.end.column, 0);
        assert_eq!(file.fn_map["0"].loc.end.column, 0);
        assert_eq!(file.f["0"], 9);
    }
}
