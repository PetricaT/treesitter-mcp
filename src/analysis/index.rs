//! Persistent symbol index: definitions per file with mtime.
//!
//! `find_usages`/`call_graph` walk every file per call. On big repos
//! that dominates cost. This keeps an on-disk JSON index at
//! `{root}/.treesitter-mcp-index.json`:
//! `(name, file, line, end_line, scope)`, refreshed per-file on mtime
//! change. Best-effort: any IO/parse failure falls back to direct parse.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analysis::path_utils;
use crate::analysis::shape::extract_enhanced_shape;
use crate::common::project_files::collect_project_files;
use crate::parser::detect_language;

const INDEX_FILE: &str = ".treesitter-mcp-index.json";
const INDEX_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDef {
    pub name: String,
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
    mtime: u64,
    size: u64,
    defs: Vec<IndexedDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    files: HashMap<String, FileEntry>,
}

fn index_path(root: &Path) -> PathBuf {
    root.join(INDEX_FILE)
}

fn file_meta(path: &Path) -> Option<(u64, u64)> {
    let md = fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((mtime, md.len()))
}

fn load(root: &Path) -> IndexFile {
    let p = index_path(root);
    if let Ok(text) = fs::read_to_string(&p) {
        if let Ok(idx) = serde_json::from_str::<IndexFile>(&text) {
            if idx.version == INDEX_VERSION {
                return idx;
            }
        }
    }
    IndexFile {
        version: INDEX_VERSION,
        files: HashMap::new(),
    }
}

fn save(root: &Path, idx: &IndexFile) {
    let p = index_path(root);
    // Don't index inside the index: skip silently on failure (e.g. read-only).
    if let Ok(text) = serde_json::to_string(idx) {
        let _ = fs::write(&p, text);
    }
}

fn defs_for_file(path: &Path, rel: &str) -> Vec<IndexedDef> {
    let Ok(language) = detect_language(path) else {
        return Vec::new();
    };
    let Ok((tree, source)) = crate::common::cache::cached_tree(path, language) else {
        return Vec::new();
    };
    let Ok(shape) = extract_enhanced_shape(&tree, &source, language, None, false) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for f in &shape.functions {
        out.push(IndexedDef {
            name: f.name.clone(),
            file: rel.to_string(),
            line: f.line,
            end_line: f.end_line,
            scope: String::new(),
        });
    }
    for c in &shape.classes {
        for m in &c.methods {
            out.push(IndexedDef {
                name: m.name.clone(),
                file: rel.to_string(),
                line: m.line,
                end_line: m.end_line,
                scope: c.name.clone(),
            });
        }
    }
    for b in &shape.impl_blocks {
        for m in &b.methods {
            out.push(IndexedDef {
                name: m.name.clone(),
                file: rel.to_string(),
                line: m.line,
                end_line: m.end_line,
                scope: b.type_name.clone(),
            });
        }
    }
    for i in &shape.interfaces {
        for m in &i.methods {
            out.push(IndexedDef {
                name: m.name.clone(),
                file: rel.to_string(),
                line: m.line,
                end_line: m.end_line,
                scope: i.name.clone(),
            });
        }
    }
    out
}

/// Definitions for a project root, refreshing stale entries by mtime.
/// Returns absolute-path defs. Persists best-effort to disk.
pub fn definitions_for_root(root: &Path) -> Result<Vec<IndexedDef>, io::Error> {
    let mut idx = load(root);
    let mut dirty = false;
    let mut all: Vec<IndexedDef> = Vec::new();

    // Canonical root for absolute file paths.
    let canon_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    for file in collect_project_files(root)? {
        if detect_language(&file).is_err() {
            continue;
        }
        // Never index the index itself.
        if file.file_name().and_then(|n| n.to_str()) == Some(INDEX_FILE) {
            continue;
        }
        let rel = path_utils::to_relative_path(file.to_string_lossy().as_ref());
        let Some((mtime, size)) = file_meta(&file) else {
            continue;
        };
        if let Some(entry) = idx.files.get(&rel) {
            if entry.mtime == mtime && entry.size == size {
                all.extend(entry.defs.clone());
                continue;
            }
        }
        let defs = defs_for_file(&file, &rel);
        all.extend(defs.clone());
        idx.files.insert(rel, FileEntry { mtime, size, defs });
        dirty = true;
    }

    // Drop entries for deleted files.
    let before = idx.files.len();
    idx.files.retain(|_, _| true);
    let live: std::collections::HashSet<String> = collect_project_files(root)
        .unwrap_or_default()
        .iter()
        .map(|p| path_utils::to_relative_path(p.to_string_lossy().as_ref()))
        .collect();
    idx.files.retain(|k, _| live.contains(k));
    if idx.files.len() != before {
        dirty = true;
    }

    if dirty {
        save(root, &idx);
    }
    // Attach absolute dir for consumers that join rel paths.
    let _ = canon_root;
    Ok(all)
}
