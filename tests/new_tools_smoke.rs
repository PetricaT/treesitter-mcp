use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

fn result_text(r: &treesitter_mcp::mcp_types::CallToolResult) -> String {
    let first = r.content.first().expect("no content");
    let s = serde_json::to_string(first).unwrap();
    let v: Value = serde_json::from_str(&s).unwrap();
    v["text"].as_str().unwrap().to_string()
}

fn write_tmp(dir: &TempDir, name: &str, content: &str) -> String {
    let p = dir.path().join(name);
    fs::write(&p, content).unwrap();
    p.to_string_lossy().to_string()
}

#[test]
fn search_text_finds_string_literal() {
    let dir = TempDir::new().unwrap();
    write_tmp(&dir, "a.rs", "fn m() {\n  let k = get(game_id, \"save_parser_ids\");\n}\n");
    let args = json!({"pattern": "save_parser_ids", "path": dir.path().to_str().unwrap()});
    let r = treesitter_mcp::analysis::search_text::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert_eq!(v["total"], 1);
    assert!(v["m"].as_str().unwrap().contains("save_parser_ids"));
}

#[test]
fn search_text_multi_patterns_lint() {
    let dir = TempDir::new().unwrap();
    write_tmp(&dir, "a.rs", "foo — bar\nTODO fix\n");
    let args = json!({"patterns": ["—", "TODO"], "path": dir.path().to_str().unwrap()});
    let r = treesitter_mcp::analysis::search_text::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert_eq!(v["total"], 2);
}

#[test]
fn find_writes_filters_assignments() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(
        &dir,
        "s.py",
        "x = 1\ny = x + 1\nx = 3\nprint(x)\n",
    );
    let args = json!({"symbol": "x", "path": f});
    let r = treesitter_mcp::analysis::find_writes::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert!(v["total"].as_u64().unwrap() >= 2);
    for row in v["u"].as_str().unwrap().lines() {
        assert!(row.contains("write"));
    }
}

#[test]
fn view_code_isolate_returns_only_target() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(
        &dir,
        "c.rs",
        "pub fn alpha() {}\npub fn beta() {}\npub fn gamma() {}\n",
    );
    let args = json!({"file_path": f, "detail": "full", "focus_symbol": "beta", "isolate": true, "include_deps": false});
    let r = treesitter_mcp::analysis::view_code::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    let f_rows = v.get("f").and_then(|x| x.as_str()).unwrap_or("");
    assert!(f_rows.contains("beta"));
    assert!(!f_rows.contains("alpha"));
    assert!(!f_rows.contains("gamma"));
}

#[test]
fn batch_view_fetches_two_symbols() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(&dir, "c.rs", "pub fn alpha() {}\npub fn beta() {}\n");
    let args = json!({"items": [{"file_path": f, "focus_symbol": "alpha"}, {"file_path": f, "focus_symbol": "beta"}]});
    let r = treesitter_mcp::analysis::batch::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert_eq!(v["items"].as_object().unwrap().len(), 2);
}

#[test]
fn batch_view_mixes_view_and_usages() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(&dir, "c.rs", "pub fn alpha() {}\npub fn beta() { alpha(); }\n");
    let root = dir.path().to_str().unwrap().to_string();
    let args = json!({"items": [
        {"file_path": f, "focus_symbol": "alpha"},
        {"kind": "usages", "symbol": "alpha", "path": root},
    ]});
    let r = treesitter_mcp::analysis::batch::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    let items = v["items"].as_object().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.keys().any(|k| k.starts_with("usages::alpha@")));
}

#[test]
fn depends_on_traces_chain() {
    let dir = TempDir::new().unwrap();
    let b = write_tmp(&dir, "b.h", "int x;\n");
    let b_name = std::path::Path::new(&b).file_name().unwrap().to_string_lossy().to_string();
    let a = write_tmp(&dir, "a.c", &format!("#include \"{b_name}\"\nint y;\n"));
    let args = json!({"from": a, "to": b});
    let r = treesitter_mcp::analysis::depends::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert_eq!(v["reachable"], true);
}

#[test]
fn arg_flow_single_hop() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(
        &dir,
        "a.py",
        "def f():\n    cfg = load()\n    run(cfg)\n",
    );
    let args = json!({"file_path": f, "line": 3});
    let r = treesitter_mcp::analysis::arg_flow::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert!(v["flows"].as_str().unwrap().contains("cfg"));
}

#[test]
fn arg_flow_multi_hop_chain() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(
        &dir,
        "a.py",
        "def f():\n    db = connect()\n    ctl = db\n    run(ctl)\n",
    );
    let args = json!({"file_path": f, "line": 4, "depth": 3});
    let r = treesitter_mcp::analysis::arg_flow::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    let flows = v["flows"].as_str().unwrap();
    assert!(flows.contains("assign:1"));
    assert!(flows.contains("assign:2"));
}

#[test]
fn call_graph_rank_adds_hints() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(
        &dir,
        "a.py",
        "def target():\n    pass\ndef caller():\n    for i in range(3):\n        target()\n",
    );
    // init git for project root discovery
    std::process::Command::new("git").arg("init").current_dir(dir.path()).output().ok();
    let args = json!({"file_path": f, "symbol_name": "target", "direction": "callers", "rank": true});
    let r = treesitter_mcp::analysis::call_graph::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert!(v["h"].as_str().unwrap().contains("freq"));
}

#[test]
fn code_map_paging_meta() {
    let dir = TempDir::new().unwrap();
    write_tmp(&dir, "a.py", "def fa():\n    pass\n");
    write_tmp(&dir, "b.py", "def fb():\n    pass\n");
    let args = json!({"path": dir.path().to_str().unwrap(), "limit": 1, "offset": 0});
    let r = treesitter_mcp::analysis::code_map::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert_eq!(v["@"]["total"], 2);
    assert_eq!(v["@"]["offset"], 0);
    // only one file key besides @/types/hint
    let file_keys: Vec<_> = v.as_object().unwrap().keys().filter(|k| *k != "@" && *k != "types" && *k != "hint").collect();
    assert_eq!(file_keys.len(), 1);
}

#[test]
fn estimate_preview_search_and_usages() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(&dir, "a.py", "x = 1\nprint(x)\n# hello\n");
    let args = json!({"pattern": "x", "path": dir.path().to_str().unwrap(), "estimate": true});
    let r = treesitter_mcp::analysis::search_text::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert!(v["estimated_rows"].as_u64().unwrap() >= 1);
    assert!(v.get("m").is_none());

    let args = json!({"symbol": "x", "path": f, "estimate": true});
    let r = treesitter_mcp::analysis::find_usages::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert!(v["estimated_rows"].as_u64().unwrap() >= 1);
}

#[test]
fn apply_symbol_edit_dry_run() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(&dir, "a.py", "def foo():\n    return 1\n");
    let args = json!({"file_path": f, "symbol_name": "foo", "new_body": "def foo():\n    return 2", "dry_run": true});
    let r = treesitter_mcp::analysis::apply_edit::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert_eq!(v["sym"], "foo");
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["parses"], true);
    // file untouched in dry run
    assert!(fs::read_to_string(dir.path().join("a.py")).unwrap().contains("return 1"));
}

#[test]
fn call_path_and_cycles() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(
        &dir,
        "a.py",
        "def a():\n    b()\ndef b():\n    c()\ndef c():\n    pass\ndef rec(n):\n    return rec(n - 1)\n",
    );
    std::process::Command::new("git").arg("init").current_dir(dir.path()).output().ok();
    let args = json!({"file_path": f, "symbol": "a", "to": "c"});
    let r = treesitter_mcp::analysis::call_path::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert_eq!(v["reachable"], true);
    assert!(v["chain"].as_str().unwrap().contains("b"));

    let args = json!({"file_path": f, "symbol_name": "rec", "direction": "callees"});
    let r = treesitter_mcp::analysis::call_graph::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert!(v["cycles"].as_str().unwrap().contains("rec -> rec"));
}

#[test]
fn find_usages_paging_total_offset() {
    let dir = TempDir::new().unwrap();
    let f = write_tmp(&dir, "a.py", "x = 1\nprint(x)\nprint(x)\n");
    let args = json!({"symbol": "x", "path": f, "limit": 1, "offset": 0});
    let r = treesitter_mcp::analysis::find_usages::execute(&args).unwrap();
    let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
    assert!(v["total"].as_u64().unwrap() >= 2);
    assert_eq!(v["offset"], 0);
    assert_eq!(v["u"].as_str().unwrap().lines().count(), 1);
}
