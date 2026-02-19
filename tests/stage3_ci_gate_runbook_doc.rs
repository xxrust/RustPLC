use std::fs;
use std::path::Path;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

#[test]
fn stage3_ci_gate_runbook_mentions_script_contract_preflight() {
    let doc = read_doc("docs/stage3_ci_gate_runbook.md");

    for needle in [
        "script mode/EOL preflight",
        "scripts/ci_script_contract_preflight.sh",
        "executable mode (`100755`)",
        "CRLF",
        "LF",
        "scripts/stage3_runtime_dev_gate.sh",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }
}
