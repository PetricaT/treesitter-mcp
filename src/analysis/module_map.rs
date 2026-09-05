//! Ownership and module boundary detection.
//!
//! Agents violate module boundaries they can't see. One row per file:
//! `module` (relative parent dir), `exports` (top-level public-ish
//! symbols), `file`. Language-aware via the shape extractors.

use std::io;
use std::path::Path;

use serde_json::{json, Value};
use tiktoken_rs::cl100k_base;

use crate::analysis::path_utils;
use crate::analysis::shape::extract_enhanced_shape;
use crate::common::format;
use crate::common::project_files::collect_project_files;
use crate::mcp_types::{CallToolResult, CallToolResultExt};
use crate::parser::detect_language;

const HEADER: &str = "module|exports|file";

/// Args: `path`, optional `max_tokens`, `offset`, `limit`.
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let path_str = arguments["path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'path' argument",
        )
    })?;
    let max_tokens = arguments["max_tokens"].as_u64().map(|v| v as usize);
    let offset = arguments["offset"].as_u64().unwrap_or(0) as usize;
    let limit = arguments["limit"].as_u64().map(|v| v as usize);

    let path = Path::new(path_str);
    if !path.exists() {
        return Err(crate::common::suggest::missing_file_err(path_str));
    }

    let files: Vec<std::path::PathBuf> = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        collect_project_files(path)?
    };

    let mut rows: Vec<String> = Vec::new();
    for file in files {
        if detect_language(&file).is_err() {
            continue;
        }
        let Ok(language) = detect_language(&file) else {
            continue;
        };
        let Ok((tree, source)) = crate::common::cache::cached_tree(&file, language) else {
            continue;
        };
        let Ok(shape) = extract_enhanced_shape(&tree, &source, language, None, false) else {
            continue;
        };
        let mut exports: Vec<String> = shape.functions.iter().map(|f| f.name.clone()).collect();
        exports.extend(shape.structs.iter().map(|s| s.name.clone()));
        exports.extend(shape.classes.iter().map(|c| c.name.clone()));
        exports.extend(shape.interfaces.iter().map(|i| i.name.clone()));
        exports.extend(shape.traits.iter().map(|t| t.name.clone()));
        exports.sort();
        exports.dedup();
        if exports.is_empty() {
            continue;
        }
        // Cap exports per row so one giant file doesn't eat the budget.
        if exports.len() > 12 {
            exports.truncate(12);
        }
        let rel = path_utils::to_relative_path(file.to_string_lossy().as_ref());
        let module = Path::new(&rel)
            .parent()
            .map(|p| {
                let s = p.to_string_lossy().to_string();
                if s.is_empty() {
                    ".".to_string()
                } else {
                    s
                }
            })
            .unwrap_or_else(|| ".".to_string());
        rows.push(format::format_row(&[
            module.as_str(),
            exports.join(", ").as_str(),
            rel.as_str(),
        ]));
    }
    rows.sort();

    let total = rows.len();
    let paged: Vec<String> = {
        let it = rows.into_iter().skip(offset);
        match limit {
            Some(n) => it.take(n).collect(),
            None => it.collect(),
        }
    };

    let (final_rows, truncated) = match max_tokens {
        Some(budget) => enforce(&paged.join("\n"), budget)?,
        None => (paged.join("\n"), false),
    };

    let mut result = json!({
        "h": HEADER,
        "modules": final_rows,
        "total": total,
        "offset": offset,
        "hint": "internal vs public is heuristic; visibility rules vary by language",
    });
    if truncated {
        result["@"] = json!({"t": true});
    }
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}

fn enforce(rows: &str, budget: usize) -> Result<(String, bool), io::Error> {
    let bpe =
        cl100k_base().map_err(|e| io::Error::other(format!("tokenizer: {e}")))?;
    let mut lines: Vec<&str> = if rows.is_empty() {
        Vec::new()
    } else {
        rows.lines().collect()
    };
    let mut truncated = false;
    loop {
        let joined = lines.join("\n");
        let candidate = json!({"h": HEADER, "modules": joined});
        let text = serde_json::to_string(&candidate).unwrap_or_default();
        if bpe.encode_with_special_tokens(&text).len() <= budget {
            return Ok((joined, truncated));
        }
        if lines.pop().is_none() {
            return Ok((String::new(), true));
        }
        truncated = true;
    }
}
