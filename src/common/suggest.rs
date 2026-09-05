//! Compact contextual errors: help agents self-correct in one step.
//!
//! Bare "path does not exist" causes blind retries. These helpers
//! append did-you-mean file suggestions and ambiguous-symbol
//! candidates to error messages (cheap `walkdir` + substring match).

use std::io;
use std::path::Path;

use crate::common::project_files::collect_project_files;

/// io::Error for a missing path with up to 3 did-you-mean suggestions.
pub fn missing_file_err(path_str: &str) -> io::Error {
    let mut msg = format!("Path does not exist: {path_str}");
    let wanted = Path::new(path_str);
    let file_name = wanted
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path_str);
    let root = guess_root(wanted);
    if let Ok(files) = collect_project_files(&root) {
        let mut scored: Vec<(u8, String)> = Vec::new();
        for f in files {
            let rel = crate::analysis::path_utils::to_relative_path(
                f.to_string_lossy().as_ref(),
            );
            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let score = if name == file_name {
                0
            } else if name.to_lowercase() == file_name.to_lowercase() {
                1
            } else if name.contains(file_name) || file_name.contains(name) {
                2
            } else if edit_distance(
                &stem_of(name).to_lowercase(),
                &stem_of(file_name).to_lowercase(),
            ) <= 2
            {
                3
            } else {
                continue;
            };
            scored.push((score, rel));
            if scored.len() >= 20 {
                break;
            }
        }
        scored.sort();
        scored.truncate(3);
        if !scored.is_empty() {
            let list: Vec<_> = scored.iter().map(|(_, r)| r.clone()).collect();
            msg.push_str(&format!("; did you mean: {}", list.join(", ")));
        }
    }
    msg.push_str("; hint: use a repo-relative path from code_map");
    io::Error::new(io::ErrorKind::NotFound, msg)
}

fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) => &name[..i],
        None => name,
    }
}

/// Bounded Levenshtein distance (early exit past bound 2).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 2 {
        return 3;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, &cb) in b.iter().enumerate() {
            cur.push(
                (prev[j] + usize::from(ca != cb))
                    .min(prev[j + 1] + 1)
                    .min(cur[j] + 1),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}

fn guess_root(wanted: &Path) -> std::path::PathBuf {
    if let Some(parent) = wanted.parent() {
        if !parent.as_os_str().is_empty() && parent.exists() {
            if let Some(root) = crate::analysis::path_utils::find_project_root(parent) {
                return root;
            }
            return parent.to_path_buf();
        }
    }
    std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
}

/// io::Error for an unknown symbol with up to 5 candidates from the file.
pub fn unknown_symbol_err(symbol: &str, candidates: &[String], file: &str) -> io::Error {
    let mut scored: Vec<(u8, &String)> = Vec::new();
    let lower = symbol.to_lowercase();
    for c in candidates {
        let cl = c.to_lowercase();
        if cl == lower {
            scored.push((0, c));
        } else if cl.contains(lower.as_str()) || lower.contains(cl.as_str()) {
            scored.push((1, c));
        } else if edit_distance(&cl, &lower) <= 2 {
            scored.push((2, c));
        }
    }
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    let mut msg = format!("Symbol '{symbol}' not found in {file}");
    let list: Vec<_> = scored.into_iter().take(5).map(|(_, c)| c.clone()).collect();
    if !list.is_empty() {
        msg.push_str(&format!("; candidates: {}", list.join(", ")));
    }
    msg.push_str("; hint: code_map the dir or symbol_at_line the location");
    io::Error::new(io::ErrorKind::NotFound, msg)
}
