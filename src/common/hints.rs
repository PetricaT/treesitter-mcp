//! Next-step hints: one cheap line steering the follow-up call.
//!
//! ~10-20 tokens per response. Each builder takes the counts the tool
//! already computed and suggests the highest-leverage next call.

/// Hint after a search with total hits (possibly truncated).
pub fn search_hint(total: usize, truncated: bool, symbol: &str) -> String {
    if total == 0 {
        "no hits; try search_text for string literals or regex=true".to_string()
    } else if truncated {
        format!("{total} hits truncated; re-call with offset to page")
    } else if total == 1 {
        format!("single hit; view_code isolate {symbol} for edit context")
    } else if total > 20 {
        format!("{total} hits; narrow path/scope or batch_view top callers")
    } else {
        format!("callers span hits; minimal_edit_context on {symbol} for focused edit")
    }
}

/// Hint after a call graph.
pub fn graph_hint(callers: usize, callees: usize, cycles: usize) -> String {
    if cycles > 0 {
        "cycle detected; call_path self-query shows the loop".to_string()
    } else if callers > 5 {
        format!("{callers} callers; rank=true prioritizes by freq/hints")
    } else if callees == 0 && callers == 0 {
        "no edges; confirm symbol with symbol_at_line (overload/homonym?)".to_string()
    } else {
        "depth=1 shown; depth=2 for transitive, call_path for A→B proof".to_string()
    }
}

/// Hint after viewing code.
pub fn view_hint(has_focus: bool, truncated: bool) -> String {
    if truncated {
        "truncated; isolate one symbol or raise max_tokens".to_string()
    } else if has_focus {
        "focused; minimal_edit_context adds callees/types/imports".to_string()
    } else {
        "overview; view_code isolate or batch_view for 2-3 symbols".to_string()
    }
}

/// Hint after a directory map.
pub fn map_hint(files: usize, truncated: bool) -> String {
    if truncated {
        format!("{files} files truncated; offset/limit to page or pattern to filter")
    } else {
        "map done; batch_view entry points, type_map for key types".to_string()
    }
}

/// Hint after an edit-related response.
pub fn edit_hint() -> String {
    "verify with parse_diff then affected_by_diff before tests".to_string()
}
