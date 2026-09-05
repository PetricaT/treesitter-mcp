//! MCP Tool definitions and implementations
//!
//! This module defines all the tools provided by the treesitter-mcp server
//! using the rust-mcp-sdk macros and conventions.

use rust_mcp_sdk::macros::{mcp_tool, JsonSchema};
use rust_mcp_sdk::schema::{schema_utils::CallToolError, CallToolResult};
use rust_mcp_sdk::tool_box;

use crate::analysis::{
    apply_edit, arg_flow, batch, bootstrap, call_graph, call_path, code_map, depends, diff,
    find_usages, find_writes, format_diagnostics, format_references, minimal_edit_context,
    module_map, prompt_snippet, query_pattern, relevant_tests, rename, review_context,
    search_text, symbol_at_line, verify_edit, view_code,
};

// Helper function for serde default
fn default_full() -> String {
    "full".to_string()
}

fn default_one() -> Option<u32> {
    Some(1)
}

/// View a source file with flexible detail levels and automatic type inclusion
#[mcp_tool(
    name = "view_code",
    description = "View file in compact schema (BREAKING). Output keys: `p` (relative path), `h` (header for f/s/c rows), `f` (functions rows), `s` (structs rows), `c` (classes rows), optional deps `deps` (map dep_path -> type rows), plus optional tables: imports `ih`+`im`, trait methods `th`+`tm`, interfaces `ah`+`i`, properties `ph`+`pr`, class implements `ch`+`ci`, class methods `mh`+`cm`, Rust impl methods `bh`+`bm`. Rows are newline-delimited; fields are pipe-delimited and escaped: `\\` -> `\\\\`, `\n` -> `\\n`, `\r` -> `\\r`, `|` -> `\\|`. Meta: `@.t=true` when truncated. DETAIL: 'signatures' (name/line/sig), 'full' (adds doc/code). COMMENTS: `comment_mode=\"leading\"` prepends the contiguous leading comment block to returned code fields. FOCUS: set focus_symbol to keep code only for that symbol. LSP: pass definition_location from textDocument/definition to include the exact dependency type."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct ViewCode {
    /// Path to the source file
    pub file_path: String,

    /// Detail level: "signatures" or "full" (default: "full")
    /// - "signatures": Function/class signatures only (no bodies)
    /// - "full": Complete implementation code
    #[serde(default = "default_full")]
    pub detail: String,

    /// Optional: Focus on ONE symbol, show full code only for it
    /// When set, returns full code for this symbol + signatures for rest
    #[serde(default)]
    pub focus_symbol: Option<String>,

    /// Optional: isolate=true returns ONLY the focus symbol rows.
    /// Use when focus_symbol still returns the whole file.
    #[serde(default)]
    pub isolate: Option<bool>,

    /// Optional LSP or compact definition location for exact dependency type selection.
    #[serde(default)]
    pub definition_location: Option<ReferenceLocation>,

    /// Optional comment handling for returned code fields.
    /// - "none" (default): keep current compact behavior
    /// - "leading": prepend the contiguous leading comment block above returned symbols
    #[serde(default)]
    pub comment_mode: Option<String>,
}

/// Generate a high-level code map of a directory with token budget awareness and detail levels
#[mcp_tool(
    name = "code_map",
    description = "Generate hierarchical map of a DIRECTORY (not single file). Returns structure overview of multiple files with functions/classes/types. Detail levels: 'minimal' (names only), 'signatures' (DEFAULT, names + signatures), 'full' (includes code). USE WHEN: ✅ First time exploring unfamiliar codebase ✅ Finding where functionality lives across files ✅ Getting project structure overview ✅ Don't know which file to examine. DON'T USE: ❌ Know specific file → use view_code ❌ Need implementation details → use view_code after identifying files. TOKEN COST: MEDIUM (scales with project size). OPTIMIZATION: Start with detail='minimal' for large projects, use pattern to filter. WORKFLOW: code_map → view_code. COMBINED MODE: Set with_types=true to also extract type definitions (structs, enums, interfaces, etc.) in the same pass - more efficient than calling type_map separately."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct CodeMap {
    /// Path to file or directory
    pub path: String,
    /// Maximum tokens for output (approximate, default: 2000)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Detail level: "minimal", "signatures", or "full" (default: "signatures")
    #[serde(default)]
    pub detail: Option<String>,
    /// Glob pattern to filter files (e.g., "*.rs")
    #[serde(default)]
    pub pattern: Option<String>,
    /// Also extract type definitions (structs, enums, interfaces, etc.) in the same pass.
    /// More efficient than calling type_map separately. Output includes a "types" key.
    #[serde(default)]
    pub with_types: Option<bool>,
    /// When with_types=true, also count usages for each type (default: false for performance).
    #[serde(default)]
    pub count_usages: Option<bool>,
    /// Paging offset over files (uses `@.total`/`@.offset`).
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit over files.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Cost preview only.
    #[serde(default)]
    pub estimate: Option<bool>,
}

/// Find all usages of a symbol with context and usage type classification
#[mcp_tool(
    name = "find_usages",
    description = "Find ALL usages of a symbol (function, variable, class, type) across files. Syntax-aware search, not text search. Returns file locations, code context, usage type (definition, call, type_reference, import, reference). USE WHEN: ✅ Refactoring: see all places that call a function ✅ Impact analysis: checking what breaks if you change signature ✅ Tracing data flow ✅ Before renaming/modifying shared code. DON'T USE: ❌ Need structural changes only → use parse_diff ❌ Want risk assessment → use affected_by_diff ❌ Symbol used >50 places → use affected_by_diff or set max_context_lines=50. TOKEN COST: MEDIUM-HIGH (scales with usage count × context_lines). OPTIMIZATION: Set max_context_lines=50 for frequent symbols, context_lines=1 for locations only. WORKFLOW: find_usages (before changes) → make changes → affected_by_diff (verify)"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct FindUsages {
    /// Symbol name to search for
    pub symbol: String,
    /// File or directory path to search in
    pub path: String,
    /// Number of context lines around each usage (default: 3)
    #[serde(default)]
    pub context_lines: Option<u32>,
    /// Maximum total context lines across ALL usages (prevents token explosion)
    /// When set, limits the total number of context lines returned
    #[serde(default)]
    pub max_context_lines: Option<u32>,
    /// Maximum tokens for output (tiktoken counted). When set, output is
    /// truncated by dropping code/context and/or truncating usages.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Paging offset over total matches (default 0).
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit over total matches.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Cost preview only: return estimated_tokens/rows/scope without payload.
    #[serde(default)]
    pub estimate: Option<bool>,
}

#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct LspPosition {
    /// 0-based LSP line
    pub line: u32,
    /// 0-based LSP character
    pub character: u32,
}

#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct LspRange {
    /// LSP range start; end is ignored by this tool
    pub start: LspPosition,
}

#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct ReferenceLocation {
    /// Source file path. Use this or `uri`.
    #[serde(default)]
    pub file: Option<String>,
    /// Alternative source file path field accepted by the analysis module.
    #[serde(default)]
    pub file_path: Option<String>,
    /// LSP file URI, e.g. file:///repo/src/lib.rs. Use this or `file`.
    #[serde(default)]
    pub uri: Option<String>,
    /// 1-based line for compact non-LSP locations.
    #[serde(default)]
    pub line: Option<u32>,
    /// 1-based column for compact non-LSP locations.
    #[serde(default)]
    pub col: Option<u32>,
    /// 1-based column alias.
    #[serde(default)]
    pub column: Option<u32>,
    /// LSP 0-based range. When provided, line/col are ignored.
    #[serde(default)]
    pub range: Option<LspRange>,
}

/// Format precise LSP reference locations into the compact find_usages schema
#[mcp_tool(
    name = "format_references",
    description = "Format LSP-provided reference locations into the same compact schema as find_usages. Input accepts `symbol` plus `references` rows using either 1-based `{file,line,col}` / `{file_path,line,column}` or LSP `{uri,range:{start:{line,character}}}`. Output keys: `sym`, `h`, `u`; rows are `file|line|col|type|context|scope|conf|owner` with `conf=high` because locations are assumed to come from precise LSP resolution. USE WHEN: ✅ You already called LSP textDocument/references and need compact context for an LLM ✅ You want scope/context around precise references without rerunning syntax-aware search. DON'T USE: ❌ You need MCP to discover references itself → use find_usages."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct FormatReferences {
    /// Symbol name these LSP locations resolve to
    pub symbol: String,
    /// LSP or compact reference locations
    pub references: Vec<ReferenceLocation>,
    /// Number of context lines around each reference (default: 3)
    #[serde(default)]
    pub context_lines: Option<u32>,
    /// Maximum tokens for output (tiktoken counted)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Paging offset
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct DiagnosticItem {
    /// Source file path. Use this or `uri`.
    #[serde(default)]
    pub file: Option<String>,
    /// Alternative source file path field accepted by the analysis module.
    #[serde(default)]
    pub file_path: Option<String>,
    /// LSP file URI, e.g. file:///repo/src/lib.rs. Use this or `file`.
    #[serde(default)]
    pub uri: Option<String>,
    /// 1-based line for compact non-LSP locations.
    #[serde(default)]
    pub line: Option<u32>,
    /// 1-based column for compact non-LSP locations.
    #[serde(default)]
    pub col: Option<u32>,
    /// 1-based column alias.
    #[serde(default)]
    pub column: Option<u32>,
    /// LSP 0-based range. When provided, line/col are ignored.
    #[serde(default)]
    pub range: Option<LspRange>,
    /// LSP diagnostic severity: 1 error, 2 warning, 3 info, 4 hint.
    #[serde(default)]
    pub severity: Option<u32>,
    /// Diagnostic message.
    pub message: String,
    /// Diagnostic source, such as rustc or typescript.
    #[serde(default)]
    pub source: Option<String>,
    /// Diagnostic code as a compact string/number.
    #[serde(default)]
    pub code: Option<String>,
}

/// Format LSP diagnostics into compact rows with structural owners
#[mcp_tool(
    name = "format_diagnostics",
    description = "Format LSP-provided diagnostics into compact rows with structural owner context. Input accepts `diagnostics` rows using either 1-based `{file,line,col}` / `{file_path,line,column}` or LSP `{uri,range:{start:{line,character}}}` plus `severity`, `message`, optional `source`, and optional `code`. Output keys: `h`, `d`; rows are `severity|file|line|col|owner|source|code|message`. USE WHEN: ✅ You already have LSP diagnostics and need a token-efficient grouped summary ✅ You want to know which function/class owns each diagnostic. DON'T USE: ❌ You need to run diagnostics itself → use LSP/compiler/test tools."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct FormatDiagnostics {
    /// LSP or compact diagnostics
    pub diagnostics: Vec<DiagnosticItem>,
    /// Maximum tokens for output (tiktoken counted)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Paging offset
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Return compact context needed to edit one symbol
#[mcp_tool(
    name = "minimal_edit_context",
    description = "Return focused edit context for one symbol. Output keys include `p`, `sym`, `scope`, `h`, `target`, optional `dh`+`deps`, optional `tyh`+`types`, optional `ih`+`imports`, and optional `@.t=true` when truncated. USE WHEN: ✅ Editing one known function/method and need the smallest useful context ✅ Avoiding full-file reads for large files. DON'T USE: ❌ Exploring an unfamiliar file → use view_code or code_map first. COMMENTS: `comment_mode=\"leading\"` prepends the contiguous leading comment block to the target code row. Current scope: same-file deps/types/imports plus direct project-local dependency signatures from imports."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct MinimalEditContext {
    /// Path to the source file
    pub file_path: String,
    /// Symbol name to edit
    pub symbol_name: String,
    /// Maximum tokens for output (default: 2000)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Optional comment handling for the target code row.
    /// - "none" (default): keep current compact behavior
    /// - "leading": prepend the contiguous leading comment block above the target symbol
    #[serde(default)]
    pub comment_mode: Option<String>,
    /// Append conf column to deps/types rows (same-file high, project-local medium)
    #[serde(default)]
    pub with_conf: Option<bool>,
}

/// Return compact callers/callees for one symbol
#[mcp_tool(
    name = "call_graph",
    description = "Return a compact best-effort call graph for one function or method. Output keys: `sym`, `h`, `edges`, `cycles`, `total`, `offset`; rows are `direction|symbol|file|line|scope|depth` where direction is `caller` or `callee`. With rank=true rows add `freq|hints|conf` (same-file high, freq>=2 medium, else low). USE WHEN: ✅ You need to know what calls a symbol and what it calls ✅ You want depth=1 impact/navigation context without manual multi-file reads. DON'T USE: ❌ You need compiler-grade name resolution across imports/generics/traits → use LSP references/definitions when available. TOKEN COST: LOW-MEDIUM. Current resolution is syntax-aware and project-local, with same-file definitions preferred."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct CallGraph {
    /// Path to the source file containing the symbol
    pub file_path: String,
    /// Function or method name to analyze
    pub symbol_name: String,
    /// Direction: "callers", "callees", or "both" (default: "both")
    #[serde(default)]
    pub direction: Option<String>,
    /// Traversal depth (default: 1, max: 3)
    #[serde(default)]
    pub depth: Option<u32>,
    /// Maximum tokens for output (default: 2000)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Rank callers by freq desc with freq|hints columns (loop/signal/ctor/thread).
    #[serde(default)]
    pub rank: Option<bool>,
    /// Paging offset over total edges.
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit over total edges.
    #[serde(default)]
    pub limit: Option<u32>,
    /// Cost preview only.
    #[serde(default)]
    pub estimate: Option<bool>,
}

/// Get symbol information at a specific line with signature and scope chain
#[mcp_tool(
    name = "symbol_at_line",
    description = "Get symbol (function/class/method) at specific line with signature and scope chain. Returns symbol name, signature, kind, and enclosing scopes from innermost to outermost. USE WHEN: ✅ Have line number from error/stack trace ✅ Need to know 'what function is this line in?' ✅ Want function signature at a location ✅ Understanding scope hierarchy. DON'T USE: ❌ Need full code → use view_code with focus_symbol ❌ Know symbol name already → use view_code directly. TOKEN COST: LOW. WORKFLOW: symbol_at_line (find symbol) → view_code (see code)"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct SymbolAtLine {
    /// Path to the source file
    pub file_path: String,

    /// Line number (1-indexed)
    pub line: u32,

    /// Column number (1-indexed, default: 1)
    #[serde(default = "default_one")]
    pub column: Option<u32>,
}

/// Analyze structural changes in a file compared to a git revision
#[mcp_tool(
    name = "parse_diff",
    description = "Analyze structural changes vs git revision. Returns symbol-level diff (functions/classes added/removed/modified), not line-level. USE WHEN: ✅ Verifying what you changed at structural level ✅ Checking if changes are cosmetic (formatting) or substantive ✅ Understanding changes without re-reading entire file ✅ Generating change summaries. DON'T USE: ❌ Need to see what might break → use affected_by_diff ❌ Haven't made changes yet → use view_code ❌ Need line-by-line diff → use git diff. TOKEN COST: LOW-MEDIUM (much smaller than re-reading file). WORKFLOW: After changes: parse_diff (verify) → affected_by_diff (check impact)"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct ParseDiff {
    /// Path to the source file to analyze
    pub file_path: String,
    /// Git revision to compare against (default: "HEAD")
    /// Examples: "HEAD", "HEAD~1", "main", "abc123"
    #[serde(default)]
    pub compare_to: Option<String>,
}

/// Find usages that might be affected by changes in a file
#[mcp_tool(
    name = "affected_by_diff",
    description = "Find usages AFFECTED by your changes. Combines parse_diff + find_usages to show blast radius with risk levels (HIGH/MEDIUM/LOW) based on change type. USE WHEN: ✅ After modifying function signatures - what might break? ✅ Before running tests - anticipate failures ✅ During refactoring - understand impact radius ✅ Risk assessment for code changes. DON'T USE: ❌ Haven't made changes yet → use find_usages first ❌ Just want to see what changed → use parse_diff ❌ Changes are purely internal (no signature changes) → parse_diff is enough. TOKEN COST: MEDIUM-HIGH (combines parse_diff + find_usages). OPTIMIZATION: Use scope parameter to limit search area. WORKFLOW: parse_diff (see changes) → affected_by_diff (assess impact) → fix issues"
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct AffectedByDiff {
    /// Path to the changed source file
    pub file_path: String,
    /// Git revision to compare against (default: "HEAD")
    #[serde(default)]
    pub compare_to: Option<String>,
    /// Directory to search for affected usages (default: project root)
    #[serde(default)]
    pub scope: Option<String>,
    /// Append conf column (high/medium/low) to affected rows
    #[serde(default)]
    pub with_conf: Option<bool>,
}

/// Preview downstream impact from a planned signature change
#[mcp_tool(
    name = "preview_impact",
    description = "Preview downstream blast radius for a planned signature change before editing the file. Input accepts `file_path`, `symbol_name`, and `new_signature`; optional `scope` limits the search area. Output keys: `p`, `sym`, `before`, `after`, `dh`, `d`, `h`, `affected`; detail rows are `kind|name|from|to` and affected rows reuse `symbol|change|file|line|risk`. USE WHEN: ✅ You want to estimate call-site fallout before changing a function signature ✅ You are comparing alternative signatures and want the least disruptive option."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct PreviewImpact {
    /// Path to the source file containing the symbol
    pub file_path: String,
    /// Function or method name to analyze
    pub symbol_name: String,
    /// Planned replacement signature
    pub new_signature: String,
    /// Optional directory to search for affected usages
    #[serde(default)]
    pub scope: Option<String>,
}

/// Execute a custom tree-sitter query pattern on a source file with code context
#[mcp_tool(
    name = "query_pattern",
    description = "Execute custom tree-sitter S-expression query for advanced AST pattern matching. Returns matches with code context for complex structural patterns. USE WHEN: ✅ Finding all instances of specific syntax pattern (e.g., all if statements) ✅ Complex structural queries (e.g., all async functions with try-catch) ✅ Language-specific patterns find_usages can't handle ✅ You know tree-sitter query syntax. DON'T USE: ❌ Finding function/variable usages → use find_usages (simpler, cross-language) ❌ Don't know tree-sitter syntax → use find_usages or view_code ❌ Simple symbol search → use find_usages. TOKEN COST: MEDIUM (depends on matches). COMPLEXITY: HIGH - requires tree-sitter query knowledge. RECOMMENDATION: Prefer find_usages for 90% of use cases."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct QueryPattern {
    /// Path to the source file
    pub file_path: String,
    /// Tree-sitter query pattern in S-expression format
    pub query: String,
    /// Number of context lines around each match (default: 2)
    #[serde(default)]
    pub context_lines: Option<u32>,
    /// Paging offset
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Identify likely relevant tests for one symbol
#[mcp_tool(
    name = "relevant_tests",
    description = "Identify test files and test functions most likely to exercise a symbol. Output keys: `sym`, `h`, `tests`; rows are `test_file|test_fn|line|relevance` where relevance is `direct`, `indirect`, or `same_module`. USE WHEN: ✅ After changing a symbol and deciding what tests to run ✅ Narrowing test execution before reading large test output."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct RelevantTests {
    /// Path to the source file containing the symbol
    pub file_path: String,
    /// Symbol name to analyze
    pub symbol_name: String,
    /// Paging offset
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Verify that an edit stayed within the intended structural scope
#[mcp_tool(
    name = "verify_edit",
    description = "Verify that an edit touched the intended symbol and avoided extra structural changes. Output keys: `p`, `cmp`, `ok`, `h`, `checks`; rows are `check|status|detail`. USE WHEN: ✅ After editing one symbol and wanting a compact regression guard ✅ Before committing or moving on to broader follow-up work."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct VerifyEdit {
    /// Path to the changed source file
    pub file_path: String,
    /// Git revision to compare against (default: "HEAD")
    #[serde(default)]
    pub compare_to: Option<String>,
    /// Optional symbol expected to be changed
    #[serde(default)]
    pub target_symbol: Option<String>,
}

/// Build compact review context for a changed file
#[mcp_tool(
    name = "review_context",
    description = "Assemble compact review context for a changed file by combining structural diff, affected usages, relevant tests, and focused edit context for changed symbols. Output keys: `p`, `cmp`, `ch`, `changes`, `ah`, `affected`, `th`, `tests`, `ctx`; `ctx` maps changed symbols to nested minimal_edit_context payloads. USE WHEN: ✅ Preparing for code review ✅ Gathering high-signal context around a local change without reading whole files."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct ReviewContext {
    /// Path to the changed source file
    pub file_path: String,
    /// Git revision to compare against (default: "HEAD")
    #[serde(default)]
    pub compare_to: Option<String>,
    /// Optional directory to search for affected usages
    #[serde(default)]
    pub scope: Option<String>,
    /// Maximum tokens for output (default: 2000)
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

// Implement tool execution logic for each tool
impl ViewCode {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "detail": self.detail,
            "focus_symbol": self.focus_symbol,
            "isolate": self.isolate,
            "definition_location": self.definition_location
        });

        view_code::execute(&args).map_err(CallToolError::new)
    }
}

impl CodeMap {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "path": self.path,
            "max_tokens": self.max_tokens.unwrap_or(2000),
            "detail": self.detail,
            "pattern": self.pattern,
            "with_types": self.with_types.unwrap_or(false),
            "count_usages": self.count_usages.unwrap_or(false),
            "offset": self.offset,
            "limit": self.limit,
            "estimate": self.estimate
        });

        code_map::execute(&args).map_err(CallToolError::new)
    }
}

impl FindUsages {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "symbol": self.symbol,
            "path": self.path,
            "context_lines": self.context_lines,
            "max_context_lines": self.max_context_lines,
            "max_tokens": self.max_tokens,
            "offset": self.offset,
            "limit": self.limit,
            "estimate": self.estimate
        });

        find_usages::execute(&args).map_err(CallToolError::new)
    }
}

impl FormatReferences {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "symbol": self.symbol,
            "references": self.references,
            "context_lines": self.context_lines,
            "max_tokens": self.max_tokens,
            "offset": self.offset,
            "limit": self.limit
        });

        format_references::execute(&args).map_err(CallToolError::new)
    }
}

impl FormatDiagnostics {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "diagnostics": self.diagnostics,
            "max_tokens": self.max_tokens,
            "offset": self.offset,
            "limit": self.limit
        });

        format_diagnostics::execute(&args).map_err(CallToolError::new)
    }
}

impl MinimalEditContext {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "symbol_name": self.symbol_name,
            "max_tokens": self.max_tokens,
            "with_conf": self.with_conf
        });

        minimal_edit_context::execute(&args).map_err(CallToolError::new)
    }
}

impl CallGraph {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "symbol_name": self.symbol_name,
            "direction": self.direction,
            "depth": self.depth,
            "max_tokens": self.max_tokens,
            "rank": self.rank,
            "offset": self.offset,
            "limit": self.limit,
            "estimate": self.estimate
        });

        call_graph::execute(&args).map_err(CallToolError::new)
    }
}

impl SymbolAtLine {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "line": self.line,
            "column": self.column
        });

        symbol_at_line::execute(&args).map_err(CallToolError::new)
    }
}

impl ParseDiff {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "compare_to": self.compare_to
        });

        diff::execute_parse_diff(&args).map_err(CallToolError::new)
    }
}

impl AffectedByDiff {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "compare_to": self.compare_to,
            "scope": self.scope,
            "with_conf": self.with_conf
        });

        diff::execute_affected_by_diff(&args).map_err(CallToolError::new)
    }
}

impl PreviewImpact {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "symbol_name": self.symbol_name,
            "new_signature": self.new_signature,
            "scope": self.scope
        });

        diff::execute_preview_impact(&args).map_err(CallToolError::new)
    }
}

impl QueryPattern {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "query": self.query,
            "context_lines": self.context_lines,
            "offset": self.offset,
            "limit": self.limit
        });

        query_pattern::execute(&args).map_err(CallToolError::new)
    }
}

impl RelevantTests {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "symbol_name": self.symbol_name,
            "offset": self.offset,
            "limit": self.limit
        });

        relevant_tests::execute(&args).map_err(CallToolError::new)
    }
}

impl VerifyEdit {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "compare_to": self.compare_to,
            "target_symbol": self.target_symbol
        });

        verify_edit::execute(&args).map_err(CallToolError::new)
    }
}

impl ReviewContext {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "compare_to": self.compare_to,
            "scope": self.scope,
            "max_tokens": self.max_tokens
        });

        review_context::execute(&args).map_err(CallToolError::new)
    }
}

/// Find Rust structs that provide context for an Askama template.
///
/// USE WHEN:
/// ✅ Editing Askama HTML templates and need to know available variables
/// ✅ Understanding what data is passed to a template
/// ✅ Debugging template rendering issues
///
/// DON'T USE:
/// ❌ Not using Askama templates
/// ❌ Working with non-template files
///
/// RETURNS:
/// - Struct names associated with the template
/// - All fields with their types (resolved up to 3 levels deep)
/// - Nested struct field expansions
///
/// TOKEN COST: LOW-MEDIUM
/// WORKFLOW: template_context → edit template with known variables
#[mcp_tool(
    name = "template_context",
    description = "Find Askama template context in compact schema (BREAKING). Output keys: `tpl` (relative template path), `h` (header), `ctx` (rows: struct|field|type), `sh` (header), `s` (rows: struct|file|line). Rows are newline-delimited; fields are pipe-delimited and escaped: `\\` -> `\\\\`, `\n` -> `\\n`, `\r` -> `\\r`, `|` -> `\\|`."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct TemplateContext {
    /// Path to the template file (relative or absolute)
    pub template_path: String,
}

impl TemplateContext {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "template_path": self.template_path
        });

        crate::analysis::askama::execute(&args).map_err(CallToolError::new)
    }
}

/// Generate a usage-sorted map of all project types. Returns structs, classes, enums, interfaces, traits, protocols, and type aliases prioritized by usage frequency.
#[mcp_tool(
    name = "type_map",
    description = "Generate a usage-sorted map of project types in compact schema (BREAKING). Output keys: `h` (header) and `types` (rows: name|kind|file|line|usage_count). Optional meta under `@` (e.g. `@.t=true` when truncated). Rows are newline-delimited; fields are pipe-delimited and escaped: `\\` -> `\\\\`, `\n` -> `\\n`, `\r` -> `\\r`, `|` -> `\\|`. PERFORMANCE: Set count_usages=false to skip usage counting for faster results when you only need type locations."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct TypeMap {
    /// Directory path to scan for types
    pub path: String,
    /// Maximum tokens in output (counted via tiktoken, default: 2000)
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Optional glob pattern to filter files (e.g., '*.rs', 'src/**/*.ts')
    #[serde(default)]
    pub pattern: Option<String>,
    /// Whether to count usages across the project (default: true).
    /// Set to false for faster results when you only need type locations.
    #[serde(default)]
    pub count_usages: Option<bool>,
    /// Paging limit (uses `@.total`/`@.offset`).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Paging offset.
    #[serde(default)]
    pub offset: Option<u32>,
    /// Cost preview only.
    #[serde(default)]
    pub estimate: Option<bool>,
}

impl TypeMap {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "path": self.path,
            "max_tokens": self.max_tokens.unwrap_or(2000),
            "pattern": self.pattern,
            "count_usages": self.count_usages.unwrap_or(true),
            "limit": self.limit,
            "offset": self.offset,
            "estimate": self.estimate
        });

        crate::analysis::type_map::execute(&args)
            .map_err(|e| CallToolError::new(std::io::Error::other(e.to_string())))
    }
}

/// Text/pattern search with compact schema. Finds string literals, hook keys,
/// error messages, macro names — anything find_usages cannot see.
#[mcp_tool(
    name = "search_text",
    description = "Search literal substring or regex across files. Output keys: `pat`, `h` (`file|line|col|context` or `pattern|file|line|col|context` for multi), `m`, `total`, `offset`, optional `@.t`. USE WHEN: string literals, config keys, error messages, macro names, lint scans. Pass `patterns[]` for one-pass lint."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct SearchText {
    /// File or directory to search
    pub path: String,
    /// Single literal/regex pattern
    #[serde(default)]
    pub pattern: Option<String>,
    /// Multiple patterns (lint mode)
    #[serde(default)]
    pub patterns: Option<Vec<String>>,
    /// Treat pattern as regex (default false)
    #[serde(default)]
    pub regex: Option<bool>,
    /// Case sensitive (default true)
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    /// Context lines around match (default 0)
    #[serde(default)]
    pub context_lines: Option<u32>,
    /// Paging offset (default 0)
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
    /// Max tokens budget
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Cost preview only
    #[serde(default)]
    pub estimate: Option<bool>,
}

impl SearchText {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let mut args = serde_json::json!({
            "path": self.path,
            "regex": self.regex,
            "case_sensitive": self.case_sensitive,
            "context_lines": self.context_lines,
            "offset": self.offset,
            "limit": self.limit,
            "max_tokens": self.max_tokens,
            "estimate": self.estimate
        });
        if let Some(p) = &self.pattern {
            args["pattern"] = serde_json::json!(p);
        }
        if let Some(ps) = &self.patterns {
            args["patterns"] = serde_json::json!(ps);
        }
        search_text::execute(&args).map_err(CallToolError::new)
    }
}

/// Find where a symbol gets assigned (writes only).
#[mcp_tool(
    name = "find_writes",
    description = "Find assignment/write sites of a symbol. Same schema as find_usages plus `total`/`offset`, but `type=write`. USE WHEN: tracing state flow, 'where does this member get set?'."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct FindWrites {
    /// Symbol name
    pub symbol: String,
    /// File or directory to search
    pub path: String,
    /// Context lines (default 3)
    #[serde(default)]
    pub context_lines: Option<u32>,
    /// Max tokens budget
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Paging offset
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
}

impl FindWrites {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "symbol": self.symbol,
            "path": self.path,
            "context_lines": self.context_lines,
            "max_tokens": self.max_tokens,
            "offset": self.offset,
            "limit": self.limit
        });
        find_writes::execute(&args).map_err(CallToolError::new)
    }
}

/// Batch fetch multiple files/symbols/usages in one call.
#[mcp_tool(
    name = "batch_view",
    description = "Fetch multiple requests in one call: view {file_path, focus_symbol?, detail?} or usages {kind:\"usages\", symbol, path}. Returns `items` map keyed `file::symbol` / `usages::symbol@path` with nested payloads. Batch view defaults isolate=true."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct BatchView {
    /// Items to fetch
    pub items: Vec<BatchItem>,
    /// Shared max tokens (default 4000)
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct BatchItem {
    /// File path (view items)
    #[serde(default)]
    pub file_path: Option<String>,
    /// Optional focus symbol (view items)
    #[serde(default)]
    pub focus_symbol: Option<String>,
    /// Optional detail (view items)
    #[serde(default)]
    pub detail: Option<String>,
    /// Item kind: omit for view, "usages" for find_usages
    #[serde(default)]
    pub kind: Option<String>,
    /// Symbol name (usages items)
    #[serde(default)]
    pub symbol: Option<String>,
    /// Search path (usages items)
    #[serde(default)]
    pub path: Option<String>,
}

impl BatchView {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "items": self.items,
            "max_tokens": self.max_tokens
        });
        batch::execute(&args).map_err(CallToolError::new)
    }
}

/// Check transitive include/import reachability.
#[mcp_tool(
    name = "depends_on",
    description = "Check whether `from` file transitively includes/imports `to` file. Returns `reachable` plus `chain` for cycle review before adding includes."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct DependsOn {
    /// Source file
    pub from: String,
    /// Target file
    pub to: String,
}

impl DependsOn {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "from": self.from,
            "to": self.to
        });
        depends::execute(&args).map_err(CallToolError::new)
    }
}

/// Splice a replacement body over one symbol's AST span.
#[mcp_tool(
    name = "apply_symbol_edit",
    description = "Replace one symbol's code by splicing new_body over its AST line span. Verifies the file still parses. Pass dry_run=true to preview without writing. Returns replaced_lines and parses flag."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct ApplySymbolEdit {
    /// File path
    pub file_path: String,
    /// Symbol to replace
    pub symbol_name: String,
    /// Replacement code block
    pub new_body: String,
    /// Preview without writing (default false)
    #[serde(default)]
    pub dry_run: Option<bool>,
}

impl ApplySymbolEdit {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "symbol_name": self.symbol_name,
            "new_body": self.new_body,
            "dry_run": self.dry_run
        });
        apply_edit::execute(&args).map_err(CallToolError::new)
    }
}

/// Agent system-prompt fragment: when to reach for which tool.
#[mcp_tool(
    name = "prompt_snippet",
    description = "Return a short pasteable system-prompt fragment with tool-choice decision rules (~150 words). No input needed."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct PromptSnippet {}

impl PromptSnippet {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        Ok(CallToolResult::text_content(vec![
            rust_mcp_sdk::schema::TextContent::from(prompt_snippet::snippet().to_string()),
        ]))
    }
}

/// Rename dry-run: preview edits without touching files.
#[mcp_tool(
    name = "rename_preview",
    description = "Preview a rename across the project without editing. Output `h` (file|line|col|old_text|new_text|confidence), `edits`, `files_modified`, `total_edits`. Confidence reuses find_usages scope signal."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct RenamePreview {
    /// Symbol to rename
    pub symbol: String,
    /// Replacement identifier
    pub new_name: String,
    /// File or directory to search
    pub path: String,
    /// Paging offset
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
    /// Max tokens budget
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl RenamePreview {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "symbol": self.symbol,
            "new_name": self.new_name,
            "path": self.path,
            "offset": self.offset,
            "limit": self.limit,
            "max_tokens": self.max_tokens
        });
        rename::execute(&args).map_err(CallToolError::new)
    }
}

/// Module map: one row per file with its exported symbols.
#[mcp_tool(
    name = "module_map",
    description = "Detect module boundaries: one row per file (`module|exports|file`) with top-level symbols. USE WHEN: judging what is internal vs shared before calling across modules."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct ModuleMap {
    /// File or directory to scan
    pub path: String,
    /// Max tokens budget
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Paging offset
    #[serde(default)]
    pub offset: Option<u32>,
    /// Paging limit
    #[serde(default)]
    pub limit: Option<u32>,
}

impl ModuleMap {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "path": self.path,
            "max_tokens": self.max_tokens,
            "offset": self.offset,
            "limit": self.limit
        });
        module_map::execute(&args).map_err(CallToolError::new)
    }
}

/// Session bootstrap: types + minimal map + entries + tests in one call.
#[mcp_tool(
    name = "session_bootstrap",
    description = "Orient a new session in one call: top types, minimal code map, likely entry points (main/lib/index), test dirs. Returns `types`, `map`, `entry_points`, `test_dirs`."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct SessionBootstrap {
    /// Directory to scan
    pub path: String,
    /// Shared budget (default 3000)
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

impl SessionBootstrap {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "path": self.path,
            "max_tokens": self.max_tokens
        });
        bootstrap::execute(&args).map_err(CallToolError::new)
    }
}

/// Call path: does A transitively call B (cycles via self-path).
#[mcp_tool(
    name = "call_path",
    description = "Check whether symbol A transitively calls symbol B (BFS over project-local callee edges, depth max 5). Self-path (to==from) reports recursion cycles. Returns `reachable` plus `chain` (symbol@file:line rows)."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct CallPath {
    /// File containing the source symbol
    pub file_path: String,
    /// Source symbol
    pub symbol: String,
    /// Destination symbol
    pub to: String,
    /// Max depth (default 5, max 5)
    #[serde(default)]
    pub depth: Option<u32>,
}

impl CallPath {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "symbol": self.symbol,
            "to": self.to,
            "depth": self.depth
        });
        call_path::execute(&args).map_err(CallToolError::new)
    }
}

/// Transitive argument dataflow (bounded, same-file).
#[mcp_tool(
    name = "arg_flow",
    description = "What flows into this call's argument? Bounded transitive walk same-file (default depth 3, max 5): call row plus assignment chain with kind assign:N. Output `call`, `arg`, `h`, `flows`."
)]
#[derive(Debug, ::serde::Deserialize, ::serde::Serialize, JsonSchema)]
pub struct ArgFlow {
    /// File with call site
    pub file_path: String,
    /// 1-based line of call
    pub line: u32,
    /// 0-based arg index (default 0)
    #[serde(default)]
    pub arg: Option<u32>,
    /// Optional call name filter
    #[serde(default)]
    pub symbol: Option<String>,
    /// Hop depth (default 3, max 5)
    #[serde(default)]
    pub depth: Option<u32>,
}

impl ArgFlow {
    pub fn call_tool(&self) -> Result<CallToolResult, CallToolError> {
        let args = serde_json::json!({
            "file_path": self.file_path,
            "line": self.line,
            "arg": self.arg,
            "symbol": self.symbol,
            "depth": self.depth
        });
        arg_flow::execute(&args).map_err(CallToolError::new)
    }
}

// Generate an enum with all tools
tool_box!(
    TreesitterTools,
    [
        ViewCode,
        CodeMap,
        FindUsages,
        FormatReferences,
        FormatDiagnostics,
        MinimalEditContext,
        CallGraph,
        SymbolAtLine,
        ParseDiff,
        AffectedByDiff,
        PreviewImpact,
        QueryPattern,
        RelevantTests,
        VerifyEdit,
        ReviewContext,
        TemplateContext,
        TypeMap,
        SearchText,
        FindWrites,
        BatchView,
        DependsOn,
        ArgFlow,
        CallPath,
        ApplySymbolEdit,
        SessionBootstrap,
        PromptSnippet,
        RenamePreview,
        ModuleMap
    ]
);
