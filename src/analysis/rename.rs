//! Symbol rename dry-run: what would change, without editing.
//!
//! Combines `find_usages` (scope-qualified) with per-location edit
//! previews. Confidence comes from the usage row's own `conf` column.

use std::io;

use serde_json::{json, Value};

use crate::common::format;
use crate::mcp_types::{CallToolResult, CallToolResultExt};

const HEADER: &str = "file|line|col|old_text|new_text|confidence";

/// Args: `symbol`, `new_name`, `path`, optional `context_lines`,
/// `max_tokens`, `offset`, `limit`.
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let symbol = arguments["symbol"]
        .as_str()
        .or_else(|| arguments["symbol_name"].as_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing or invalid 'symbol' argument",
            )
        })?;
    let new_name = arguments["new_name"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'new_name' argument",
        )
    })?;
    if new_name.trim().is_empty() || new_name.contains(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "'new_name' must be a bare identifier",
        ));
    }
    let path = arguments["path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'path' argument",
        )
    })?;
    let offset = arguments["offset"].as_u64().unwrap_or(0) as usize;
    let limit = arguments["limit"].as_u64().map(|v| v as usize);
    let max_tokens = arguments["max_tokens"].as_u64().map(|v| v as usize);

    // Reuse syntax-aware search; locations only keeps it cheap.
    let usage_args = json!({
        "symbol": symbol,
        "path": path,
        "context_lines": 0,
        "max_context_lines": 0,
    });
    let usage_result = crate::analysis::find_usages::execute(&usage_args)?;
    let usage_text = call_text(&usage_result);
    let usage: Value = serde_json::from_str(&usage_text)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("usages: {e}")))?;
    let rows_str = usage.get("u").and_then(Value::as_str).unwrap_or("");

    let mut edits: Vec<(String, usize, usize, String)> = Vec::new();
    for row in rows_str.lines() {
        let cols: Vec<&str> = row.split('|').collect();
        if cols.len() < 8 {
            continue;
        }
        // find_usages escapes pipes; unescape for confidence read only.
        let conf = cols[6].to_string();
        // Skip imports: renaming an import specifier is a different edit.
        if cols[3] == "import" {
            continue;
        }
        edits.push((
            unescape(cols[0]),
            cols[1].parse().unwrap_or(0),
            cols[2].parse().unwrap_or(0),
            conf,
        ));
    }
    edits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));

    let total = edits.len();
    let paged: Vec<_> = {
        let it = edits.into_iter().skip(offset);
        match limit {
            Some(n) => it.take(n).collect(),
            None => it.collect(),
        }
    };

    let mut files = std::collections::HashSet::new();
    let rows: Vec<String> = paged
        .iter()
        .map(|(f, l, c, conf)| {
            files.insert(f.clone());
            format::format_row(&[
                f.as_str(),
                &l.to_string(),
                &c.to_string(),
                symbol,
                new_name,
                conf.as_str(),
            ])
        })
        .collect();

    let mut result = json!({
        "sym": symbol,
        "new_name": new_name,
        "h": HEADER,
        "edits": rows.join("\n"),
        "files_modified": files.len(),
        "total_edits": total,
        "offset": offset,
        "hint": "dry run only; apply with apply_symbol_edit per site or LSP rename, then verify_edit",
    });
    if let Some(budget) = max_tokens {
        // Hard cap by dropping trailing rows (already paged; enforce cheaply).
        let bpe = tiktoken_rs::cl100k_base()
            .map_err(|e| io::Error::other(format!("tokenizer: {e}")))?;
        let mut kept = rows;
        loop {
            let candidate = json!({"edits": kept.join("\n")});
            let text = serde_json::to_string(&candidate).unwrap_or_default();
            if bpe.encode_with_special_tokens(&text).len() <= budget {
                result["edits"] = json!(kept.join("\n"));
                if kept.len() < total {
                    result["@"] = json!({"t": true});
                }
                break;
            }
            if kept.pop().is_none() {
                result["edits"] = json!("");
                result["@"] = json!({"t": true});
                break;
            }
            result["@"] = json!({"t": true});
        }
    }
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}

fn call_text(r: &CallToolResult) -> String {
    if let Some(first) = r.content.first() {
        let s = serde_json::to_string(first).unwrap_or_default();
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(t) = v.get("text").and_then(Value::as_str) {
                return t.to_string();
            }
        }
        s
    } else {
        String::new()
    }
}

fn unescape(s: &str) -> String {
    s.replace("\\|", "|")
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\\\", "\\")
}
