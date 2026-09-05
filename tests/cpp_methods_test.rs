//! C++ method extraction: Qt-style classes and out-of-line definitions.
//!
//! Regression test: in-class declarators use `field_identifier` and
//! out-of-line `A::f` definitions use `qualified_identifier`. Both must
//! resolve to the plain method name so call indexes contain methods.

use serde_json::{json, Value};
use treesitter_mcp::analysis::shape::extract_enhanced_shape;
use treesitter_mcp::parser::{parse_code, Language};

const QT_STYLE: &str = r#"
namespace ui {
class ModListController : public QObject {
    Q_OBJECT
public:
    explicit ModListController(QObject *parent = nullptr);
    void refresh_plugins_tab();
public slots:
    void onToggle(bool on);
};
void ModListController::refresh_plugins_tab() {
    onToggle(true);
}
}
"#;

fn shape_of(src: &str) -> Value {
    let tree = parse_code(src, Language::Cpp).unwrap();
    let shape = extract_enhanced_shape(&tree, src, Language::Cpp, None, false).unwrap();
    serde_json::to_value(&shape).unwrap()
}

#[test]
fn qt_in_class_methods_extracted() {
    let v = shape_of(QT_STYLE);
    let classes = v["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 1);
    let mut methods: Vec<&str> = classes[0]["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    methods.sort();
    // NOTE: the `explicit ModListController(...)` prototype is swallowed by
    // tree-sitter's error recovery around the Q_OBJECT macro (unparseable as
    // a declarator), so it is intentionally absent here.
    assert_eq!(methods, vec!["onToggle", "refresh_plugins_tab"]);
}

#[test]
fn qualified_out_of_line_definition_extracted() {
    let v = shape_of(QT_STYLE);
    let funcs: Vec<&str> = v["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(
        funcs.contains(&"refresh_plugins_tab"),
        "out-of-line A::f missing, got: {funcs:?}"
    );
}

#[test]
fn call_graph_sees_method_callee() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("m.h"), QT_STYLE).unwrap();
    std::process::Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .ok();
    let header = dir.path().join("m.h").to_string_lossy().to_string();
    let args = json!({
        "file_path": header,
        "symbol_name": "refresh_plugins_tab",
        "direction": "callees",
    });
    let r = treesitter_mcp::analysis::call_graph::execute(&args).unwrap();
    let text = {
        let first = r.content.first().unwrap();
        let s = serde_json::to_string(first).unwrap();
        serde_json::from_str::<Value>(&s).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(
        v["edges"].as_str().unwrap().contains("onToggle"),
        "callee missing: {text}"
    );
}

#[test]
fn call_path_reaches_method() {
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("m.h"), QT_STYLE).unwrap();
    std::process::Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .ok();
    let header = dir.path().join("m.h").to_string_lossy().to_string();
    let args = json!({
        "file_path": header,
        "symbol": "refresh_plugins_tab",
        "to": "onToggle",
    });
    let r = treesitter_mcp::analysis::call_path::execute(&args).unwrap();
    let text = {
        let first = r.content.first().unwrap();
        let s = serde_json::to_string(first).unwrap();
        serde_json::from_str::<Value>(&s).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["reachable"], true);
}

#[test]
fn batch_view_variant_with_nulls() {
    // MCP clients send explicit nulls for absent optionals; the view
    // variant must still dispatch to view_code, not usages.
    let dir = tempfile::TempDir::new().unwrap();
    let f = dir.path().join("a.py");
    std::fs::write(&f, "def foo():\n    pass\n").unwrap();
    let args = json!({
        "items": [{
            "file_path": f.to_str().unwrap(),
            "focus_symbol": "foo",
            "detail": Value::Null,
            "kind": Value::Null,
            "symbol": Value::Null,
            "path": Value::Null,
        }]
    });
    let r = treesitter_mcp::analysis::batch::execute(&args).unwrap();
    let text = {
        let first = r.content.first().unwrap();
        let s = serde_json::to_string(first).unwrap();
        serde_json::from_str::<Value>(&s).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let v: Value = serde_json::from_str(&text).unwrap();
    let items = v["items"].as_object().unwrap();
    assert_eq!(items.len(), 1);
    let key = items.keys().next().unwrap();
    assert!(
        key.contains("foo") && !key.starts_with("usages::"),
        "view item misdispatched, key: {key}"
    );
}
