use std::fs;
use std::path::Path;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

#[test]
fn hil_regression_doc_mentions_timing_gate_verdict_and_tuning_flow() {
    let doc = read_doc("docs/hil_regression.md");

    for needle in [
        "timing_gate_verdict.json",
        "--max-p99-exec-us",
        "--max-overrun-count",
        "阈值调优建议",
        "exec_us_p99",
        "overrun_count",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }
}
