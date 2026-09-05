# Tree-sitter MCP Server

AST-first MCP for coding agents. Instead of pasting raw files into the
context window, it returns compact structural answers: signatures, usage
rows, focused edit context, impact summaries, and review bundles with
explicit token budgets.

![Token efficiency comparison: MCP vs agent-style shell baselines](docs/token-efficiency-comparison.svg)

File overviews run ~2.7× smaller than `cat`, focused edits ~5× smaller,
repo search ~4.5× smaller than `grep -C3`, call-graph tracing ~57× smaller
than grepping plus reading every hit. Details and methodology:
[BENCHMARK.md](BENCHMARK.md).

## Install

```bash
cargo build --release
```

Homebrew and release binaries:

```bash
brew tap christoph/treesitter-mcp
brew install treesitter-mcp
```

Point your MCP client at the binary (`target/release/treesitter-mcp`).
For Claude Code:

```bash
claude mcp add --scope project treesitter-mcp -- /ABSOLUTE/PATH/TO/treesitter-mcp
```

Or in `.mcp.json`:

```json
{
  "mcpServers": {
    "treesitter-mcp": { "command": "/ABSOLUTE/PATH/TO/treesitter-mcp", "args": [] }
  }
}
```

## Start here

```text
1. session_bootstrap(path="src")          → orientation in one call
2. view_code(detail="signatures")          → file structure, not bodies
3. minimal_edit_context(symbol_name="...") → smallest context for one edit
4. review_context(file_path="...")         → verify after changes
```

## Languages

Rust, Python, JavaScript, TypeScript, C, C++, Go, Java, C#, Swift, HTML, CSS.

## Tools (29)

Understand: `session_bootstrap`, `code_map`, `type_map`, `module_map`,
`view_code`, `batch_view`, `symbol_at_line`, `template_context`.
Find: `find_usages`, `find_writes`, `search_text`, `query_pattern`,
`call_graph`, `call_path`, `depends_on`, `arg_flow`, `relevant_tests`.
Change: `minimal_edit_context`, `apply_symbol_edit`, `rename_preview`,
`preview_impact`, `affected_by_diff`, `parse_diff`, `verify_edit`,
`review_context`. LSP bridges: `format_references`,
`format_diagnostics`. Meta: `plan_context`, `prompt_snippet`.

Full reference: [docs/TOOLS.md](docs/TOOLS.md).
Which tool when: [docs/WORKFLOWS.md](docs/WORKFLOWS.md).

## Notes

- Outputs are compact row schemas with token budgets (`max_tokens`),
  paging (`offset`/`limit`), and cost previews (`estimate=true`).
- Syntax-aware, not compiler-grade: check `conf`/`scope` on renames,
  pair with LSP for precise references.
- Every response carries a one-line `hint` with the best next call.

## Contributing

`cargo fmt`, `cargo test`, `cargo clippy -- -D warnings`.
TDD with fixtures; see [AGENTS.md](AGENTS.md).

## License

MIT
