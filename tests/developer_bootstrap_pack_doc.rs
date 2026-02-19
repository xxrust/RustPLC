use std::fs;
use std::path::Path;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

#[test]
fn developer_bootstrap_pack_doc_mentions_vscode_day1_contract() {
    let doc = read_doc("docs/developer_bootstrap_pack.md");
    for needle in [
        "VS Code 支持包契约",
        "*.plc",
        "plc.code-snippets",
        "scenario-doctor",
        "Troubleshooting",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }
}
