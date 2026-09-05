//! Apply a symbol edit by splicing at the AST span.
//!
//! Agents edit by rewriting files or unified diffs — both brittle.
//! This splices `new_body` over the target symbol's exact line span
//! (from `extract_enhanced_shape`) and verifies the file still parses.

use std::fs;
use std::io;

use serde_json::{json, Value};

use crate::analysis::path_utils;
use crate::analysis::shape::extract_enhanced_shape;
use crate::mcp_types::{CallToolResult, CallToolResultExt};
use crate::parser::{detect_language, parse_code};

/// Args: `file_path`, `symbol_name`, `new_body`, optional `dry_run`.
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let file_path = arguments["file_path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'file_path' argument",
        )
    })?;
    let symbol = arguments["symbol_name"]
        .as_str()
        .or_else(|| arguments["symbol"].as_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing or invalid 'symbol_name' argument",
            )
        })?;
    let new_body = arguments["new_body"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'new_body' argument",
        )
    })?;
    if new_body.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "'new_body' must not be empty",
        ));
    }
    let dry_run = arguments["dry_run"].as_bool().unwrap_or(false);

    let source = fs::read_to_string(file_path).map_err(|e| {
        io::Error::new(io::ErrorKind::NotFound, format!("read {file_path}: {e}"))
    })?;
    let language = detect_language(file_path).map_err(|e| {
        io::Error::new(io::ErrorKind::Unsupported, format!("language: {e}"))
    })?;
    let tree = parse_code(&source, language).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("parse: {e}"))
    })?;
    let shape = extract_enhanced_shape(&tree, &source, language, None, false)?;

    let (line, end_line) = find_span(&shape, symbol).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Symbol '{symbol}' not found in {file_path}"),
        )
    })?;

    let mut lines: Vec<&str> = source.lines().collect();
    let start = line.saturating_sub(1);
    let end = end_line.min(lines.len());
    if start >= lines.len() || start >= end {
        return Err(io::Error::other("symbol span out of range"));
    }
    let replaced = end - start;
    lines.splice(start..end, [new_body]);
    let updated = lines.join("\n") + if source.ends_with('\n') { "\n" } else { "" };

    // Verify the edited file still parses.
    let edited_tree = parse_code(&updated, language).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("edited file fails to parse: {e}"))
    })?;
    let has_error = edited_tree.root_node().has_error();

    if !dry_run {
        fs::write(file_path, &updated)?;
        // Drop stale cache entry by re-caching on next read (mtime changed).
    }

    let result = json!({
        "p": path_utils::to_relative_path(file_path),
        "sym": symbol,
        "dry_run": dry_run,
        "replaced_lines": replaced,
        "parses": !has_error,
        "hint": crate::common::hints::edit_hint(),
    });
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}

fn find_span(
    shape: &crate::analysis::shape::EnhancedFileShape,
    symbol: &str,
) -> Option<(usize, usize)> {
    for f in &shape.functions {
        if f.name == symbol {
            return Some((f.line, f.end_line));
        }
    }
    for s in &shape.structs {
        if s.name == symbol {
            return Some((s.line, s.end_line));
        }
    }
    for c in &shape.classes {
        if c.name == symbol {
            return Some((c.line, c.end_line));
        }
        for m in &c.methods {
            if m.name == symbol {
                return Some((m.line, m.end_line));
            }
        }
    }
    for b in &shape.impl_blocks {
        for m in &b.methods {
            if m.name == symbol {
                return Some((m.line, m.end_line));
            }
        }
    }
    for t in &shape.traits {
        for m in &t.methods {
            if m.name == symbol {
                return Some((m.line, m.end_line));
            }
        }
    }
    None
}
