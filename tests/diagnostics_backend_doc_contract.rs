use std::fs;
use std::path::Path;

fn repo_path(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

#[test]
fn diagnostics_backend_methodology_doc_keeps_required_sections() {
    let doc = fs::read_to_string(repo_path("docs/diagnostics_backend_methodology.md"))
        .expect("read diagnostics_backend_methodology.md");

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
