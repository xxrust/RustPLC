use std::fs;
use std::path::{Path, PathBuf};

use rust_plc::parser::parse_plc;

fn read_stepper_doc() -> String {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        base.join("docs").join("stepper_ab_encoder.md"),
        base.join("docs")
            .join("已实现")
            .join("stepper_ab_encoder.md"),
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
    let doc = read_stepper_doc();
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
    let doc = read_stepper_doc();
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

#[test]
fn stepper_ab_encoder_doc_covers_scope_boundaries_and_non_goals() {
    let doc = read_stepper_doc();
    for needle in [
        "本期边界与非目标",
        "实时脉冲轨迹规划",
        "复杂运动学在线求解",
        "原始 AB 边沿在 DSL 直接解码",
        "DSL 负责顺控、互锁、安全约束",
        "驱动/板级层负责高速计算",
        "隐含新增能力",
    ] {
        assert!(doc.contains(needle), "doc should mention `{needle}`");
    }
}
