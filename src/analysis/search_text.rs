//! Text/pattern search with compact schema.
//!
//! Covers string literals, hook keys, error messages, macro names —
//! anything `find_usages` cannot see because it is not a symbol.
//! Also serves as the `lint(patterns[])` primitive: pass multiple
//! patterns to scan for banned tokens in one call.

use std::fs;
use std::io;
use std::path::Path;

use regex::{Regex, RegexBuilder};
use serde_json::{json, Value};
use tiktoken_rs::cl100k_base;

use crate::analysis::path_utils;
use crate::common::format;
use crate::common::project_files::collect_project_files;
use crate::mcp_types::{CallToolResult, CallToolResultExt};

const SINGLE_HEADER: &str = "file|line|col|context";
const MULTI_HEADER: &str = "pattern|file|line|col|context";

#[derive(Debug, Clone)]
struct Hit {
    pattern: String,
    file: String,
    line: usize,
    col: usize,
    context: String,
}

enum Matcher {
    Literal { needle: String, case_sensitive: bool },
    Regex(Regex),
}

impl Matcher {
    fn find_col(&self, line: &str) -> Option<usize> {
        match self {
            Matcher::Literal {
                needle,
                case_sensitive,
            } => {
                if *case_sensitive {
                    line.find(needle).map(|b| b + 1)
                } else {
                    line.to_lowercase()
                        .find(&needle.to_lowercase())
                        .map(|b| b + 1)
                }
            }
            Matcher::Regex(re) => re.find(line).map(|m| m.start() + 1),
        }
    }

    fn is_match(&self, line: &str) -> bool {
        self.find_col(line).is_some()
    }
}

fn build_matcher(
    pattern: &str,
    regex_mode: bool,
    case_sensitive: bool,
) -> Result<Matcher, io::Error> {
    if regex_mode {
        let re = RegexBuilder::new(pattern)
            .case_insensitive(!case_sensitive)
            .build()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("Bad regex: {e}")))?;
        Ok(Matcher::Regex(re))
    } else {
        Ok(Matcher::Literal {
            needle: pattern.to_string(),
            case_sensitive,
        })
    }
}

/// Execute search_text.
///
/// Args (JSON):
/// - `pattern` (string) OR `patterns` (string[]): literal substring by default
/// - `path` (string, required): file or dir
/// - `regex` (bool, default false)
/// - `case_sensitive` (bool, default true)
/// - `context_lines` (u32, default 0): extra lines joined with `\n` in context col
/// - `offset`/`limit` (u32): paging over total matches
/// - `max_tokens` (u32): hard budget, sets `@.t=true` when truncated
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let path_str = arguments["path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'path' argument",
        )
    })?;

    let patterns: Vec<String> = if let Some(p) = arguments["pattern"].as_str() {
        vec![p.to_string()]
    } else if let Some(arr) = arguments["patterns"].as_array() {
        let mut out = Vec::new();
        for v in arr {
            if let Some(s) = v.as_str() {
                out.push(s.to_string());
            }
        }
        if out.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing or invalid 'pattern'/'patterns' argument",
            ));
        }
        out
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'pattern'/'patterns' argument",
        ));
    };

    let regex_mode = arguments["regex"].as_bool().unwrap_or(false);
    let case_sensitive = arguments["case_sensitive"].as_bool().unwrap_or(true);
    let context_lines = arguments["context_lines"].as_u64().unwrap_or(0) as usize;
    let offset = arguments["offset"].as_u64().unwrap_or(0) as usize;
    let limit = arguments["limit"].as_u64().map(|v| v as usize);
    let max_tokens = arguments["max_tokens"].as_u64().map(|v| v as usize);
    let estimate = arguments["estimate"].as_bool().unwrap_or(false);

    let mut matchers = Vec::new();
    for p in &patterns {
        matchers.push((p.clone(), build_matcher(p, regex_mode, case_sensitive)?));
    }
    let multi = matchers.len() > 1;

    let path = Path::new(path_str);
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            crate::common::suggest::missing_file_err(path_str).to_string(),
        ));
    }

    let files: Vec<std::path::PathBuf> = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        collect_project_files(path)?
    };

    let mut hits: Vec<Hit> = Vec::new();
    for file in files {
        let Ok(content) = crate::common::cache::cached_source(&file)
            .or_else(|_| fs::read_to_string(&file)) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            for (pat, m) in &matchers {
                if m.is_match(line) {
                    let col = m.find_col(line).unwrap_or(1);
                    let context = if context_lines == 0 {
                        line.to_string()
                    } else {
                        let start = idx.saturating_sub(context_lines);
                        let end = (idx + context_lines + 1).min(lines.len());
                        lines[start..end].join("\n")
                    };
                    hits.push(Hit {
                        pattern: pat.clone(),
                        file: file.to_string_lossy().to_string(),
                        line: idx + 1,
                        col,
                        context,
                    });
                    break; // one row per line even if multiple patterns hit
                }
            }
        }
    }

    hits.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.col.cmp(&b.col))
            .then_with(|| a.pattern.cmp(&b.pattern))
    });

    let total = hits.len();
    if estimate {
        // Cost preview without building payload: ~4 chars/token heuristic
        // plus per-row overhead, file scope summary.
        let total_chars: usize = hits.iter().map(|h| h.context.len() + h.file.len() + 16).sum();
        let mut files = std::collections::HashSet::new();
        for h in &hits {
            files.insert(h.file.clone());
        }
        let result = json!({
            "pat": patterns,
            "estimated_tokens": total_chars / 4 + total * 4,
            "estimated_rows": total,
            "scope_summary": format!("{} files", files.len()),
            "total": total,
        });
        let text = serde_json::to_string(&result).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
        })?;
        return Ok(CallToolResult::success(text));
    }
    let paged: Vec<Hit> = {
        let skipped = hits.into_iter().skip(offset);
        match limit {
            Some(n) => skipped.take(n).collect(),
            None => skipped.collect(),
        }
    };

    let header = if multi { MULTI_HEADER } else { SINGLE_HEADER };
    let rows = hits_to_rows(&paged, multi);

    // Budget enforcement with real token counts.
    let (final_rows, truncated) = match max_tokens {
        Some(budget) => enforce_budget(&rows, header, &patterns, budget)?,
        None => (rows, false),
    };

    let hint = crate::common::hints::search_hint(
        total,
        truncated,
        &patterns.first().cloned().unwrap_or_default(),
    );
    let mut result = if multi {
        json!({
            "pat": patterns,
            "h": header,
            "m": final_rows,
            "total": total,
            "offset": offset,
        })
    } else {
        json!({
            "pat": patterns[0],
            "h": header,
            "m": final_rows,
            "total": total,
            "offset": offset,
        })
    };
    if truncated {
        result["@"] = json!({"t": true});
    }
    result["hint"] = json!(hint);

    let json_text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to serialize result: {e}"),
        )
    })?;
    Ok(CallToolResult::success(json_text))
}

fn hits_to_rows(hits: &[Hit], multi: bool) -> String {
    hits.iter()
        .map(|h| {
            let file = path_utils::to_relative_path(&h.file);
            let line = h.line.to_string();
            let col = h.col.to_string();
            if multi {
                format::format_row(&[&h.pattern, &file, &line, &col, &h.context])
            } else {
                format::format_row(&[&file, &line, &col, &h.context])
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn enforce_budget(
    rows: &str,
    header: &str,
    patterns: &[String],
    max_tokens: usize,
) -> Result<(String, bool), io::Error> {
    let bpe = cl100k_base()
        .map_err(|e| io::Error::other(format!("Failed to init tokenizer: {e}")))?;
    let mut lines: Vec<&str> = if rows.is_empty() {
        Vec::new()
    } else {
        rows.lines().collect()
    };
    let mut truncated = false;
    loop {
        let joined = lines.join("\n");
        let candidate = json!({
            "pat": patterns,
            "h": header,
            "m": joined,
        });
        let text = serde_json::to_string(&candidate).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
        })?;
        if bpe.encode_with_special_tokens(&text).len() <= max_tokens {
            return Ok((joined, truncated));
        }
        if lines.pop().is_none() {
            return Ok((String::new(), true));
        }
        truncated = true;
    }
}


