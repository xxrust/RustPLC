use std::fs;
use std::path::{Path, PathBuf};

fn repo_path(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
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
fn diagnostics_backend_methodology_doc_keeps_required_sections() {
    let doc = read_first_existing_doc(&[
        repo_path("docs/diagnostics_backend_methodology.md"),
        repo_path("docs/已实现/diagnostics_backend_methodology.md"),
    ]);

    for needle in [
        "规则图",
        "评分策略",
        "alarm_event",
        "evidence_source",
        "evidence_inputs",
        "AI 使用边界",
        "不参与是否放行的硬判定",
        "示例 A",
        "示例 B",
        "输出格式变更说明",
    ] {
        assert!(doc.contains(needle), "doc should contain `{needle}`");
    }
}
