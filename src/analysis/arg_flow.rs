//! Light argument dataflow: what flows into this call's argument?
//!
//! Full dataflow is LSP-grade and explicitly deferred (FUTURE Tier 4).
//! This answers the question intra-procedurally with a bounded
//! transitive walk: given a call site line, find the argument
//! expression and follow same-file assignments up to `depth`
//! (default 3, max 5), so `worker ← controller ← db` chains
//! resolve without manual hops.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io;

use serde_json::{json, Value};
use tree_sitter::Node;

use crate::analysis::path_utils;
use crate::common::format;
use crate::mcp_types::{CallToolResult, CallToolResultExt};
use crate::parser::{detect_language, parse_code, Language};

const HEADER: &str = "arg|kind|file|line|text";
// kind is `call` (depth 0) or `assign:N` (N = hop depth, 1-based).

/// Args: `file_path`, `line` (1-based call site), optional `arg`
/// (0-based index, default 0), optional `symbol` (call name filter),
/// optional `depth` (default 3, max 5).
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let file_path = arguments["file_path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'file_path' argument",
        )
    })?;
    let line = arguments["line"].as_u64().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Missing or invalid 'line' argument")
    })? as usize;
    let arg_index = arguments["arg"].as_u64().unwrap_or(0) as usize;
    let symbol_filter = arguments["symbol"].as_str();
    let depth = arguments["depth"]
        .as_u64()
        .map(|v| v as usize)
        .unwrap_or(3)
        .clamp(1, 5);

    let source = fs::read_to_string(file_path).map_err(|e| {
        io::Error::new(io::ErrorKind::NotFound, format!("read {file_path}: {e}"))
    })?;
    let language = detect_language(file_path).map_err(|e| {
        io::Error::new(io::ErrorKind::Unsupported, format!("language: {e}"))
    })?;
    let tree = parse_code(&source, language).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("parse: {e}"))
    })?;

    let call = find_call_at_line(tree.root_node(), &source, language, line, symbol_filter)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("No call expression at {file_path}:{line}"),
            )
        })?;

    let args = call_args(call.node, &source);
    let arg_text = args.get(arg_index).cloned().unwrap_or_default();
    let call_name = call.name;

    let mut rows: Vec<String> = Vec::new();
    // Row 0: the call site itself.
    rows.push(format::format_row(&[
        arg_text.as_str(),
        "call",
        path_utils::to_relative_path(file_path).as_str(),
        &line.to_string(),
        format!("{call_name}({})", args.join(", ")).as_str(),
    ]));

    // Transitive walk: follow bare-identifier assignments same-file,
    // breadth-first up to `depth`, with a visited set against cycles.
    let mut visited: HashSet<(String, usize)> = HashSet::new();
    let mut queue: VecDeque<(String, usize, usize)> = VecDeque::new();
    for ident in rhs_idents(&arg_text) {
        queue.push_back((ident, line, 1));
    }
    if is_bare_identifier(&arg_text) && rhs_idents(&arg_text).is_empty() {
        queue.push_back((arg_text.trim().to_string(), line, 1));
    }
    while let Some((ident, before, d)) = queue.pop_front() {
        if d > depth {
            continue;
        }
        if let Some((row_line, rhs, row)) = latest_assignment(
            tree.root_node(),
            &source,
            language,
            &ident,
            before,
            file_path,
        ) {
            if !visited.insert((ident.clone(), row_line)) {
                continue;
            }
            // Rewrite row with depth column.
            rows.push(with_depth(&row, d));
            if d < depth {
                for next in rhs_idents(&rhs) {
                    queue.push_back((next, row_line, d + 1));
                }
            }
        }
    }

    let mut result = json!({
        "call": call_name,
        "arg": arg_text,
        "h": HEADER,
        "flows": rows.join("\n"),
    });
    let _ = &mut result;
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}

struct CallSite {
    node: Node<'static>,
    name: String,
}

// SAFETY: we transmute lifetimes to return an owned handle; the tree
// outlives this call since we only use node positions/text within execute.
fn find_call_at_line(
    root: Node,
    source: &str,
    language: Language,
    line: usize,
    symbol_filter: Option<&str>,
) -> Option<CallSite> {
    let target_row = line.saturating_sub(1);
    let mut best: Option<(Node, String, usize)> = None;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        if is_call_node(n.kind(), language)
            && n.start_position().row <= target_row
            && target_row <= n.end_position().row
        {
            let name = call_name(n, source).unwrap_or_default();
            if let Some(f) = symbol_filter {
                if name != f {
                    // keep searching children
                } else {
                    let span = n.end_position().row - n.start_position().row;
                    if best.as_ref().map(|(_, _, s)| span < *s).unwrap_or(true) {
                        // SAFETY: see note above; node ids/positions copied out only.
                        let raw: Node<'static> =
                            unsafe { std::mem::transmute::<Node<'_>, Node<'static>>(n) };
                        best = Some((raw, name, span));
                    }
                }
            } else {
                let span = n.end_position().row - n.start_position().row;
                if best.as_ref().map(|(_, _, s)| span < *s).unwrap_or(true) {
                    let raw: Node<'static> =
                        unsafe { std::mem::transmute::<Node<'_>, Node<'static>>(n) };
                    best = Some((raw, name, span));
                }
            }
        }
        let mut c = n.walk();
        for child in n.named_children(&mut c) {
            stack.push(child);
        }
    }
    best.map(|(node, name, _)| CallSite { node, name })
}

fn is_call_node(kind: &str, language: Language) -> bool {
    match language {
        Language::Rust => matches!(kind, "call_expression" | "method_call_expression"),
        Language::Python => kind == "call",
        Language::JavaScript | Language::TypeScript | Language::Go => kind == "call_expression",
        Language::Java | Language::CSharp | Language::Swift => kind.ends_with("invocation"),
        Language::C | Language::Cpp => kind == "call_expression",
        Language::Html | Language::Css => false,
    }
}

fn call_name(node: Node, source: &str) -> Option<String> {
    for field in ["function", "name", "method", "field"] {
        if let Some(child) = node.child_by_field_name(field) {
            if let Some(name) = last_ident(child, source) {
                return Some(name);
            }
        }
    }
    let mut c = node.walk();
    for child in node.named_children(&mut c) {
        if let Some(name) = last_ident(child, source) {
            return Some(name);
        }
    }
    None
}

fn last_ident(node: Node, source: &str) -> Option<String> {
    if node.kind() == "identifier" || node.kind().ends_with("_identifier") {
        return node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
    }
    let mut found = None;
    let mut c = node.walk();
    for child in node.named_children(&mut c) {
        if let Some(n) = last_ident(child, source) {
            found = Some(n);
        }
    }
    found
}

fn call_args(node: Node, source: &str) -> Vec<String> {
    // Find argument_list / arguments node.
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == "argument_list" || n.kind() == "arguments" {
            let mut out = Vec::new();
            let mut c = n.walk();
            for child in n.named_children(&mut c) {
                if child.kind() == "," || child.kind() == "(" || child.kind() == ")" {
                    continue;
                }
                if let Ok(t) = child.utf8_text(source.as_bytes()) {
                    // Skip punctuation-only children.
                    let t = t.trim();
                    if !t.is_empty() && t != "," {
                        out.push(t.to_string());
                    }
                }
            }
            return out;
        }
        let mut c = n.walk();
        for child in n.named_children(&mut c) {
            stack.push(child);
        }
    }
    Vec::new()
}

fn is_bare_identifier(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !t.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true)
}

/// Identifiers inside an expression (RHS, call arg, member base).
/// `request.plugins` yields `request`; `a + b` yields `a`, `b`.
fn rhs_idents(expr: &str) -> Vec<String> {
    const SKIP: &[&str] = &[
        "self", "this", "true", "false", "none", "null", "nil", "return", "await", "async",
    ];
    let mut out = Vec::new();
    // For `a.b.c`, the base `a` is what we can trace same-file.
    let base = expr.trim().split('.').next().unwrap_or("").trim();
    let base = base.split('(').next().unwrap_or("").trim();
    for tok in base.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if tok.is_empty() || SKIP.contains(&tok) {
            continue;
        }
        if tok.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true) {
            continue;
        }
        if !out.iter().any(|t| t == tok) {
            out.push(tok.to_string());
        }
    }
    out
}

fn with_depth(row: &str, depth: usize) -> String {
    // Row shape: arg|kind|file|line|text → insert depth before text.
    // Escaped pipes make exact split unsafe; depth is appended to kind instead:
    // `assign` → `assign:2`. Keeps HEADER stable for old clients.
    row.replacen("|assign|", &format!("|assign:{depth}|"), 1)
}

fn latest_assignment(
    root: Node,
    source: &str,
    _language: Language,
    ident: &str,
    before_line: usize,
    file_path: &str,
) -> Option<(usize, String, String)> {
    // Walk for assignment-like nodes whose target text mentions ident,
    // keeping the latest one before the call line.
    // Returns (line, rhs, row).
    let mut best: Option<(usize, String, String)> = None;
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let k = n.kind();
        if k.contains("assignment") || k == "variable_declarator" || k == "lexical_declaration" {
            let row = n.start_position().row + 1;
            if row < before_line {
                if let Ok(text) = n.utf8_text(source.as_bytes()) {
                    let first_line = text.lines().next().unwrap_or("").trim().to_string();
                    // Heuristic: LHS contains our identifier as a token.
                    let mut parts = first_line.splitn(2, '=');
                    let lhs = parts.next().unwrap_or("");
                    let rhs = parts.next().unwrap_or("").to_string();
                    if lhs.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|t| t == ident) {
                        match &best {
                            Some((r, _, _)) if *r >= row => {}
                            _ => {
                                let row_text = format::format_row(&[
                                    ident,
                                    "assign",
                                    path_utils::to_relative_path(file_path).as_str(),
                                    &row.to_string(),
                                    first_line.as_str(),
                                ]);
                                best = Some((row, rhs, row_text));
                            }
                        }
                    }
                }
            }
        }
        let mut c = n.walk();
        for child in n.named_children(&mut c) {
            stack.push(child);
        }
    }
    best
}
