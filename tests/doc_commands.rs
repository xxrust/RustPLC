use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock works")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn repo_path(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(p)
}

fn run_cli(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rust_plc"))
        .args(args)
        .output()
        .expect("run rust_plc")
}

#[test]
fn examples_index_validates_real_catalog_and_writes_artifact() {
    let out_dir = temp_dir("rust_plc_examples_index");
    let out = out_dir.join("examples_index.json");
    let output = run_cli(&[
        "examples-index".to_string(),
        "--catalog".to_string(),
        repo_path("examples/catalog.toml")
            .to_string_lossy()
            .into_owned(),
        "--root".to_string(),
        repo_path(".").to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "examples-index should pass for real catalog, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists(), "examples-index should write artifact");
    let payload: Value = serde_json::from_slice(&output.stdout).expect("examples-index json");
    assert_eq!(payload["command"], "examples-index");
    assert_eq!(payload["issue_count"], 0);
    assert!(payload["category_count"].as_u64().expect("category count") >= 6);
    assert!(payload["example_count"].as_u64().expect("example count") >= 30);
    assert!(
        payload["categories"]
            .as_array()
            .expect("categories array")
            .iter()
            .any(|category| category["id"] == "02_motion_control")
    );
}

#[test]
fn examples_index_writes_readme_only_category_mirror() {
    let out_dir = temp_dir("rust_plc_examples_mirror");
    let mirror = out_dir.join("by_category");
    let output = run_cli(&[
        "examples-index".to_string(),
        "--catalog".to_string(),
        repo_path("examples/catalog.toml")
            .to_string_lossy()
            .into_owned(),
        "--root".to_string(),
        repo_path(".").to_string_lossy().into_owned(),
        "--mirror-dir".to_string(),
        mirror.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "examples-index mirror should pass for real catalog, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root_readme = mirror.join("README.md");
    let basics_readme = mirror.join("01_basics").join("README.md");
    assert!(root_readme.exists(), "mirror root README should exist");
    assert!(basics_readme.exists(), "category README should exist");
    let basics = fs::read_to_string(&basics_readme).expect("read basics README");
    assert!(
        basics.contains("[`examples/demo.plc`](") && basics.contains("examples/demo.plc)"),
        "category README should link back to source examples, got:\n{basics}"
    );
    assert!(
        !mirror.join("01_basics").join("demo.plc").exists(),
        "mirror must not copy PLC files"
    );
}

#[test]
fn examples_index_reports_missing_catalog_paths() {
    let root = temp_dir("rust_plc_examples_index_missing");
    let examples_dir = root.join("examples");
    fs::create_dir_all(&examples_dir).expect("create examples dir");
    let catalog = examples_dir.join("catalog.toml");
    fs::write(
        &catalog,
        r#"schema_version = 1

[[categories]]
id = "01_basics"
name = "01 Basics"

[[categories.examples]]
id = "missing"
title = "missing"
path = "examples/missing.plc"
kind = "plc"
purpose = "Missing fixture."
"#,
    )
    .expect("write catalog");

    let output = run_cli(&[
        "examples-index".to_string(),
        "--catalog".to_string(),
        catalog.to_string_lossy().into_owned(),
        "--root".to_string(),
        root.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        !output.status.success(),
        "examples-index should fail on missing path"
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("examples-index json");
    assert_eq!(payload["status"], Value::Null);
    assert_eq!(payload["issue_count"], 1);
    assert_eq!(payload["issues"][0]["code"], "EXAMPLES-CATALOG-003");
}

#[test]
fn dsl_capabilities_reports_supported_and_unsupported_contracts() {
    let out_dir = temp_dir("rust_plc_dsl_caps");
    let out = out_dir.join("dsl_capabilities.json");
    let output = run_cli(&[
        "dsl-capabilities".to_string(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "dsl-capabilities should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists(), "dsl-capabilities should write artifact");
    let payload: Value = serde_json::from_slice(&output.stdout).expect("dsl capabilities json");
    assert_eq!(payload["command"], "dsl-capabilities");
    assert_eq!(payload["schema_version"], 1);
    assert!(
        payload["supported_features"]
            .as_array()
            .expect("supported features")
            .iter()
            .any(|feature| {
                feature["id"] == "station_protocols"
                    && feature["layer"] == "semantic_ir_verification"
            })
    );
    assert!(
        payload["template_assets"]
            .as_array()
            .expect("template assets")
            .iter()
            .any(|asset| asset["id"] == "recovery_templates")
    );
    assert!(
        payload["supported_features"]
            .as_array()
            .expect("supported features")
            .iter()
            .any(|feature| feature["id"] == "generic_task_templates"
                && feature["layer"] == "preprocess_semantic_ir")
    );
}

#[test]
fn doc_index_reports_markdown_files_and_writes_artifact() {
    let root = temp_dir("rust_plc_doc_index");
    fs::create_dir_all(root.join("nested")).expect("create nested docs");
    fs::write(
        root.join("alpha.md"),
        "# Alpha\n\nSee [Beta](nested/beta.md#beta-heading).\n",
    )
    .expect("write alpha");
    fs::write(
        root.join("nested").join("beta.md"),
        "# Beta Heading\n\n## Details\n",
    )
    .expect("write beta");
    fs::write(root.join("ignored.txt"), "# Not markdown\n").expect("write ignored");

    let out = root.join("index.json");
    let output = run_cli(&[
        "doc-index".to_string(),
        "--root".to_string(),
        root.to_string_lossy().into_owned(),
        "--out".to_string(),
        out.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "doc-index should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.exists(),
        "doc-index should write the requested artifact"
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("doc-index json");
    assert_eq!(payload["command"], "doc-index");
    assert_eq!(payload["document_count"], 2);
    let paths = payload["documents"]
        .as_array()
        .expect("documents array")
        .iter()
        .map(|doc| doc["path"].as_str().expect("doc path"))
        .collect::<Vec<_>>();
    assert!(paths.contains(&"alpha.md"));
    assert!(paths.contains(&"nested/beta.md"));
}

#[test]
fn doc_lint_accepts_local_markdown_file_and_anchor_links() {
    let root = temp_dir("rust_plc_doc_lint_pass");
    fs::create_dir_all(root.join("nested")).expect("create nested docs");
    fs::write(
        root.join("alpha.md"),
        "# Alpha\n\nSee [Beta](nested/beta.md#beta-heading).\n",
    )
    .expect("write alpha");
    fs::write(root.join("nested").join("beta.md"), "# Beta Heading\n").expect("write beta");

    let output = run_cli(&[
        "doc-lint".to_string(),
        "--root".to_string(),
        root.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "doc-lint should pass, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("doc-lint json");
    assert_eq!(payload["status"], "pass");
    assert_eq!(payload["issue_count"], 0);
    assert_eq!(payload["links_checked"], 1);
}

#[test]
fn doc_lint_reports_missing_local_files_and_anchors() {
    let root = temp_dir("rust_plc_doc_lint_fail");
    fs::create_dir_all(root.join("nested")).expect("create nested docs");
    fs::write(
        root.join("alpha.md"),
        "# Alpha\n\nSee [Missing](missing.md).\nSee [Bad Anchor](nested/beta.md#missing-anchor).\n",
    )
    .expect("write alpha");
    fs::write(root.join("nested").join("beta.md"), "# Beta Heading\n").expect("write beta");

    let output = run_cli(&[
        "doc-lint".to_string(),
        "--root".to_string(),
        root.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        !output.status.success(),
        "doc-lint should fail on broken local links"
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("doc-lint json");
    assert_eq!(payload["status"], "fail");
    assert_eq!(payload["issue_count"], 2);
    let codes = payload["issues"]
        .as_array()
        .expect("issues array")
        .iter()
        .map(|issue| issue["code"].as_str().expect("issue code"))
        .collect::<Vec<_>>();
    assert!(codes.contains(&"DOC-LINK-001"));
    assert!(codes.contains(&"DOC-LINK-002"));
}

#[test]
fn doc_xref_reports_resolved_and_unresolved_local_edges_without_failing() {
    let root = temp_dir("rust_plc_doc_xref");
    fs::create_dir_all(root.join("nested")).expect("create nested docs");
    fs::write(
        root.join("alpha.md"),
        "# Alpha\n\nSee [Beta](nested/beta.md#beta-heading).\nSee [Missing](missing.md).\n",
    )
    .expect("write alpha");
    fs::write(root.join("nested").join("beta.md"), "# Beta Heading\n").expect("write beta");

    let output = run_cli(&[
        "doc-xref".to_string(),
        "--root".to_string(),
        root.to_string_lossy().into_owned(),
        "--output".to_string(),
        "json".to_string(),
    ]);

    assert!(
        output.status.success(),
        "doc-xref should not fail on unresolved edges, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("doc-xref json");
    assert_eq!(payload["command"], "doc-xref");
    assert_eq!(payload["edge_count"], 2);
    assert_eq!(payload["unresolved_count"], 1);
    let statuses = payload["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .map(|edge| edge["status"].as_str().expect("edge status"))
        .collect::<Vec<_>>();
    assert!(statuses.contains(&"resolved"));
    assert!(statuses.contains(&"unresolved"));
}
