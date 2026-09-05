//! Session bootstrap: orientation in one call.
//!
//! New sessions reach for 3-4 tools (types, map, entry points, tests).
//! Returns top types, a minimal code map, likely entry points, and
//! test directories under one token budget.

use std::io;
use std::path::Path;

use serde_json::{json, Value};

use crate::common::project_files::collect_project_files;
use crate::mcp_types::{CallToolResult, CallToolResultExt};

/// Args: `path`, optional `max_tokens` (default 3000).
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let path_str = arguments["path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'path' argument",
        )
    })?;
    let max_tokens = arguments["max_tokens"].as_u64().unwrap_or(3000) as usize;
    let path = Path::new(path_str);
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Path does not exist: {path_str}"),
        ));
    }

    // Split budget: 40% types, 40% map, 20% entries/tests.
    let type_budget = max_tokens * 2 / 5;
    let map_budget = max_tokens * 2 / 5;

    let type_args =
        json!({"path": path_str, "max_tokens": type_budget, "count_usages": false});
    let types_text = call_text(
        &crate::analysis::type_map::execute(&type_args)
            .map_err(|e| io::Error::other(e.to_string()))?,
    );
    let types_val: Value = serde_json::from_str(&types_text).unwrap_or(json!({}));

    let map_args = json!({"path": path_str, "max_tokens": map_budget, "detail": "minimal"});
    let map_text = call_text(&crate::analysis::code_map::execute(&map_args)?);
    let map_val: Value = serde_json::from_str(&map_text).unwrap_or(json!({}));

    let (entries, tests) = scan_entries_tests(path);

    let result = json!({
        "path": path_str,
        "types": types_val,
        "map": map_val,
        "entry_points": entries.join("\n"),
        "test_dirs": tests.join("\n"),
        "hint": "bootstrap done; batch_view entry points, minimal_edit_context to edit",
    });
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

fn scan_entries_tests(root: &Path) -> (Vec<String>, Vec<String>) {
    let files = collect_project_files(root).unwrap_or_default();
    let mut entries = Vec::new();
    let mut tests = Vec::new();
    let mut seen_tests = std::collections::HashSet::new();

    for f in &files {
        let rel = crate::analysis::path_utils::to_relative_path(f.to_string_lossy().as_ref());
        let lower = rel.to_lowercase();
        let stem = f
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if stem == "main" || stem == "lib" || stem == "index" || stem == "mod" {
            entries.push(rel.clone());
        }
        // Test dirs: tests/, test/, __tests__/ at any depth.
        for ancestor in f.ancestors() {
            if let Some(name) = ancestor.file_name().and_then(|n| n.to_str()) {
                let n = name.to_lowercase();
                if n == "tests" || n == "test" || n == "__tests__" {
                    let rel_dir = crate::analysis::path_utils::to_relative_path(
                        ancestor.to_string_lossy().as_ref(),
                    );
                    if seen_tests.insert(rel_dir.clone()) {
                        tests.push(rel_dir);
                    }
                    break;
                }
            }
        }
        // Heuristic: files mentioning `fn main` / `if __name__` are entries.
        if lower.ends_with(".rs") || lower.ends_with(".py") {
            if let Ok(src) = std::fs::read_to_string(f) {
                if (src.contains("fn main(") || src.contains("__name__"))
                    && !entries.contains(&rel)
                {
                    entries.push(rel);
                }
            }
        }
    }

    entries.sort();
    entries.dedup();
    tests.sort();
    (entries, tests)
}
