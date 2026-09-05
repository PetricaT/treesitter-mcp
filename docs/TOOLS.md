# Tool Reference

Full reference for every `treesitter-mcp` tool. Rows are newline-delimited,
fields pipe-delimited and escaped (`\` → `\\`, newline → `\n`, `|` → `\|`).
Truncation is signalled by `@.t=true`; most list tools accept
`offset`/`limit` (paged via `total`) and `max_tokens`.

For *which tool to use when*, see [WORKFLOWS.md](WORKFLOWS.md).

## Understand code

### session_bootstrap
One-call orientation: top types, minimal code map, entry points
(`main`/`lib`/`index`), test dirs. Start new sessions here.
In: `path`, `max_tokens` (default 3000). Out: `types`, `map`,
`entry_points`, `test_dirs`.

### code_map
Directory structure overview. Detail `minimal` (names),
`signatures` (default), `full` (code). Filters: `pattern` glob,
`with_types`/`count_usages` to fold `type_map` in. Paging via
`offset`/`limit` (`@.total`/`@.offset`); `diff_aware` limits to
git-touched files; `estimate=true` previews cost.
Out: per-file `{h, f, s, c}`, optional `types`.

### type_map
Usage-ranked project types (structs, classes, enums, interfaces,
traits, aliases). In: `path`, `max_tokens`, `pattern`,
`count_usages` (false = locations only, faster). Out: `h`,
`types` (`name|kind|file|line|usage_count`).

### module_map
One row per file: `module|exports|file` — what each module shares.
Use before calling across module boundaries.

### view_code
Single file. Detail `minimal` (names), `signatures`, `full` (default).
`focus_symbol` keeps code for one symbol; `isolate=true` drops
everything else. `diff_aware` auto-focuses changed symbols.
`comment_mode="leading"` prepends doc comments.
`definition_location` narrows deps from an LSP definition.
Auto-includes referenced project types as `deps` (AST-derived).

### batch_view
Multiple reads in one call: view items
`{file_path, focus_symbol?, detail?}` (isolate by default) plus usages
items `{kind:"usages", symbol, path}`. Out: `items` keyed
`file::symbol` / `usages::symbol@path`.

### symbol_at_line
What symbol owns `file:line:col`? Returns name, kind, signature,
scope chain. Start from stack traces and error lines.

### template_context
Askama templates: which Rust structs/fields are available as
template variables. In: `template_path`.

## Find things

### find_usages
Syntax-aware symbol search: `file|line|col|type|context|scope|conf|owner`.
`type` is definition/call/type_reference/import/reference.
`conf` is high/medium/low (homonym signal). `compact_paths=true`
emits a `files` id map with `fid` rows (big repos).
`estimate=true` previews cost without the payload.

### find_writes
Same schema as `find_usages`, writes only (`type=write`).
Answers "where does this member get *set*?"

### search_text
Literal/regex search for non-symbols: string literals, config keys,
error messages, macros. Single `pattern` or multi-pattern `patterns`
(lint mode: `pattern|file|line|col|context`). `regex`,
`case_sensitive`, `context_lines`, `estimate=true` supported.

### query_pattern
Raw tree-sitter S-expressions for structural patterns only
`find_usages`/`search_text` can't express. Advanced use.

### call_graph
Callers + callees, depth 1–3: `direction|symbol|file|line|scope|depth`.
`rank=true` adds `freq|hints|conf` (loop/signal/ctor/thread hints,
same-file high). `cycles` reports self/2-cycles. `call_path`
proves A→B transitively.

### call_path
Does A transitively call B? BFS to depth 5, returns `chain`
(`symbol@file:line` rows). Self-path detects recursion.

### depends_on
Does A transitively include/import B? Returns `chain`.
Check before adding `#include`/imports to avoid cycles.

### arg_flow
What flows into a call argument? Bounded same-file walk
(default depth 3, max 5): call row plus `assign:N` chain rows.

### relevant_tests
Which tests exercise a symbol? Rows
`test_file|test_fn|line|relevance` (direct/indirect/same_module).

## Change code

### minimal_edit_context
Smallest useful edit context for one symbol: target code, callee
signatures, referenced types, relevant imports. `with_conf` adds
confidence. Prefer over `view_code(focus)` for large files.

### apply_symbol_edit
Splice `new_body` over a symbol's AST span. Verifies the file still
parses. `dry_run=true` previews (`replaced_lines`, `parses`).

### rename_preview
Dry-run rename: `file|line|col|old_text|new_text|confidence`
plus `files_modified`/`total_edits`. Imports skipped (separate edit).

### preview_impact
Blast radius of a *planned* signature change, before editing.
Virtual diff + affected call sites with risk levels.

### affected_by_diff
What breaks after your edit? `parse_diff` + `find_usages` with
`risk` (high/medium/low). `with_conf` appends match confidence.
Scope with `scope` param.

### parse_diff
Symbol-level diff vs git revision (added/removed/sig_changed),
not line-level. Cheap change verification.

### verify_edit
Guardrail: did the edit stay within the intended symbol?
Rows `check|status|detail`.

### review_context
Review bundle for a changed file: diff + affected + tests +
focused context per changed symbol.

### type_map / code_map
See "Understand code" — both feed review/edit flows.

## Format LSP output

### format_references
Compact `find_usages`-schema rows (`conf=high`) around precise LSP
`textDocument/references` locations. Bridge: LSP precision, MCP size.

### format_diagnostics
Compact `severity|file|line|col|owner|source|code|message` rows from
LSP diagnostics, with structural owners.

## Meta

### plan_context
Budget advisor: given `task` + `budget`, recommends an ordered tool
sequence with per-call budgets and a `fits` flag.

### prompt_snippet
Pasteable ~150-word system-prompt fragment with tool-choice rules.

## Conventions

- `estimate=true` on heavy tools (`search_text`, `find_usages`,
  `code_map`, `type_map`, `call_graph`) returns
  `estimated_tokens`/`estimated_rows`/`scope_summary` only.
- Every tool response carries a one-line `hint` with the best next call.
- Errors self-correct: missing paths get did-you-mean suggestions,
  unknown symbols get candidate lists.
