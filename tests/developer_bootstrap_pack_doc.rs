use std::fs;
use std::path::{Path, PathBuf};

fn read_doc() -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        base.join("docs").join("developer_bootstrap_pack.md"),
        base.join("docs")
            .join("已实现")
            .join("developer_bootstrap_pack.md"),
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
fn developer_bootstrap_pack_doc_mentions_vscode_day1_contract() {
    let doc = read_doc();
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
