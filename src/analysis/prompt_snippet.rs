//! Copy-paste agent prompt snippet (~300 tokens).
//!
//! The README Quick Selection Guide is too long for a system prompt.
//! This returns a short decision-rules fragment pasteable into an
//! agent's instructions.

/// The snippet text. Keep under ~350 tokens (tiktoken) — test enforces.
pub const SNIPPET: &str = "treesitter-mcp rules: explore with session_bootstrap or code_map minimal, never cat. Overview: view_code detail=signatures. Edit one symbol: minimal_edit_context (or view_code focus+isolate). Find callers: call_graph rank=true; all refs: find_usages; writes only: find_writes; strings/keys/errors: search_text. Transitive: call_path (calls), depends_on (includes), arg_flow (arg values). Batch 2-5 reads via batch_view. Check estimate=true before big calls; page with offset/limit. After edits: parse_diff then affected_by_diff, apply_symbol_edit for splices. Precision is syntax-aware (conf/low homonyms); use LSP format_references for renames.";

pub fn snippet() -> &'static str {
    SNIPPET
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_stays_short() {
        let words = SNIPPET.split_whitespace().count();
        assert!(words <= 200, "snippet grew to {words} words");
        assert!(SNIPPET.contains("minimal_edit_context"));
        assert!(SNIPPET.contains("search_text"));
    }
}
