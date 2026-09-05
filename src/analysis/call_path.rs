//! Call path: does A transitively call B?
//!
//! `depends_on` answers it for includes; this answers it for calls.
//! BFS over project-local callee edges (same resolver as `call_graph`),
//! up to depth 5, with visited set. Self-path (`to == from`) doubles
//! as cycle detection.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::analysis::call_graph;
use crate::analysis::path_utils;
use crate::mcp_types::{CallToolResult, CallToolResultExt};

/// Args: `file_path`, `symbol` (from), `to` (dest symbol),
/// optional `depth` (default 5, max 5).
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let file_path = arguments["file_path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'file_path' argument",
        )
    })?;
    let from = arguments["symbol"]
        .as_str()
        .or_else(|| arguments["symbol_name"].as_str())
        .or_else(|| arguments["from"].as_str())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Missing 'symbol' argument")
        })?;
    let to = arguments["to"].as_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Missing 'to' argument")
    })?;
    let max_depth = arguments["depth"]
        .as_u64()
        .map(|v| (v as usize).clamp(1, 5))
        .unwrap_or(5);

    let target_path = PathBuf::from(file_path);
    let root = path_utils::find_project_root(&target_path)
        .or_else(|| target_path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let files = call_graph::collect_supported_files(&root)?;
    let definitions = call_graph::collect_definitions(&files)?;
    let start = call_graph::find_target_definition(&definitions, &target_path, from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Symbol '{from}' not found in {file_path}"),
            )
        })?;

    // BFS with parent pointers for chain reconstruction.
    let mut visited: HashSet<(PathBuf, usize)> = HashSet::new();
    let mut prev: HashMap<(PathBuf, usize), (PathBuf, usize)> = HashMap::new();
    let mut names: HashMap<(PathBuf, usize), String> = HashMap::new();
    let start_key = (start.file.clone(), start.line);
    let mut queue = VecDeque::from([(start.clone(), 0usize)]);
    visited.insert(start_key.clone());
    names.insert(start_key.clone(), start.name.clone());

    let mut found_key: Option<(PathBuf, usize)> = if from == to {
        // Self-path = cycle: need at least one edge back, so don't
        // terminate immediately; seed below handles it.
        None
    } else {
        None
    };

    while let Some((cur, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        let Ok(called) = call_graph::called_names_for_symbol(&cur) else {
            continue;
        };
        for name in called {
            let Some(next) = call_graph::resolve_definition(&definitions, &name, &cur.file)
            else {
                continue;
            };
            let key = (next.file.clone(), next.line);
            if !visited.insert(key.clone()) {
                // Revisit closes a cycle: if it reaches start or dest, record.
                if key == start_key && found_key.is_none() {
                    prev.insert(key.clone(), (cur.file.clone(), cur.line));
                    names.insert(key.clone(), next.name.clone());
                    found_key = Some(key.clone());
                }
                continue;
            }
            prev.insert(key.clone(), (cur.file.clone(), cur.line));
            names.insert(key.clone(), next.name.clone());
            if next.name == to {
                found_key = Some(key.clone());
                queue.clear();
                break;
            }
            queue.push_back((next, depth + 1));
        }
        if found_key.is_some() {
            break;
        }
    }

    let (reachable, chain) = match found_key {
        Some(key) => {
            let mut rev = vec![key.clone()];
            let mut cur = key;
            while cur != start_key {
                let Some(p) = prev.get(&cur) else { break };
                rev.push(p.clone());
                cur = p.clone();
            }
            rev.reverse();
            let rows: Vec<String> = rev
                .iter()
                .map(|(f, l)| {
                    let n = names.get(&(f.clone(), *l)).cloned().unwrap_or_default();
                    format!(
                        "{}@{}:{}",
                        n,
                        path_utils::to_relative_path(f.to_string_lossy().as_ref()),
                        l
                    )
                })
                .collect();
            (true, rows.join("\n"))
        }
        None => (false, String::new()),
    };

    let result = json!({
        "from": from,
        "to": to,
        "reachable": reachable,
        "chain": chain,
    });
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}
