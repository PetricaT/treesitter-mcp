//! Accuracy harness: task-success over token-savings.
//!
//! Token multipliers don't prove edits succeed. Each test poses a
//! realistic agent task against a TempDir fixture and asserts the
//! end-to-end answer (recall + precision), not just row shape.
//! Run: `cargo test --test accuracy_harness -- --nocapture`
//! CSV rows print as `TASK,result,detail` for `benches/accuracy/results/`.

use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

fn text(r: &treesitter_mcp::mcp_types::CallToolResult) -> String {
    let first = r.content.first().expect("no content");
    let s = serde_json::to_string(first).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    v["text"].as_str().unwrap().to_string()
}

fn emit(task: &str, ok: bool, detail: &str) {
    println!("TASK,{},{},{}", task, if ok { "pass" } else { "fail" }, detail);
}

fn write(dir: &TempDir, name: &str, content: &str) -> String {
    let p = dir.path().join(name);
    fs::write(&p, content).unwrap();
    p.to_string_lossy().to_string()
}

/// Rename `add`: must find def + both call sites, and scope must
/// distinguish the method `Calc::add` from the free function.
#[test]
fn task_rename_recall_with_homonyms() {
    let dir = TempDir::new().unwrap();
    write(
        &dir,
        "m.py",
        "def add(a, b):\n    return a + b\nclass Calc:\n    def add(self, a, b):\n        return a + b\nx = add(1, 2)\nc = Calc()\ny = c.add(3, 4)\n",
    );
    let root = dir.path().to_str().unwrap();
    let args = json!({"symbol": "add", "path": root, "context_lines": 0});
    let r = treesitter_mcp::analysis::find_usages::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    let rows = v["u"].as_str().unwrap();
    let has_scope = rows.contains("Calc");
    let count = rows.lines().count();
    let ok = count >= 4 && has_scope;
    emit("rename_recall", ok, &format!("rows={count} scope={has_scope}"));
    assert!(ok, "expected def+calls with scope, got:\n{rows}");
}

/// String-key trace: all readers of knowledge key `save_parser_ids`.
#[test]
fn task_string_key_trace() {
    let dir = TempDir::new().unwrap();
    write(
        &dir,
        "a.py",
        "IDS = get(game_id, \"save_parser_ids\")\n",
    );
    write(
        &dir,
        "b.py",
        "x = store.get(\"save_parser_ids\")\n# save_parser_ids documented\n",
    );
    let root = dir.path().to_str().unwrap();
    let args = json!({"pattern": "save_parser_ids", "path": root});
    let r = treesitter_mcp::analysis::search_text::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    let total = v["total"].as_u64().unwrap();
    let ok = total == 3;
    emit("string_key_trace", ok, &format!("total={total}"));
    assert!(ok);
}

/// State-flow: where is `current_game_id_` assigned (not merely read)?
#[test]
fn task_write_trace() {
    let dir = TempDir::new().unwrap();
    let f = write(
        &dir,
        "s.py",
        "current_game_id_ = None\ndef load(g):\n    global current_game_id_\n    current_game_id_ = g\nprint(current_game_id_)\n",
    );
    let args = json!({"symbol": "current_game_id_", "path": f});
    let r = treesitter_mcp::analysis::find_writes::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    let rows = v["u"].as_str().unwrap();
    let ok = v["total"].as_u64().unwrap() >= 2 && rows.lines().all(|l| l.contains("write"));
    emit("write_trace", ok, &format!("total={}", v["total"]));
    assert!(ok);
}

/// Arg dataflow: `run(ctl)` ← `ctl = db` ← `db = connect()`.
#[test]
fn task_arg_flow_chain() {
    let dir = TempDir::new().unwrap();
    let f = write(
        &dir,
        "a.py",
        "def f():\n    db = connect()\n    ctl = db\n    run(ctl)\n",
    );
    let args = json!({"file_path": f, "line": 4, "depth": 3});
    let r = treesitter_mcp::analysis::arg_flow::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    let flows = v["flows"].as_str().unwrap();
    let ok = flows.contains("assign:1") && flows.contains("assign:2");
    emit("arg_flow_chain", ok, flows);
    assert!(ok);
}

/// Include cycle guard: a.c → b.h must report reachable with chain.
#[test]
fn task_include_cycle_guard() {
    let dir = TempDir::new().unwrap();
    let b = write(&dir, "b.h", "int x;\n");
    let bname = std::path::Path::new(&b)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let a = write(&dir, "a.c", &format!("#include \"{bname}\"\nint y;\n"));
    let args = json!({"from": a, "to": b});
    let r = treesitter_mcp::analysis::depends::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&text(&r)).unwrap();
    let ok = v["reachable"] == true && !v["chain"].as_str().unwrap().is_empty();
    emit("include_guard", ok, v["chain"].as_str().unwrap());
    assert!(ok);
}
