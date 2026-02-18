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
fn stepper_ab_encoder_markdown_plc_snippets_parse() {
    let doc = read_doc("docs/stepper_ab_encoder.md");
    let blocks = extract_fenced_blocks(&doc, "plc");
    assert!(
        !blocks.is_empty(),
        "docs/stepper_ab_encoder.md should contain at least one ```plc fenced block"
    );

    for (idx, block) in blocks.iter().enumerate() {
        if let Err(err) = parse_plc(block) {
            panic!(
                "docs/stepper_ab_encoder.md plc block #{idx} failed to parse:\n{err}\n\n--- block ---\n{block}"
            );
        }
    }
}

#[test]
fn stepper_ab_encoder_doc_covers_rule_templates_and_playbook_link() {
    let doc = read_doc("docs/stepper_ab_encoder.md");
    for needle in [
        "规则模板",
        "6.1 单阈值互斥",
        "6.2 区间互斥",
        "6.3 多执行器碰撞矩阵",
        "6.4 双向互锁",
        "常见误区 -> 修正方式",
        "scenario_playbook.md",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }
}
