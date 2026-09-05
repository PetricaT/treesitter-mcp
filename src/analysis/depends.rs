//! Include-graph query: does A transitively include B?
//!
//! Adding `#include "x.h"` needs a manual cycle check via grep today.
//! `depends_on(from, to)` answers it with a BFS over project-local
//! `#include "..."` / import edges plus a chain for review.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::analysis::dependencies::resolve_dependencies;
use crate::analysis::path_utils;
use crate::mcp_types::{CallToolResult, CallToolResultExt};
use crate::parser::detect_language;

/// Args: `from` (file), `to` (file), optional `project_root`.
/// Output: {from, to, reachable, chain[]}.
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let from = arguments["from"]
        .as_str()
        .or_else(|| arguments["file_path"].as_str())
        .or_else(|| arguments["source"].as_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing or invalid 'from' argument",
            )
        })?;
    let to = arguments["to"]
        .as_str()
        .or_else(|| arguments["target"].as_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Missing or invalid 'to' argument",
            )
        })?;

    let from_path = PathBuf::from(from);
    let to_path = PathBuf::from(to);
    if !from_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Path does not exist: {from}"),
        ));
    }
    if !to_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Path does not exist: {to}"),
        ));
    }

    let root = arguments["project_root"]
        .as_str()
        .map(PathBuf::from)
        .or_else(|| {
            path_utils::find_project_root(&from_path)
                .or_else(|| from_path.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| PathBuf::from("."));

    let target = fs::canonicalize(&to_path).unwrap_or(to_path.clone());
    let start = fs::canonicalize(&from_path).unwrap_or(from_path.clone());

    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut prev: std::collections::HashMap<PathBuf, PathBuf> = std::collections::HashMap::new();
    let mut queue = VecDeque::from([start.clone()]);
    visited.insert(start.clone());

    let mut reachable = start == target;
    while let Some(cur) = queue.pop_front() {
        if cur == target {
            reachable = true;
            break;
        }
        for dep in direct_deps(&cur, &root) {
            if visited.insert(dep.clone()) {
                prev.insert(dep.clone(), cur.clone());
                queue.push_back(dep);
            }
        }
        if visited.contains(&target) {
            reachable = true;
            break;
        }
    }

    let chain: Vec<String> = if reachable {
        let mut rev = vec![target.clone()];
        let mut cur = &target;
        while let Some(p) = prev.get(cur) {
            rev.push(p.clone());
            cur = p;
            if cur == &start {
                break;
            }
        }
        if rev.last().map(|p| p != &start).unwrap_or(true) && start != target {
            rev.push(start.clone());
        }
        rev.reverse();
        rev.iter()
            .map(|p| path_utils::to_relative_path(p.to_string_lossy().as_ref()))
            .collect()
    } else {
        Vec::new()
    };

    let result = json!({
        "from": path_utils::to_relative_path(from),
        "to": path_utils::to_relative_path(to),
        "reachable": reachable,
        "chain": chain.join("\n"),
        "hint": if reachable { "reachable; adding the edge closes a cycle — pick the other direction or break it" } else { "not reachable; safe to add the include" },
    });
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}

fn direct_deps(file: &Path, root: &Path) -> Vec<PathBuf> {
    let Ok(source) = fs::read_to_string(file) else {
        return Vec::new();
    };
    let Ok(language) = detect_language(file) else {
        return Vec::new();
    };
    resolve_dependencies(language, &source, file, root)
        .into_iter()
        .filter_map(|p| fs::canonicalize(&p).ok().or(Some(p)))
        .collect()
}
