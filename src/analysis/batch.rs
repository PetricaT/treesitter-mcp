//! Batch symbol fetch: multiple view + usages requests in one call.
//!
//! Every investigation needs 3–5 sequential calls
//! (map dir → view fn A → view fn B → usages). This halves round trips.
//! Items are either view (`{file_path, focus_symbol?, ...}`) or usages
//! (`{kind:"usages", symbol, path, context_lines?}`), keyed in `items`.

use std::io;

use serde_json::{json, Map, Value};
use tiktoken_rs::cl100k_base;

use crate::analysis::{find_usages, view_code};
use crate::mcp_types::{CallToolResult, CallToolResultExt};

/// Args:
/// - `items`: view `{file_path, focus_symbol?, detail?, isolate?, comment_mode?}`
///   or usages `{kind:"usages", symbol, path, context_lines?}`
/// - `max_tokens` (default 4000): shared budget across items
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let items = arguments["items"].as_array().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'items' argument",
        )
    })?;
    if items.is_empty() || items.len() > 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "'items' must have 1..=20 entries",
        ));
    }
    let max_tokens = arguments["max_tokens"].as_u64().unwrap_or(4000) as usize;

    let bpe = cl100k_base()
        .map_err(|e| io::Error::other(format!("tokenizer: {e}")))?;

    let mut out = Map::new();
    let mut truncated = false;

    for (idx, item) in items.iter().enumerate() {
        let is_usages = item
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|k| k == "usages")
            || (item.get("symbol").is_some() && item.get("path").is_some());

        if is_usages {
            let symbol = item["symbol"].as_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("items[{idx}] usages missing 'symbol'"),
                )
            })?;
            let path = item["path"].as_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("items[{idx}] usages missing 'path'"),
                )
            })?;
            let mut args = serde_json::Map::new();
            args.insert("symbol".to_string(), json!(symbol));
            args.insert("path".to_string(), json!(path));
            // Lean usages in batch: locations only unless overridden.
            args.insert(
                "context_lines".to_string(),
                item.get("context_lines").cloned().unwrap_or(json!(1)),
            );
            if let Some(m) = item.get("max_context_lines") {
                args.insert("max_context_lines".to_string(), m.clone());
            }
            let key = format!("usages::{symbol}@{path}");
            let result = match find_usages::execute(&Value::Object(args)) {
                Ok(r) => {
                    let text = call_result_text(&r);
                    serde_json::from_str::<Value>(&text).unwrap_or(json!({"raw": text}))
                }
                Err(e) => json!({"error": e.to_string()}),
            };
            out.insert(key, result);
        } else {
            let file_path = item["file_path"].as_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("items[{idx}] missing 'file_path'"),
                )
            })?;
            let mut args = serde_json::Map::new();
            args.insert("file_path".to_string(), json!(file_path));
            if let Some(d) = item.get("detail").and_then(Value::as_str) {
                args.insert("detail".to_string(), json!(d));
            }
            if let Some(f) = item.get("focus_symbol").and_then(Value::as_str) {
                args.insert("focus_symbol".to_string(), json!(f));
            }
            if let Some(iso) = item.get("isolate").and_then(Value::as_bool) {
                args.insert("isolate".to_string(), json!(iso));
            } else if item.get("focus_symbol").is_some() {
                // Default isolate=true in batch: the whole point is to avoid
                // paying for the whole file per symbol.
                args.insert("isolate".to_string(), json!(true));
            }
            if let Some(c) = item.get("comment_mode").and_then(Value::as_str) {
                args.insert("comment_mode".to_string(), json!(c));
            }
            // Keep batch payloads lean: no cross-file deps per item by default.
            if !item.get("include_deps").is_some() {
                args.insert("include_deps".to_string(), json!(false));
            } else {
                args.insert(
                    "include_deps".to_string(),
                    item["include_deps"].clone(),
                );
            }

            let key = match item.get("focus_symbol").and_then(Value::as_str) {
                Some(sym) => format!("{file_path}::{sym}"),
                None => file_path.to_string(),
            };

            let result = match view_code::execute(&Value::Object(args)) {
                Ok(r) => {
                    let text = call_result_text(&r);
                    serde_json::from_str::<Value>(&text).unwrap_or(json!({"raw": text}))
                }
                Err(e) => json!({"error": e.to_string()}),
            };
            out.insert(key, result);
        }

        // Early stop when over budget.
        let snapshot = serde_json::to_string(&Value::Object(out.clone())).unwrap_or_default();
        if bpe.encode_with_special_tokens(&snapshot).len() > max_tokens {
            truncated = true;
            break;
        }
    }

    // Hard enforcement: drop last items until within budget.
    loop {
        let snapshot = serde_json::to_string(&json!({"items": out})).unwrap_or_default();
        if bpe.encode_with_special_tokens(&snapshot).len() <= max_tokens {
            break;
        }
        let Some(last) = out.keys().next_back().cloned() else {
            break;
        };
        out.remove(&last);
        truncated = true;
        if out.is_empty() {
            break;
        }
    }

    let mut result = json!({"items": out});
    if truncated {
        result["@"] = json!({"t": true});
    }
    result["hint"] = json!("batch done; minimal_edit_context on edit target, affected_by_diff after edits");
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}

fn call_result_text(result: &CallToolResult) -> String {
    if let Some(first) = result.content.first() {
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
