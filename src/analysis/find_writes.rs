//! Assignment search: where does a symbol get *set*.
//!
//! `find_usages("x")` mixes reads and writes. State-flow traces
//! ("where does `current_game_id_` get assigned?") need writes only.
//! Returns the same compact schema as `find_usages` plus `total`/`offset`.

use std::io;
use std::path::Path;

use serde_json::{json, Value};
use tree_sitter::{Node, Tree};

use crate::analysis::find_usages::{
    build_rows_with_budget, extract_code_with_context, owner_hint, scope_for_node, UsageRow,
    USAGE_HEADER,
};
use crate::analysis::path_utils;
use crate::common::project_files::collect_project_files;
use crate::mcp_types::{CallToolResult, CallToolResultExt};
use crate::parser::{detect_language, Language};

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
    let path_str = arguments["path"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'path' argument",
        )
    })?;
    let context_lines = arguments["context_lines"].as_u64().unwrap_or(3) as u32;
    let max_tokens = arguments["max_tokens"].as_u64().map(|v| v as usize);
    let offset = arguments["offset"].as_u64().unwrap_or(0) as usize;
    let limit = arguments["limit"].as_u64().map(|v| v as usize);

    let path = Path::new(path_str);
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Path does not exist: {path_str}"),
        ));
    }

    let mut usages: Vec<UsageRow> = Vec::new();
    if path.is_file() {
        search_file_writes(path, symbol, context_lines, &mut usages)?;
    } else {
        for file in collect_project_files(path)? {
            if detect_language(&file).is_ok() {
                let _ = search_file_writes(&file, symbol, context_lines, &mut usages);
            }
        }
    }

    usages.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
    });
    for u in &mut usages {
        u.file = path_utils::to_relative_path(&u.file);
    }

    let total = usages.len();
    let paged: Vec<UsageRow> = {
        let it = usages.into_iter().skip(offset);
        match limit {
            Some(n) => it.take(n).collect(),
            None => it.collect(),
        }
    };

    let (rows, truncated) = match max_tokens {
        Some(budget) => build_rows_with_budget(&paged, symbol, USAGE_HEADER, budget, true)?,
        None => build_rows_with_budget(&paged, symbol, USAGE_HEADER, usize::MAX, false)?,
    };

    let mut result = json!({
        "sym": symbol,
        "h": USAGE_HEADER,
        "u": rows,
        "total": total,
        "offset": offset,
    });
    if truncated {
        result["@"] = json!({"t": true});
    }
    let json_text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(json_text))
}

fn search_file_writes(
    path: &Path,
    symbol: &str,
    context_lines: u32,
    out: &mut Vec<UsageRow>,
) -> Result<(), io::Error> {
    let language = detect_language(path).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("Cannot detect language: {e}"),
        )
    })?;
    let (tree, source) = crate::common::cache::cached_tree(path, language).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("read/parse: {e}"))
    })?;
    visit(&tree, &source, symbol, language, path, context_lines, out);
    Ok(())
}

fn visit(
    tree: &Tree,
    source: &str,
    symbol: &str,
    language: Language,
    path: &Path,
    context_lines: u32,
    out: &mut Vec<UsageRow>,
) {
    let root = tree.root_node();
    let mut cursor = root.walk();
    visit_node(&mut cursor, source, symbol, language, path, context_lines, out);
}

fn visit_node(
    cursor: &mut tree_sitter::TreeCursor,
    source: &str,
    symbol: &str,
    language: Language,
    path: &Path,
    context_lines: u32,
    out: &mut Vec<UsageRow>,
) {
    let node = cursor.node();
    if (node.kind() == "identifier" || node.kind().ends_with("_identifier"))
        && node.utf8_text(source.as_bytes()).unwrap_or_default() == symbol
        && is_write_position(node, source, symbol)
    {
        let pos = node.start_position();
        out.push(UsageRow {
            file: path.to_string_lossy().to_string(),
            line: pos.row + 1,
            column: pos.column + 1,
            usage_type: "write".to_string(),
            context: extract_code_with_context(source, pos.row, context_lines),
            scope: scope_for_node(node, source, language),
            confidence: "high".to_string(),
            owner_hint: owner_hint(node, source),
        });
    }
    if cursor.goto_first_child() {
        loop {
            visit_node(cursor, source, symbol, language, path, context_lines, out);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// True when the identifier is the *target* of an assignment/declaration.
fn is_write_position(node: Node, source: &str, symbol: &str) -> bool {
    // Direct parent checks: left/target/name fields.
    let mut current = Some(node);
    // Walk up at most 3 levels: identifier -> attribute/field_expr -> assignment.
    for _ in 0..4 {
        let Some(n) = current else { break };
        let Some(parent) = n.parent() else { break };
        let pk = parent.kind();

        // assignment-like parents
        let is_assign = pk.contains("assignment")
            || pk == "variable_declarator"
            || pk == "lexical_declaration"
            || pk == "let_declaration"
            || pk == "const_item"
            || pk == "static_item"
            || pk == "field_declaration"
            || pk == "for_statement"
            || pk == "for_clause"
            || pk == "for_in_clause";

        if is_assign {
            // If parent has left/target/name/pattern field pointing at our subtree, it's a write.
            for field in ["left", "target", "name", "pattern", "declarator"] {
                if let Some(f) = parent.child_by_field_name(field) {
                    if f.id() == n.id() || is_ancestor(f, node) {
                        return true;
                    }
                }
            }
            // Fallback: first named child is usually the target.
            let mut c = parent.walk();
            if let Some(first) = parent.named_children(&mut c).next() {
                if first.id() == n.id() || is_ancestor(first, node) {
                    return true;
                }
            }
            // `x += 1` / `x++` / `++x` style: operator assignment counts.
            if pk.contains("augmented") || pk.contains("update") || pk.contains("increment") {
                return true;
            }
        }

        // `x++`, `x--`, `++x` update expressions.
        if pk == "update_expression" || pk == "unary_expression" {
            if let Ok(t) = parent.utf8_text(source.as_bytes()) {
                let t = t.trim();
                if t.contains("++") || t.contains("--") {
                    return true;
                }
            }
        }

        // Keep climbing through member/field wrappers: `self.x = ...`,
        // `obj.field = ...` — the identifier is the field, parent is
        // attribute/field_expression, grandparent is assignment.
        if pk == "attribute"
            || pk == "field_expression"
            || pk == "member_expression"
            || pk == "scoped_identifier"
        {
            // Check whether this wrapper is itself the assignment target.
            if let Some(gp) = parent.parent() {
                let gk = gp.kind();
                if gk.contains("assignment") {
                    for field in ["left", "target"] {
                        if let Some(f) = gp.child_by_field_name(field) {
                            if f.id() == parent.id() || is_ancestor(f, node) {
                                return true;
                            }
                        }
                    }
                }
            }
            // Also match bare `symbol =` where tree puts identifier inside wrapper:
            // e.g. python `current_game_id_ = 5` has no wrapper, handled above.
            let _ = symbol;
        }

        current = Some(parent);
    }
    false
}

fn is_ancestor(ancestor: Node, node: Node) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n.id() == ancestor.id() {
            return true;
        }
        cur = n.parent();
    }
    false
}
