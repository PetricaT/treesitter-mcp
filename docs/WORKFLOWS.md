# Workflows

How to drive `treesitter-mcp` in an agent loop. Tool parameters live in
[TOOLS.md](TOOLS.md).

## Quick selection

**Understand code:** unknown area → `session_bootstrap` (or `code_map`
minimal). Known file overview → `view_code(signatures)`. One function →
`view_code(focus+isolate)` or `minimal_edit_context`. Line from a stack
trace → `symbol_at_line`.

**Find something:** symbol → `find_usages` (writes only: `find_writes`;
strings/keys/messages: `search_text`). Callers/callees → `call_graph
rank=true`. Transitive proof → `call_path` (calls) / `depends_on`
(includes). Argument origins → `arg_flow`. Tests → `relevant_tests`.

**Change code:** signature change? `preview_impact` first. Editing?
`minimal_edit_context`, then `apply_symbol_edit` (or your own edit),
then `parse_diff` → `affected_by_diff` → `verify_edit`. Reviewing?
`review_context`. Renaming? `rename_preview`, then LSP rename.

**Already have LSP data:** `format_references`, `format_diagnostics`,
or `view_code(definition_location=...)`.

## Cost tiers

| Budget | Pattern |
|---|---|
| <2000 tokens | `signatures`/`minimal`, `focus+isolate`, `estimate=true` first |
| 2000–5000 | defaults, `batch_view` for 2–5 reads |
| >5000 | `full`, `code_map(full)`, `review_context` |

Batch independent reads (`batch_view`), page large results
(`offset`/`limit`), compress paths (`compact_paths=true`).

## Precision vs heuristic

Strong guarantees (AST-exact): `view_code`, `parse_diff`,
`query_pattern`, `symbol_at_line`, `apply_symbol_edit` spans,
`rename_preview` locations.

Best-effort syntax-aware: `find_usages`/`find_writes` (homonyms
possible — check `conf`/`scope`), `call_graph` (same-file preferred),
`affected_by_diff`/`preview_impact` (depend on the above),
`relevant_tests`, `arg_flow` (same-file only), `module_map`
(visibility is heuristic).

For compiler-grade resolution, pair with an LSP server and funnel its
output through `format_references` / `format_diagnostics`.

## Recipes

- New session: `session_bootstrap(path)` → work.
- Explore: `code_map(minimal)` → `view_code(signatures)` → `focus`.
- Refactor: `preview_impact` → edit → `affected_by_diff` → tests.
- Review: `review_context` → drill with `view_code`/`minimal_edit_context`.
- Debug: `symbol_at_line` → `minimal_edit_context` → `find_writes`.

## Anti-patterns

`view_code(full)` for overviews (use `signatures`, 10× cheaper);
`query_pattern` for symbol search (use `find_usages`);
unbounded `find_usages` on hot symbols (set `max_context_lines`
or `compact_paths`); `focus_symbol` without `isolate` on big files;
ignoring `conf=low` on renames.
