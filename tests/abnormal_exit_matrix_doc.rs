use std::fs;
use std::path::{Path, PathBuf};

fn read_doc() -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        base.join("docs").join("abnormal_exit_matrix.md"),
        base.join("docs")
            .join("已实现")
            .join("abnormal_exit_matrix.md"),
    ];
    read_first_existing_doc(&candidates)
}

fn read_first_existing_doc(candidates: &[PathBuf]) -> String {
    for path in candidates {
        if path.exists() {
            return fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read doc failed {}: {e}", path.display()));
        }
    }
    panic!(
        "read doc failed: none of the expected files exist: {}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn abnormal_exit_doc_mentions_matrix_contract_and_verifier_command() {
    let doc = read_doc();
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
