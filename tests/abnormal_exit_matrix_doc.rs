use std::fs;
use std::path::Path;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

#[test]
fn abnormal_exit_doc_mentions_matrix_contract_and_verifier_command() {
    let doc = read_doc("docs/abnormal_exit_matrix.md");
    for needle in [
        "Abnormal-Exit Safety Matrix (A/B/C/D)",
        "matrix.json",
        "evidence_schema.json",
        "abnormal_exit_matrix_verify.py",
        "hardware_only",
        "vertical",
        "do2",
        "do1",
        "provenance.source_path",
        "Class-D-Abnormal-Exit-Evidence-Workflow",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }
}
