use crate::cli_support::common::{
    CliOutputMode, DispatchResult, display_path_relative_to_cwd, write_json_pretty,
};
use crate::cli_support::help::command_usage;
use rust_plc::dsl_capabilities::build_dsl_capabilities_report;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(super) fn try_dispatch(
    program: &str,
    command: &str,
    remaining: &[String],
) -> Option<DispatchResult> {
    let (error_prefix, result) = match command {
        "doc-index" => (
            Some("[DOC-INDEX-000]"),
            run_doc_index_subcommand(program, remaining.iter().cloned()),
        ),
        "doc-lint" => (
            Some("[DOC-LINT-000]"),
            run_doc_lint_subcommand(program, remaining.iter().cloned()),
        ),
        "doc-xref" => (
            Some("[DOC-XREF-000]"),
            run_doc_xref_subcommand(program, remaining.iter().cloned()),
        ),
        "examples-index" => (
            Some("[EXAMPLES-INDEX-000]"),
            run_examples_index_subcommand(program, remaining.iter().cloned()),
        ),
        "dsl-capabilities" => (
            Some("[DSL-CAPS-000]"),
            run_dsl_capabilities_subcommand(program, remaining.iter().cloned()),
        ),
        _ => return None,
    };
    Some(DispatchResult {
        error_prefix,
        result,
    })
}

#[derive(Debug, Serialize)]
struct DocIndexReport {
    schema_version: u32,
    command: &'static str,
    root: String,
    output: &'static str,
    document_count: usize,
    documents: Vec<DocIndexEntry>,
}

#[derive(Debug, Serialize)]
struct DocIndexEntry {
    path: String,
    title: Option<String>,
    heading_count: usize,
    headings: Vec<DocHeading>,
}

#[derive(Debug, Clone, Serialize)]
struct DocHeading {
    level: usize,
    text: String,
    anchor: String,
}

#[derive(Debug, Serialize)]
struct DocLintReport {
    schema_version: u32,
    command: &'static str,
    root: String,
    output: &'static str,
    status: &'static str,
    document_count: usize,
    links_checked: usize,
    issue_count: usize,
    issues: Vec<DocLintIssue>,
}

#[derive(Debug, Serialize)]
struct DocLintIssue {
    code: &'static str,
    path: String,
    line: usize,
    target: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct DocXrefReport {
    schema_version: u32,
    command: &'static str,
    root: String,
    output: &'static str,
    document_count: usize,
    edge_count: usize,
    unresolved_count: usize,
    non_markdown_count: usize,
    edges: Vec<DocXrefEdge>,
}

#[derive(Debug, Serialize)]
struct DocXrefEdge {
    source_path: String,
    line: usize,
    target: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExampleCatalog {
    schema_version: u32,
    #[serde(default)]
    categories: Vec<ExampleCatalogCategory>,
}

#[derive(Debug, Deserialize)]
struct ExampleCatalogCategory {
    id: String,
    name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    examples: Vec<ExampleCatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct ExampleCatalogEntry {
    id: String,
    title: String,
    path: String,
    kind: String,
    purpose: String,
    #[serde(default)]
    scenario_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ExamplesIndexReport {
    schema_version: u32,
    command: &'static str,
    root: String,
    catalog: String,
    output: &'static str,
    category_count: usize,
    example_count: usize,
    issue_count: usize,
    categories: Vec<ExamplesIndexCategory>,
    issues: Vec<ExamplesIndexIssue>,
}

#[derive(Debug, Serialize)]
struct ExamplesIndexCategory {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    example_count: usize,
    examples: Vec<ExamplesIndexEntry>,
}

#[derive(Debug, Serialize)]
struct ExamplesIndexEntry {
    id: String,
    title: String,
    path: String,
    kind: String,
    purpose: String,
    exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scenario_exists: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ExamplesIndexIssue {
    code: &'static str,
    category_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    example_id: Option<String>,
    path: String,
    message: String,
}

#[derive(Debug, Clone)]
struct MarkdownDocument {
    path: PathBuf,
    relative_path: String,
    text: String,
    headings: Vec<DocHeading>,
}

fn run_dsl_capabilities_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "dsl-capabilities");
    let mut out_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <dsl_capabilities.json>".to_string()
                })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for dsl-capabilities: {other}\n{usage}"
                ));
            }
        }
    }

    let report = build_dsl_capabilities_report(output_mode.as_str());
    if let Some(path) = out_path.as_ref() {
        write_json_pretty(path, &report)?;
    }

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize dsl-capabilities JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!("dsl-capabilities: PASS");
            eprintln!("  parser_contract: {}", report.parser_contract);
            eprintln!("  supported: {}", report.supported_features.len());
            eprintln!("  template_assets: {}", report.template_assets.len());
            eprintln!("  unsupported: {}", report.unsupported_features.len());
            if let Some(path) = out_path {
                eprintln!("  out: {}", display_path_relative_to_cwd(&path));
            }
        }
    }

    Ok(())
}

fn run_doc_index_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "doc-index");
    let mut root = PathBuf::from("docs");
    let mut out_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "Missing value for --root <docs_dir>".to_string())?,
                );
            }
            "--out" => {
                out_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <doc_index.json|md>".to_string()
                })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for doc-index: {other}\n{usage}")),
        }
    }

    let documents = load_markdown_documents(&root)?;
    let entries = documents
        .iter()
        .map(|doc| {
            let title = doc
                .headings
                .iter()
                .find(|heading| heading.level == 1)
                .or_else(|| doc.headings.first())
                .map(|heading| heading.text.clone());
            DocIndexEntry {
                path: doc.relative_path.clone(),
                title,
                heading_count: doc.headings.len(),
                headings: doc.headings.clone(),
            }
        })
        .collect::<Vec<_>>();

    let report = DocIndexReport {
        schema_version: 1,
        command: "doc-index",
        root: display_path_relative_to_cwd(&root),
        output: output_mode.as_str(),
        document_count: entries.len(),
        documents: entries,
    };

    if let Some(path) = out_path.as_ref() {
        if is_markdown_path(path) {
            write_doc_index_markdown(path, &report)?;
        } else {
            write_json_pretty(path, &report)?;
        }
    }

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize doc-index JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!("doc-index: PASS");
            eprintln!("  root: {}", report.root);
            eprintln!("  documents: {}", report.document_count);
            if let Some(path) = out_path {
                eprintln!("  out: {}", display_path_relative_to_cwd(&path));
            }
        }
    }

    Ok(())
}

fn run_examples_index_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "examples-index");
    let mut root = PathBuf::from(".");
    let mut catalog_path = PathBuf::from("examples/catalog.toml");
    let mut out_path: Option<PathBuf> = None;
    let mut mirror_dir: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "Missing value for --root <repo_root>".to_string())?,
                );
            }
            "--catalog" => {
                catalog_path = PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --catalog <examples/catalog.toml>".to_string()
                })?);
            }
            "--out" => {
                out_path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --out <examples_index.json|md>".to_string()
                })?));
            }
            "--mirror-dir" => {
                mirror_dir = Some(PathBuf::from(args.next().ok_or_else(|| {
                    "Missing value for --mirror-dir <categorized_examples_dir>".to_string()
                })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => {
                return Err(format!(
                    "Unknown argument for examples-index: {other}\n{usage}"
                ));
            }
        }
    }

    let report = build_examples_index_report(&root, &catalog_path, output_mode.as_str())?;

    if let Some(path) = out_path.as_ref() {
        if is_markdown_path(path) {
            write_examples_index_markdown(path, &report)?;
        } else {
            write_json_pretty(path, &report)?;
        }
    }
    if let Some(path) = mirror_dir.as_ref() {
        write_examples_category_mirror(&root, path, &report)?;
    }

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize examples-index JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!(
                "examples-index: {} (issues={})",
                if report.issue_count == 0 {
                    "PASS"
                } else {
                    "FAIL"
                },
                report.issue_count
            );
            eprintln!("  catalog: {}", report.catalog);
            eprintln!("  categories: {}", report.category_count);
            eprintln!("  examples: {}", report.example_count);
            for issue in report.issues.iter().take(20) {
                eprintln!(
                    "  [{}] {} {}: {}",
                    issue.code,
                    issue.category_id,
                    issue.example_id.as_deref().unwrap_or("-"),
                    issue.message
                );
            }
            if let Some(path) = out_path {
                eprintln!("  out: {}", display_path_relative_to_cwd(&path));
            }
            if let Some(path) = mirror_dir {
                eprintln!("  mirror: {}", display_path_relative_to_cwd(&path));
            }
        }
    }

    if report.issue_count > 0 {
        return Err(format!(
            "examples-index failed: {} issue(s)",
            report.issue_count
        ));
    }

    Ok(())
}

fn build_examples_index_report(
    root: &Path,
    catalog_path: &Path,
    output: &'static str,
) -> Result<ExamplesIndexReport, String> {
    let text = fs::read_to_string(catalog_path).map_err(|err| {
        format!(
            "Failed to read examples catalog {}: {err}",
            catalog_path.display()
        )
    })?;
    let catalog: ExampleCatalog = toml::from_str(&text).map_err(|err| {
        format!(
            "Failed to parse examples catalog {}: {err}",
            catalog_path.display()
        )
    })?;
    if catalog.schema_version != 1 {
        return Err(format!(
            "Unsupported examples catalog schema_version {}; expected 1",
            catalog.schema_version
        ));
    }

    let mut issues = Vec::new();
    let mut categories = Vec::new();
    let mut category_ids = BTreeSet::new();
    let mut example_ids = BTreeSet::new();
    let mut example_count = 0usize;

    for category in catalog.categories {
        if !category_ids.insert(category.id.clone()) {
            issues.push(ExamplesIndexIssue {
                code: "EXAMPLES-CATALOG-001",
                category_id: category.id.clone(),
                example_id: None,
                path: String::new(),
                message: "duplicate category id".to_string(),
            });
        }

        let mut examples = Vec::new();
        for example in category.examples {
            example_count += 1;
            if !example_ids.insert(example.id.clone()) {
                issues.push(ExamplesIndexIssue {
                    code: "EXAMPLES-CATALOG-002",
                    category_id: category.id.clone(),
                    example_id: Some(example.id.clone()),
                    path: example.path.clone(),
                    message: "duplicate example id".to_string(),
                });
            }

            let example_exists = resolve_examples_catalog_path(root, &example.path).is_file();
            if !example_exists {
                issues.push(ExamplesIndexIssue {
                    code: "EXAMPLES-CATALOG-003",
                    category_id: category.id.clone(),
                    example_id: Some(example.id.clone()),
                    path: example.path.clone(),
                    message: "example path does not exist".to_string(),
                });
            }

            let scenario_exists = example.scenario_path.as_ref().map(|scenario_path| {
                let exists = resolve_examples_catalog_path(root, scenario_path).is_file();
                if !exists {
                    issues.push(ExamplesIndexIssue {
                        code: "EXAMPLES-CATALOG-004",
                        category_id: category.id.clone(),
                        example_id: Some(example.id.clone()),
                        path: scenario_path.clone(),
                        message: "scenario path does not exist".to_string(),
                    });
                }
                exists
            });

            examples.push(ExamplesIndexEntry {
                id: example.id,
                title: example.title,
                path: example.path,
                kind: example.kind,
                purpose: example.purpose,
                exists: example_exists,
                scenario_path: example.scenario_path,
                scenario_exists,
            });
        }

        categories.push(ExamplesIndexCategory {
            id: category.id,
            name: category.name,
            summary: category.summary,
            example_count: examples.len(),
            examples,
        });
    }

    Ok(ExamplesIndexReport {
        schema_version: 1,
        command: "examples-index",
        root: display_path_relative_to_cwd(root),
        catalog: display_path_relative_to_cwd(catalog_path),
        output,
        category_count: categories.len(),
        example_count,
        issue_count: issues.len(),
        categories,
        issues,
    })
}

fn resolve_examples_catalog_path(root: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn write_examples_index_markdown(path: &Path, report: &ExamplesIndexReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("Failed to create examples index dir {parent:?}: {err}"))?;
        }
    }

    let mut body = String::new();
    body.push_str("# RustPLC Examples Index\n\n");
    body.push_str("Generated from `");
    body.push_str(&report.catalog);
    body.push_str("`.\n\n");
    for category in &report.categories {
        let _ = writeln!(body, "## {}", category.name);
        if let Some(summary) = category.summary.as_ref() {
            let _ = writeln!(body, "\n{summary}");
        }
        body.push_str("\n| Example | Kind | Purpose |\n| --- | --- | --- |\n");
        for example in &category.examples {
            let _ = writeln!(
                body,
                "| [`{}`]({}) | `{}` | {} |",
                example.title, example.path, example.kind, example.purpose
            );
        }
        body.push('\n');
    }
    if report.issue_count > 0 {
        body.push_str("## Catalog Issues\n\n");
        for issue in &report.issues {
            let _ = writeln!(
                body,
                "- `{}` `{}`: {}",
                issue.code, issue.path, issue.message
            );
        }
    }

    fs::write(path, body).map_err(|err| format!("Failed to write examples index {path:?}: {err}"))
}

fn write_examples_category_mirror(
    root: &Path,
    mirror_dir: &Path,
    report: &ExamplesIndexReport,
) -> Result<(), String> {
    fs::create_dir_all(mirror_dir)
        .map_err(|err| format!("Failed to create examples mirror dir {mirror_dir:?}: {err}"))?;

    let root_readme = mirror_dir.join("README.md");
    let mut body = String::new();
    body.push_str("# RustPLC Categorized Examples\n\n");
    body.push_str("This directory is generated from `");
    body.push_str(&report.catalog);
    body.push_str("`. It contains navigation README files only; the source examples stay at their catalog paths.\n\n");
    body.push_str("| Category | Examples | Summary |\n| --- | ---: | --- |\n");
    for category in &report.categories {
        let _ = writeln!(
            body,
            "| [{}]({}/README.md) | {} | {} |",
            category.name,
            category.id,
            category.example_count,
            category.summary.as_deref().unwrap_or("")
        );
    }
    fs::write(&root_readme, body)
        .map_err(|err| format!("Failed to write examples mirror {root_readme:?}: {err}"))?;

    for category in &report.categories {
        let category_dir = mirror_dir.join(&category.id);
        fs::create_dir_all(&category_dir).map_err(|err| {
            format!("Failed to create examples category dir {category_dir:?}: {err}")
        })?;
        let category_readme = category_dir.join("README.md");
        let mut body = String::new();
        let _ = writeln!(body, "# {}\n", category.name);
        if let Some(summary) = category.summary.as_ref() {
            let _ = writeln!(body, "{summary}\n");
        }
        body.push_str(
            "| Example | Kind | Source | Scenario | Purpose |\n| --- | --- | --- | --- | --- |\n",
        );
        for example in &category.examples {
            let source_link = markdown_link_from_file(root, &category_readme, &example.path)?;
            let scenario = if let Some(path) = example.scenario_path.as_ref() {
                let link = markdown_link_from_file(root, &category_readme, path)?;
                format!("[`{path}`]({link})")
            } else {
                String::new()
            };
            let _ = writeln!(
                body,
                "| `{}` | `{}` | [`{}`]({}) | {} | {} |",
                example.title, example.kind, example.path, source_link, scenario, example.purpose
            );
        }
        fs::write(&category_readme, body).map_err(|err| {
            format!("Failed to write examples category mirror {category_readme:?}: {err}")
        })?;
    }

    Ok(())
}

fn markdown_link_from_file(root: &Path, from_file: &Path, target: &str) -> Result<String, String> {
    let from_dir = from_file.parent().unwrap_or_else(|| Path::new("."));
    let target_path = resolve_examples_catalog_path(root, target);
    let relative = lexical_relative_path(from_dir, &target_path)?;
    Ok(path_to_markdown_link(&relative))
}

fn lexical_relative_path(from_dir: &Path, target: &Path) -> Result<PathBuf, String> {
    let from = absolute_lexical_path(from_dir)?;
    let target = absolute_lexical_path(target)?;
    let from_components = normalized_components(&from);
    let target_components = normalized_components(&target);

    let mut common = 0usize;
    while common < from_components.len()
        && common < target_components.len()
        && from_components[common] == target_components[common]
    {
        common += 1;
    }

    if common == 0 {
        return Ok(target);
    }

    let mut relative = PathBuf::new();
    for _ in common..from_components.len() {
        relative.push("..");
    }
    for component in target_components.iter().skip(common) {
        relative.push(component);
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Ok(relative)
}

fn absolute_lexical_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|err| format!("Failed to resolve current directory: {err}"))
    }
}

fn normalized_components(path: &Path) -> Vec<String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                components.pop();
            }
            Component::Prefix(prefix) => {
                components.push(prefix.as_os_str().to_string_lossy().to_string());
            }
            Component::RootDir => {
                components.push(component.as_os_str().to_string_lossy().to_string());
            }
            Component::Normal(value) => {
                components.push(value.to_string_lossy().to_string());
            }
        }
    }
    components
}

fn path_to_markdown_link(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn run_doc_lint_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "doc-lint");
    let mut root = PathBuf::from("docs");
    let mut out_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "Missing value for --root <docs_dir>".to_string())?,
                );
            }
            "--out" => {
                out_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <doc_lint.json>".to_string()
                    })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for doc-lint: {other}\n{usage}")),
        }
    }

    let documents = load_markdown_documents(&root)?;
    let (links_checked, issues) = lint_markdown_documents(&root, &documents);
    let report = DocLintReport {
        schema_version: 1,
        command: "doc-lint",
        root: display_path_relative_to_cwd(&root),
        output: output_mode.as_str(),
        status: if issues.is_empty() { "pass" } else { "fail" },
        document_count: documents.len(),
        links_checked,
        issue_count: issues.len(),
        issues,
    };

    if let Some(path) = out_path.as_ref() {
        write_json_pretty(path, &report)?;
    }

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize doc-lint JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!(
                "doc-lint: {} (issues={})",
                report.status.to_ascii_uppercase(),
                report.issue_count
            );
            eprintln!("  root: {}", report.root);
            eprintln!("  documents: {}", report.document_count);
            eprintln!("  links_checked: {}", report.links_checked);
            for issue in report.issues.iter().take(20) {
                eprintln!(
                    "  [{}] {}:{} -> {}: {}",
                    issue.code, issue.path, issue.line, issue.target, issue.message
                );
            }
            if let Some(path) = out_path {
                eprintln!("  out: {}", display_path_relative_to_cwd(&path));
            }
        }
    }

    if report.issue_count > 0 {
        return Err(format!("doc-lint failed: {} issue(s)", report.issue_count));
    }

    Ok(())
}

fn run_doc_xref_subcommand(
    program: &str,
    mut args: impl Iterator<Item = String>,
) -> Result<(), String> {
    let usage = command_usage(program, "doc-xref");
    let mut root = PathBuf::from("docs");
    let mut out_path: Option<PathBuf> = None;
    let mut output_mode = CliOutputMode::Human;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "Missing value for --root <docs_dir>".to_string())?,
                );
            }
            "--out" => {
                out_path =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "Missing value for --out <doc_xref.json>".to_string()
                    })?));
            }
            "--output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "Missing value for --output <human|json>".to_string())?;
                output_mode = CliOutputMode::parse(&raw).ok_or_else(|| {
                    format!("Invalid value for --output `{raw}` (expected human or json)")
                })?;
            }
            "-h" | "--help" => return Err(usage.clone()),
            other => return Err(format!("Unknown argument for doc-xref: {other}\n{usage}")),
        }
    }

    let documents = load_markdown_documents(&root)?;
    let edges = build_doc_xref_edges(&root, &documents);
    let unresolved_count = edges
        .iter()
        .filter(|edge| edge.status == "unresolved")
        .count();
    let non_markdown_count = edges
        .iter()
        .filter(|edge| edge.status == "non_markdown")
        .count();
    let report = DocXrefReport {
        schema_version: 1,
        command: "doc-xref",
        root: display_path_relative_to_cwd(&root),
        output: output_mode.as_str(),
        document_count: documents.len(),
        edge_count: edges.len(),
        unresolved_count,
        non_markdown_count,
        edges,
    };

    if let Some(path) = out_path.as_ref() {
        write_json_pretty(path, &report)?;
    }

    match output_mode {
        CliOutputMode::Json => {
            let mut body = serde_json::to_string_pretty(&report)
                .map_err(|err| format!("Failed to serialize doc-xref JSON: {err}"))?;
            body.push('\n');
            print!("{body}");
        }
        CliOutputMode::Human => {
            eprintln!("doc-xref: PASS");
            eprintln!("  root: {}", report.root);
            eprintln!("  documents: {}", report.document_count);
            eprintln!("  edges: {}", report.edge_count);
            eprintln!("  unresolved: {}", report.unresolved_count);
            eprintln!("  non_markdown: {}", report.non_markdown_count);
            if let Some(path) = out_path {
                eprintln!("  out: {}", display_path_relative_to_cwd(&path));
            }
        }
    }

    Ok(())
}

fn load_markdown_documents(root: &Path) -> Result<Vec<MarkdownDocument>, String> {
    if !root.is_dir() {
        return Err(format!("Documentation root not found: {}", root.display()));
    }
    let root_abs = root.canonicalize().map_err(|err| {
        format!(
            "Failed to canonicalize documentation root {}: {err}",
            root.display()
        )
    })?;
    let mut paths = Vec::new();
    collect_markdown_paths(&root_abs, &mut paths)?;
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path)
                .map_err(|err| format!("Failed to read markdown file {}: {err}", path.display()))?;
            let relative_path = path
                .strip_prefix(&root_abs)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let headings = extract_markdown_headings(&text);
            Ok(MarkdownDocument {
                path,
                relative_path,
                text,
                headings,
            })
        })
        .collect()
}

fn collect_markdown_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir)
        .map_err(|err| format!("Failed to read directory {}: {err}", dir.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|err| format!("Failed to read directory entry in {}: {err}", dir.display()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_markdown_paths(&path, paths)?;
        } else if is_markdown_path(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn extract_markdown_headings(text: &str) -> Vec<DocHeading> {
    let mut seen = BTreeMap::<String, usize>::new();
    let mut headings = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let level = trimmed.chars().take_while(|ch| *ch == '#').count();
        if level == 0 || level > 6 {
            continue;
        }
        if !trimmed[level..].starts_with(' ') {
            continue;
        }
        let text = trimmed[level..].trim().trim_end_matches('#').trim();
        if text.is_empty() {
            continue;
        }
        let base_anchor = markdown_anchor(text);
        let count = seen.entry(base_anchor.clone()).or_insert(0);
        let anchor = if *count == 0 {
            base_anchor
        } else {
            format!("{base_anchor}-{count}")
        };
        *count += 1;
        headings.push(DocHeading {
            level,
            text: text.to_string(),
            anchor,
        });
    }
    headings
}

fn markdown_anchor(text: &str) -> String {
    let mut anchor = String::new();
    let mut previous_dash = false;
    for ch in text.trim().to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '_' {
            anchor.push(ch);
            previous_dash = false;
        } else if ch.is_whitespace() || ch == '-' {
            if !previous_dash && !anchor.is_empty() {
                anchor.push('-');
                previous_dash = true;
            }
        }
    }
    while anchor.ends_with('-') {
        anchor.pop();
    }
    anchor
}

fn write_doc_index_markdown(path: &Path, report: &DocIndexReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "Failed to create output directory {}: {err}",
                    parent.display()
                )
            })?;
        }
    }
    let mut body = String::new();
    body.push_str("# RustPLC Documentation Index\n\n");
    body.push_str(&format!("- Root: `{}`\n", report.root));
    body.push_str(&format!("- Documents: `{}`\n\n", report.document_count));
    for doc in &report.documents {
        let title = doc.title.as_deref().unwrap_or("<untitled>");
        body.push_str(&format!("## {}\n\n", doc.path));
        body.push_str(&format!("- Title: `{title}`\n"));
        body.push_str(&format!("- Headings: `{}`\n", doc.heading_count));
        if !doc.headings.is_empty() {
            body.push('\n');
            for heading in &doc.headings {
                body.push_str(&format!(
                    "{} [{}](./{}#{})\n",
                    "  ".repeat(heading.level.saturating_sub(1)),
                    heading.text,
                    doc.path,
                    heading.anchor
                ));
            }
        }
        body.push('\n');
    }
    fs::write(path, body).map_err(|err| format!("Failed to write {}: {err}", path.display()))
}

fn build_doc_xref_edges(root: &Path, documents: &[MarkdownDocument]) -> Vec<DocXrefEdge> {
    let mut by_path = BTreeMap::<PathBuf, String>::new();
    for doc in documents {
        let canonical = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
        by_path.insert(canonical, doc.relative_path.clone());
    }

    let mut edges = Vec::new();
    for doc in documents {
        for link in extract_markdown_links(&doc.text) {
            if should_skip_markdown_target(&link.target) {
                continue;
            }
            let (target_path, fragment) = split_markdown_target(&link.target);
            let resolved = if target_path.is_empty() {
                doc.path.clone()
            } else {
                doc.path
                    .parent()
                    .unwrap_or_else(|| root)
                    .join(percent_decode(target_path))
            };
            let (status, target_path) = match resolved.canonicalize() {
                Ok(path) => match by_path.get(&path) {
                    Some(relative) => ("resolved", Some(relative.clone())),
                    None => (
                        "non_markdown",
                        Some(display_path_relative_to_cwd(&resolved)),
                    ),
                },
                Err(_) => ("unresolved", None),
            };
            let fragment = fragment.map(percent_decode);
            edges.push(DocXrefEdge {
                source_path: doc.relative_path.clone(),
                line: link.line,
                target: link.target,
                status,
                target_path,
                fragment,
            });
        }
    }
    edges
}

fn lint_markdown_documents(
    root: &Path,
    documents: &[MarkdownDocument],
) -> (usize, Vec<DocLintIssue>) {
    let mut by_path = BTreeMap::<PathBuf, BTreeSet<String>>::new();
    for doc in documents {
        let canonical = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
        let anchors = doc
            .headings
            .iter()
            .map(|heading| heading.anchor.clone())
            .collect::<BTreeSet<_>>();
        by_path.insert(canonical, anchors);
    }

    let mut links_checked = 0;
    let mut issues = Vec::new();
    for doc in documents {
        for link in extract_markdown_links(&doc.text) {
            if should_skip_markdown_target(&link.target) {
                continue;
            }
            links_checked += 1;
            let (target_path, fragment) = split_markdown_target(&link.target);
            let resolved = if target_path.is_empty() {
                doc.path.clone()
            } else {
                doc.path
                    .parent()
                    .unwrap_or_else(|| root)
                    .join(percent_decode(target_path))
            };
            let resolved_canonical = match resolved.canonicalize() {
                Ok(path) => path,
                Err(_) => {
                    issues.push(DocLintIssue {
                        code: "DOC-LINK-001",
                        path: doc.relative_path.clone(),
                        line: link.line,
                        target: link.target,
                        message: format!("Local markdown target not found: {}", resolved.display()),
                    });
                    continue;
                }
            };
            if let Some(fragment) = fragment {
                let Some(anchors) = by_path.get(&resolved_canonical) else {
                    continue;
                };
                let wanted = percent_decode(fragment)
                    .trim_start_matches('#')
                    .to_lowercase();
                if !wanted.is_empty() && !anchors.contains(&wanted) {
                    issues.push(DocLintIssue {
                        code: "DOC-LINK-002",
                        path: doc.relative_path.clone(),
                        line: link.line,
                        target: link.target,
                        message: format!("Markdown anchor not found: #{wanted}"),
                    });
                }
            }
        }
    }
    (links_checked, issues)
}

#[derive(Debug)]
struct MarkdownLink {
    line: usize,
    target: String,
}

fn extract_markdown_links(text: &str) -> Vec<MarkdownLink> {
    let mut links = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut idx = 0;
        while idx < bytes.len() {
            if bytes[idx] != b'[' || (idx > 0 && bytes[idx - 1] == b'!') {
                idx += 1;
                continue;
            }
            let Some(close_rel) = line[idx + 1..].find(']') else {
                break;
            };
            let close = idx + 1 + close_rel;
            if !line[close + 1..].starts_with('(') {
                idx = close + 1;
                continue;
            }
            let target_start = close + 2;
            let Some(target_close_rel) = line[target_start..].find(')') else {
                break;
            };
            let target_close = target_start + target_close_rel;
            let target = line[target_start..target_close]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches('<')
                .trim_matches('>')
                .to_string();
            if !target.is_empty() {
                links.push(MarkdownLink {
                    line: line_idx + 1,
                    target,
                });
            }
            idx = target_close + 1;
        }
    }
    links
}

fn should_skip_markdown_target(target: &str) -> bool {
    let lower = target.to_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
        || lower.starts_with("ftp://")
        || lower.starts_with("file:")
}

fn split_markdown_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (target, None),
    }
}

fn percent_decode(input: &str) -> String {
    let mut output = Vec::new();
    let bytes = input.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            let hi = from_hex(bytes[idx + 1]);
            let lo = from_hex(bytes[idx + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                output.push(hi * 16 + lo);
                idx += 3;
                continue;
            }
        }
        output.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
