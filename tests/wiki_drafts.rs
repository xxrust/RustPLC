use std::fs;
use std::path::Path;

use rust_plc::parser::parse_plc;

fn read_doc(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read doc failed {rel}: {e}"))
}

fn extract_fenced_blocks(markdown: &str, fence_lang: &str) -> Vec<String> {
    let fence_open = format!("```{fence_lang}");
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut buf = String::new();

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if !in_block {
            if trimmed.starts_with(&fence_open) {
                in_block = true;
                buf.clear();
            }
            continue;
        }

        if trimmed.starts_with("```") {
            in_block = false;
            let block = buf.trim().to_string();
            if !block.is_empty() {
                blocks.push(block);
            }
            continue;
        }

        buf.push_str(line);
        buf.push('\n');
    }

    assert!(!in_block, "unclosed fenced code block for `{fence_lang}`");
    blocks
}

#[test]
fn wiki_draft_stepper_ab_encoder_contains_parseable_plc_and_key_terms() {
    let doc = read_doc("docs/wiki/Stepper-AB-Encoder-Safety-Modeling.md");
    for needle in [
        "docs/stepper_ab_encoder.md",
        "zone_code",
        "Bi-directional Interlock",
    ] {
        assert!(doc.contains(needle), "wiki draft should mention `{needle}`");
    }

    let blocks = extract_fenced_blocks(&doc, "plc");
    assert!(
        !blocks.is_empty(),
        "docs/wiki/Stepper-AB-Encoder-Safety-Modeling.md should contain at least one ```plc fenced block"
    );
    for (idx, block) in blocks.iter().enumerate() {
        if let Err(err) = parse_plc(block) {
            panic!(
                "docs/wiki/Stepper-AB-Encoder-Safety-Modeling.md plc block #{idx} failed to parse:\n{err}\n\n--- block ---\n{block}"
            );
        }
    }
}

#[test]
fn wiki_draft_topology_abstraction_contains_parseable_plc_and_key_terms() {
    let doc = read_doc("docs/wiki/Topology-Abstraction-PLS-Angle-Distance.md");
    for needle in [
        "docs/stepper_ab_encoder.md",
        "Primary + Derived",
        "pos_consistent",
    ] {
        assert!(doc.contains(needle), "wiki draft should mention `{needle}`");
    }

    let blocks = extract_fenced_blocks(&doc, "plc");
    assert!(
        !blocks.is_empty(),
        "docs/wiki/Topology-Abstraction-PLS-Angle-Distance.md should contain at least one ```plc fenced block"
    );
    for (idx, block) in blocks.iter().enumerate() {
        if let Err(err) = parse_plc(block) {
            panic!(
                "docs/wiki/Topology-Abstraction-PLS-Angle-Distance.md plc block #{idx} failed to parse:\n{err}\n\n--- block ---\n{block}"
            );
        }
    }
}
