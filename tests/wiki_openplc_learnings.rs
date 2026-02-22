use std::fs;
use std::path::Path;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

#[test]
fn openplc_learnings_wiki_mentions_delivered_capabilities() {
    let doc = read_doc("docs/wiki/OpenPLC-v3-Learnings-Integration.md");
    for needle in [
        "board-parse",
        "tick_timing.jsonl",
        "forces",
        "io-map-normalize",
        "IEC",
        "HAL",
        "timing-report",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }
}
