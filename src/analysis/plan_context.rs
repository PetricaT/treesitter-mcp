//! Context budget advisor: which calls fit the budget.
//!
//! Agents over-fetch or under-fetch. Given a task description and a
//! token budget, recommend an ordered tool sequence with per-call
//! budgets summing under the total. Heuristic keyword mapping —
//! no LLM planning, stays cheap.

use std::io;

use serde_json::{json, Value};

use crate::mcp_types::{CallToolResult, CallToolResultExt};

struct Step {
    tool: &'static str,
    args: Value,
    budget: usize,
    note: &'static str,
}

/// Args: `task`, `budget` (default 3000), optional `path`, `file_path`, `symbol`.
pub fn execute(arguments: &Value) -> Result<CallToolResult, io::Error> {
    let task = arguments["task"].as_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Missing or invalid 'task' argument",
        )
    })?;
    let budget = arguments["budget"].as_u64().unwrap_or(3000) as usize;
    let path = arguments["path"].as_str().unwrap_or("src");
    let file_path = arguments["file_path"].as_str().unwrap_or("");
    let symbol = arguments["symbol"]
        .as_str()
        .or_else(|| arguments["symbol_name"].as_str())
        .unwrap_or("");

    let t = task.to_lowercase();
    let mut steps: Vec<Step> = Vec::new();

    if t.contains("rename") && !symbol.is_empty() {
        steps.push(Step {
            tool: "rename_preview",
            args: json!({"symbol": symbol, "new_name": "<new>", "path": path}),
            budget: budget * 2 / 5,
            note: "blast radius first; fill new_name",
        });
        steps.push(Step {
            tool: "relevant_tests",
            args: json!({"file_path": file_path, "symbol_name": symbol}),
            budget: budget / 5,
            note: "tests to run after",
        });
    } else if t.contains("test") && !symbol.is_empty() {
        steps.push(Step {
            tool: "relevant_tests",
            args: json!({"file_path": file_path, "symbol_name": symbol}),
            budget: budget / 2,
            note: "targeted tests",
        });
    } else if (t.contains("call") || t.contains("impact") || t.contains("caller"))
        && !symbol.is_empty()
    {
        steps.push(Step {
            tool: "call_graph",
            args: json!({"file_path": file_path, "symbol_name": symbol, "rank": true}),
            budget: budget / 2,
            note: "ranked callers/callees",
        });
    } else if (t.contains("edit") || t.contains("fix") || t.contains("change"))
        && !symbol.is_empty()
        && !file_path.is_empty()
    {
        steps.push(Step {
            tool: "minimal_edit_context",
            args: json!({"file_path": file_path, "symbol_name": symbol}),
            budget: budget * 2 / 5,
            note: "smallest edit context",
        });
        steps.push(Step {
            tool: "verify_edit",
            args: json!({"file_path": file_path, "target_symbol": symbol}),
            budget: budget / 5,
            note: "after editing",
        });
    } else if t.contains("string")
        || t.contains("key")
        || t.contains("literal")
        || t.contains("error message")
    {
        steps.push(Step {
            tool: "search_text",
            args: json!({"pattern": symbol, "path": path, "estimate": true}),
            budget: 200,
            note: "estimate first, then real search",
        });
    } else if t.contains("type") {
        steps.push(Step {
            tool: "type_map",
            args: json!({"path": path, "count_usages": false}),
            budget: budget / 2,
            note: "type locations, no usage counting",
        });
    } else if t.contains("review") || t.contains("diff") {
        steps.push(Step {
            tool: "review_context",
            args: json!({"file_path": file_path}),
            budget: budget * 3 / 5,
            note: "diff + impact + tests bundle",
        });
    } else {
        steps.push(Step {
            tool: "session_bootstrap",
            args: json!({"path": path}),
            budget: budget * 3 / 5,
            note: "orientation in one call",
        });
    }

    let estimated: usize = steps.iter().map(|s| s.budget).sum();
    let fits = estimated <= budget;
    let steps_json: Vec<Value> = steps
        .iter()
        .map(|s| {
            json!({
                "tool": s.tool,
                "args": s.args,
                "budget": s.budget,
                "note": s.note,
            })
        })
        .collect();

    let result = json!({
        "task": task,
        "budget": budget,
        "steps": steps_json,
        "estimated_tokens": estimated,
        "fits": fits,
        "note": if fits { "sequence fits budget" } else { "over budget; raise budget or page with offset/limit" },
    });
    let text = serde_json::to_string(&result).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("serialize: {e}"))
    })?;
    Ok(CallToolResult::success(text))
}
