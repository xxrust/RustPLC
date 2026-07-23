use crate::ast::{DeviceType, PlcProgram};
use crate::device_library::DeviceLibrary;
use crate::error::PlcError;
use crate::parser::parse_plc;
use crate::semantic::{
    build_constraint_set, build_state_machine, build_timing_model, build_topology_graph,
    preprocess_program_with_library,
};
use crate::topology_semantic_gate::{
    TopologySemanticGateError, validate_device_purpose_required, validate_removed_legacy_io_model,
    validate_topology_semantics,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as JsonResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

const KEYWORDS: &[&str] = &[
    "topology",
    "constraints",
    "tasks",
    "device",
    "device_template",
    "device_instance",
    "relation",
    "controller_io",
    "station",
    "handshake",
    "transfer_point",
    "controller_sync",
    "variable",
    "workpiece_type",
    "workpiece_site",
    "semantic_resource",
    "task",
    "task_template",
    "task_instance",
    "step",
    "action",
    "wait",
    "delay",
    "timeout",
    "goto",
    "on_complete",
    "match",
    "case",
    "default",
    "purpose",
    "model_ref",
    "subtype",
    "ports",
    "tags",
    "functional_group",
    "danger_level",
    "location_group",
    "from",
    "to",
    "via",
    "driven_by",
    "reports_to",
    "detects",
    "extend",
    "retract",
    "set",
    "compute",
    "allow_indefinite_wait",
];

#[derive(Debug, Clone, Default)]
pub struct LspAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub symbols: Vec<LspSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LspSymbol {
    pub name: String,
    pub qualified_name: String,
    pub kind: LspSymbolKind,
    pub line: usize,
    pub detail: String,
    pub documentation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LspSymbolKind {
    Device,
    Task,
    Step,
    Variable,
    Resource,
    Workpiece,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspCompletion {
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    pub snippet: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspLanguageSnapshot {
    pub symbols: Vec<LspSymbol>,
    pub completions: Vec<LspCompletion>,
}

#[derive(Debug, Clone, Default)]
struct DocumentState {
    text: String,
    analysis: LspAnalysis,
}

#[derive(Debug, Clone)]
enum CachedDeviceLibrary {
    Loaded(DeviceLibrary),
    Errors(Vec<PlcError>),
}

pub async fn run_stdio_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: Arc::new(RwLock::new(HashMap::new())),
        device_library: Arc::new(RwLock::new(None)),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}

struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
    device_library: Arc<RwLock<Option<CachedDeviceLibrary>>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> JsonResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        ":".to_string(),
                        " ".to_string(),
                    ]),
                    ..CompletionOptions::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "rustplc-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "RustPLC LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> JsonResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.update_document(
            params.text_document.uri,
            params.text_document.version,
            params.text_document.text,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.update_document(
                params.text_document.uri,
                params.text_document.version,
                change.text,
            )
            .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> JsonResult<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let state = self.documents.read().await.get(&uri).cloned();
        let items = completion_items(state.as_ref().map(|state| &state.analysis));
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> JsonResult<Option<Hover>> {
        let text_position = params.text_document_position_params;
        let uri = text_position.text_document.uri;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let Some(word) = word_at_position(&state.text, text_position.position) else {
            return Ok(None);
        };
        let Some(symbol) = lookup_symbol(&state.analysis, &word) else {
            return Ok(None);
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: symbol.documentation.clone(),
            }),
            range: Some(range_for_line(&state.text, symbol.line, 1)),
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> JsonResult<Option<GotoDefinitionResponse>> {
        let text_position = params.text_document_position_params;
        let uri = text_position.text_document.uri;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let Some(word) = word_at_position(&state.text, text_position.position) else {
            return Ok(None);
        };
        let Some(symbol) = lookup_symbol(&state.analysis, &word) else {
            return Ok(None);
        };
        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri,
            range: range_for_line(&state.text, symbol.line, 1),
        })))
    }

    async fn references(&self, params: ReferenceParams) -> JsonResult<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let Some(word) = word_at_position(&state.text, params.text_document_position.position)
        else {
            return Ok(None);
        };
        let locations = word_ranges(&state.text, &word)
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range,
            })
            .collect::<Vec<_>>();
        Ok(Some(locations))
    }

    async fn rename(&self, params: RenameParams) -> JsonResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let Some(word) = word_at_position(&state.text, params.text_document_position.position)
        else {
            return Ok(None);
        };
        if !is_identifier(&params.new_name) || is_reserved_keyword(&params.new_name) {
            return Ok(None);
        }
        let edits = word_ranges(&state.text, &word)
            .into_iter()
            .map(|range| TextEdit {
                range,
                new_text: params.new_name.clone(),
            })
            .collect::<Vec<_>>();
        let mut changes = HashMap::new();
        changes.insert(uri, edits);
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }))
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> JsonResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let formatted = normalize_document_whitespace(&state.text);
        if formatted == state.text {
            return Ok(Some(Vec::new()));
        }
        Ok(Some(vec![TextEdit {
            range: full_document_range(&state.text),
            new_text: formatted,
        }]))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> JsonResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        Ok(Some(format_range_edits(&state.text, params.range)))
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> JsonResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let actions = code_actions_for_range(&state.text, uri, params.range, &params.context);
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}

impl Backend {
    async fn update_document(&self, uri: Url, version: i32, text: String) {
        if let Some(existing) = self
            .documents
            .read()
            .await
            .get(&uri)
            .filter(|state| state.text == text)
            .cloned()
        {
            self.client
                .publish_diagnostics(uri, existing.analysis.diagnostics, Some(version))
                .await;
            return;
        }

        let device_library = self.cached_device_library().await;
        let analysis = analyze_document_with_device_library(&text, &device_library);
        let diagnostics = analysis.diagnostics.clone();
        self.documents
            .write()
            .await
            .insert(uri.clone(), DocumentState { text, analysis });
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn cached_device_library(&self) -> CachedDeviceLibrary {
        if let Some(cached) = self.device_library.read().await.clone() {
            return cached;
        }

        let loaded = load_device_library();
        let mut cache = self.device_library.write().await;
        if let Some(cached) = cache.clone() {
            cached
        } else {
            *cache = Some(loaded.clone());
            loaded
        }
    }
}

pub fn analyze_document(source: &str) -> LspAnalysis {
    let device_library = load_device_library();
    analyze_document_with_device_library(source, &device_library)
}

fn analyze_document_with_device_library(
    source: &str,
    device_library: &CachedDeviceLibrary,
) -> LspAnalysis {
    let mut analysis = LspAnalysis::default();
    let program = match parse_plc(source) {
        Ok(program) => program,
        Err(error) => {
            analysis
                .diagnostics
                .push(plc_error_to_diagnostic(source, error));
            return analysis;
        }
    };

    analysis.symbols = collect_symbols(&program);
    collect_front_door_diagnostics(source, &program, &mut analysis.diagnostics);
    if !analysis.diagnostics.is_empty() {
        return analysis;
    }

    let device_library = match device_library {
        CachedDeviceLibrary::Loaded(library) => library,
        CachedDeviceLibrary::Errors(errors) => {
            analysis.diagnostics.extend(
                errors
                    .iter()
                    .cloned()
                    .map(|error| plc_error_to_diagnostic(source, error)),
            );
            return analysis;
        }
    };
    let expanded = match preprocess_program_with_library(
        &program,
        if device_library.is_empty() {
            None
        } else {
            Some(&device_library)
        },
    ) {
        Ok(program) => program,
        Err(errors) => {
            analysis.diagnostics.extend(
                errors
                    .into_iter()
                    .map(|error| plc_error_to_diagnostic(source, error)),
            );
            return analysis;
        }
    };

    collect_topology_gate_diagnostics(
        source,
        validate_topology_semantics(&expanded.topology),
        &mut analysis.diagnostics,
    );
    collect_semantic_stage_diagnostics(source, &expanded, &mut analysis.diagnostics);
    analysis
}

fn load_device_library() -> CachedDeviceLibrary {
    match DeviceLibrary::load(Path::new("devices")) {
        Ok(library) => CachedDeviceLibrary::Loaded(library),
        Err(errors) => CachedDeviceLibrary::Errors(errors),
    }
}

fn collect_front_door_diagnostics(
    source: &str,
    program: &PlcProgram,
    diagnostics: &mut Vec<Diagnostic>,
) {
    collect_topology_gate_diagnostics(
        source,
        validate_removed_legacy_io_model(&program.topology),
        diagnostics,
    );
    collect_topology_gate_diagnostics(
        source,
        validate_device_purpose_required(&program.topology),
        diagnostics,
    );
}

fn collect_semantic_stage_diagnostics(
    source: &str,
    program: &PlcProgram,
    diagnostics: &mut Vec<Diagnostic>,
) {
    append_stage_errors(source, build_topology_graph(program), diagnostics);
    append_stage_errors(source, build_state_machine(program), diagnostics);
    append_stage_errors(source, build_constraint_set(program), diagnostics);
    append_stage_errors(source, build_timing_model(program), diagnostics);
}

fn append_stage_errors<T>(
    source: &str,
    result: Result<T, Vec<PlcError>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Err(errors) = result {
        diagnostics.extend(
            errors
                .into_iter()
                .map(|error| plc_error_to_diagnostic(source, error)),
        );
    }
}

fn collect_topology_gate_diagnostics(
    source: &str,
    result: Result<(), TopologySemanticGateError>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Err(error) = result else {
        return;
    };
    diagnostics.extend(error.issues.into_iter().map(|issue| Diagnostic {
        range: range_for_line(source, issue.line.max(1), 1),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(issue.code.as_str().to_string())),
        code_description: None,
        source: Some("rustplc".to_string()),
        message: format!("{}\nfix: {}", issue.message, issue.suggestion),
        related_information: None,
        tags: None,
        data: None,
    }));
}

fn plc_error_to_diagnostic(source: &str, error: PlcError) -> Diagnostic {
    Diagnostic {
        range: range_for_line(source, error.line().max(1), error.column().max(1)),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(plc_error_code(&error).to_string())),
        code_description: None,
        source: Some("rustplc".to_string()),
        message: error.to_string(),
        related_information: None,
        tags: None,
        data: None,
    }
}

fn plc_error_code(error: &PlcError) -> &'static str {
    match error {
        PlcError::ParseError { .. } => "parse",
        PlcError::SemanticError { .. } => "semantic",
        PlcError::UndefinedReference { .. } => "undefined_reference",
        PlcError::TypeMismatch { .. } => "type_mismatch",
        PlcError::DuplicateDefinition { .. } => "duplicate_definition",
    }
}

fn collect_symbols(program: &PlcProgram) -> Vec<LspSymbol> {
    let mut symbols = Vec::new();
    for device in &program.topology.devices {
        let type_name = device_type_name(&device.device_type);
        let purpose = device
            .attributes
            .purpose
            .as_deref()
            .unwrap_or("No purpose declared.");
        let ports = if device.attributes.ports.is_empty() {
            "ports: inferred from device library".to_string()
        } else {
            format!(
                "ports: {}",
                device
                    .attributes
                    .ports
                    .iter()
                    .map(|port| format!("{}:{:?}/{:?}", port.id, port.port_type, port.role))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        symbols.push(LspSymbol {
            name: device.name.clone(),
            qualified_name: device.name.clone(),
            kind: LspSymbolKind::Device,
            line: device.line.max(1),
            detail: format!("device {type_name}"),
            documentation: format!(
                "**{}**\n\n`device {type_name}`\n\n{}\n\n{}",
                device.name, purpose, ports
            ),
        });
    }
    for variable in &program.topology.variables {
        symbols.push(LspSymbol {
            name: variable.name.clone(),
            qualified_name: variable.name.clone(),
            kind: LspSymbolKind::Variable,
            line: variable.line.max(1),
            detail: format!("variable {:?}", variable.var_type),
            documentation: format!(
                "**{}**\n\n`variable {:?}`\n\ninitial: `{}`",
                variable.name, variable.var_type, variable.initial_value
            ),
        });
    }
    for resource in &program.topology.semantic_resources {
        symbols.push(LspSymbol {
            name: resource.name.clone(),
            qualified_name: resource.name.clone(),
            kind: LspSymbolKind::Resource,
            line: resource.line.max(1),
            detail: "semantic_resource".to_string(),
            documentation: format!("**{}**\n\n`semantic_resource`", resource.name),
        });
    }
    for workpiece in &program.topology.workpiece_types {
        symbols.push(LspSymbol {
            name: workpiece.name.clone(),
            qualified_name: workpiece.name.clone(),
            kind: LspSymbolKind::Workpiece,
            line: workpiece.line.max(1),
            detail: "workpiece_type".to_string(),
            documentation: format!("**{}**\n\n`workpiece_type`", workpiece.name),
        });
    }
    for task in &program.tasks.tasks {
        symbols.push(LspSymbol {
            name: task.name.clone(),
            qualified_name: task.name.clone(),
            kind: LspSymbolKind::Task,
            line: task.line.max(1),
            detail: "task".to_string(),
            documentation: format!(
                "**{}**\n\n`task` with {} step(s)",
                task.name,
                task.steps.len()
            ),
        });
        for step in &task.steps {
            symbols.push(LspSymbol {
                name: step.name.clone(),
                qualified_name: format!("{}.{}", task.name, step.name),
                kind: LspSymbolKind::Step,
                line: step.line.max(1),
                detail: format!("step in task {}", task.name),
                documentation: format!(
                    "**{}.{}**\n\n`step` with {} statement(s)",
                    task.name,
                    step.name,
                    step.statements.len()
                ),
            });
        }
    }
    symbols
}

pub fn completion_items_for_analysis(analysis: Option<&LspAnalysis>) -> Vec<CompletionItem> {
    completion_items(analysis)
}

pub fn language_snapshot_for_source(source: &str) -> LspLanguageSnapshot {
    let analysis = analyze_document(source);
    language_snapshot_for_analysis(&analysis)
}

pub fn language_snapshot_for_analysis(analysis: &LspAnalysis) -> LspLanguageSnapshot {
    LspLanguageSnapshot {
        symbols: analysis.symbols.clone(),
        completions: completion_items(Some(analysis))
            .into_iter()
            .map(lsp_completion_from_item)
            .collect(),
    }
}

fn completion_items(analysis: Option<&LspAnalysis>) -> Vec<CompletionItem> {
    let mut items = KEYWORDS
        .iter()
        .map(|keyword| CompletionItem {
            label: (*keyword).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            insert_text: Some((*keyword).to_string()),
            ..CompletionItem::default()
        })
        .collect::<Vec<_>>();

    items.extend(snippet_items());

    if let Some(analysis) = analysis {
        for symbol in &analysis.symbols {
            items.push(CompletionItem {
                label: symbol.qualified_name.clone(),
                detail: Some(symbol.detail.clone()),
                documentation: Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: symbol.documentation.clone(),
                })),
                kind: Some(match symbol.kind {
                    LspSymbolKind::Device => CompletionItemKind::CLASS,
                    LspSymbolKind::Task => CompletionItemKind::MODULE,
                    LspSymbolKind::Step => CompletionItemKind::FIELD,
                    LspSymbolKind::Variable => CompletionItemKind::VARIABLE,
                    LspSymbolKind::Resource => CompletionItemKind::VALUE,
                    LspSymbolKind::Workpiece => CompletionItemKind::STRUCT,
                }),
                ..CompletionItem::default()
            });
        }
    }

    items
}

fn lsp_completion_from_item(item: CompletionItem) -> LspCompletion {
    LspCompletion {
        label: item.label,
        kind: item
            .kind
            .map(|kind| format!("{kind:?}").to_ascii_lowercase())
            .unwrap_or_else(|| "text".to_string()),
        detail: item.detail,
        documentation: item.documentation.map(documentation_to_markdown),
        insert_text: item.insert_text,
        snippet: item.insert_text_format == Some(InsertTextFormat::SNIPPET),
    }
}

fn documentation_to_markdown(documentation: Documentation) -> String {
    match documentation {
        Documentation::String(text) => text,
        Documentation::MarkupContent(content) => content.value,
    }
}

fn snippet_items() -> Vec<CompletionItem> {
    vec![
        CompletionItem {
            label: "relation block".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("relation { from: ${1:source.port}, to: ${2:target.port}, via: ${3|driven_by,reports_to,detects|} }".to_string()),
            ..CompletionItem::default()
        },
        CompletionItem {
            label: "task block".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("task ${1:name}:\n    step ${2:start}:\n        ${3:action: set device.port on}\n    on_complete: goto ${4:done}".to_string()),
            ..CompletionItem::default()
        },
        CompletionItem {
            label: "task_template block".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("task_template ${1:name}<${2:ACT}>:\n    task ${3:run}:\n        step ${4:start}:\n            action: extend ${2:ACT}\n            timeout: ${5:100ms} -> goto ${6:fault}\n            goto ${7:done}\n        step ${7:done}:\n        step ${6:fault}:\n    on_complete: unreachable\ntask_instance ${8:instance}: ${1:name}<${9:device}>".to_string()),
            ..CompletionItem::default()
        },
        CompletionItem {
            label: "device block".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("device ${1:name}: ${2|sensor,cylinder,solenoid_valve,plc,stepper_motor,servo_drive|} {\n    purpose: \"${3:role in the process}\"\n}".to_string()),
            ..CompletionItem::default()
        },
        CompletionItem {
            label: "device_template block".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("device_template ${1:name}<${2:T}> {\n    device ${3:main}: ${2:T} {\n        purpose: \"${4:templated device role}\"\n    }\n}\ndevice_instance ${5:instance}: ${1:name}<${6|sensor,cylinder,solenoid_valve,stepper_motor,servo_drive|}>".to_string()),
            ..CompletionItem::default()
        },
        CompletionItem {
            label: "controller_sync block".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            insert_text: Some("controller_sync ${1:name} {\n    controllers: [${2:plc_a}, ${3:plc_b}],\n    max_skew: ${4:5ms},\n    heartbeat: ${5:100ms}\n}".to_string()),
            ..CompletionItem::default()
        },
    ]
}

fn lookup_symbol<'a>(analysis: &'a LspAnalysis, word: &str) -> Option<&'a LspSymbol> {
    analysis
        .symbols
        .iter()
        .find(|symbol| symbol.qualified_name == word || symbol.name == word)
}

fn range_for_line(source: &str, line: usize, column: usize) -> Range {
    let line_index = line.saturating_sub(1) as u32;
    let char_count = source
        .lines()
        .nth(line.saturating_sub(1))
        .map(|line| line.chars().count() as u32)
        .unwrap_or(0);
    let start_character = (column.saturating_sub(1) as u32).min(char_count);
    let end_character = (start_character + 1).min(char_count.max(start_character + 1));
    Range {
        start: Position {
            line: line_index,
            character: start_character,
        },
        end: Position {
            line: line_index,
            character: end_character,
        },
    }
}

fn line_full_range(source: &str, zero_based_line: u32) -> Range {
    let lines = source.lines().collect::<Vec<_>>();
    let line_idx = zero_based_line as usize;
    if line_idx + 1 < lines.len() {
        Range {
            start: Position {
                line: zero_based_line,
                character: 0,
            },
            end: Position {
                line: zero_based_line + 1,
                character: 0,
            },
        }
    } else {
        Range {
            start: Position {
                line: zero_based_line,
                character: 0,
            },
            end: Position {
                line: zero_based_line,
                character: lines
                    .get(line_idx)
                    .map(|line| line.chars().count() as u32)
                    .unwrap_or(0),
            },
        }
    }
}

fn full_document_range(source: &str) -> Range {
    let line_count = source.lines().count() as u32;
    let last_line = line_count.saturating_sub(1);
    let last_character = source
        .lines()
        .last()
        .map(|line| line.chars().count() as u32)
        .unwrap_or(0);
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: last_line,
            character: last_character,
        },
    }
}

fn word_at_position(source: &str, position: Position) -> Option<String> {
    let line = source.lines().nth(position.line as usize)?;
    let chars = line.chars().collect::<Vec<_>>();
    let mut index = (position.character as usize).min(chars.len());
    if index == chars.len() && index > 0 {
        index -= 1;
    }
    if chars.get(index).is_some_and(|ch| !is_word_char(*ch)) && index > 0 {
        index -= 1;
    }
    if chars.get(index).is_some_and(|ch| !is_word_char(*ch)) {
        return None;
    }
    let mut start = index;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = index + 1;
    while end < chars.len() && is_word_char(chars[end]) {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

fn word_ranges(source: &str, word: &str) -> Vec<Range> {
    if word.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let mut byte_offset = 0;
        while let Some(relative) = line[byte_offset..].find(word) {
            let start_byte = byte_offset + relative;
            let end_byte = start_byte + word.len();
            let before = line[..start_byte].chars().last();
            let after = line[end_byte..].chars().next();
            if before.is_none_or(|ch| !is_word_char(ch)) && after.is_none_or(|ch| !is_word_char(ch))
            {
                let start_character = line[..start_byte].chars().count() as u32;
                let end_character = line[..end_byte].chars().count() as u32;
                ranges.push(Range {
                    start: Position {
                        line: line_idx as u32,
                        character: start_character,
                    },
                    end: Position {
                        line: line_idx as u32,
                        character: end_character,
                    },
                });
            }
            byte_offset = end_byte;
        }
    }
    ranges
}

fn normalize_document_whitespace(source: &str) -> String {
    let mut out = source
        .lines()
        .map(|line| line.trim_end().replace('\t', "    "))
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    out
}

fn format_range_edits(source: &str, range: Range) -> Vec<TextEdit> {
    let lines = source.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return Vec::new();
    }
    let start = (range.start.line as usize).min(lines.len().saturating_sub(1));
    let mut end = (range.end.line as usize).min(lines.len().saturating_sub(1));
    if range.end.character == 0 && end > start {
        end -= 1;
    }
    let selected = lines[start..=end]
        .iter()
        .map(|line| line.trim_end().replace('\t', "    "))
        .collect::<Vec<_>>();
    let mut new_text = selected.join("\n");
    if end + 1 < lines.len() {
        new_text.push('\n');
    }
    let edit_range = Range {
        start: Position {
            line: start as u32,
            character: 0,
        },
        end: if end + 1 < lines.len() {
            Position {
                line: (end + 1) as u32,
                character: 0,
            }
        } else {
            Position {
                line: end as u32,
                character: lines[end].chars().count() as u32,
            }
        },
    };
    let original = source_slice_by_line_range(source, edit_range);
    if original == new_text {
        Vec::new()
    } else {
        vec![TextEdit {
            range: edit_range,
            new_text,
        }]
    }
}

fn source_slice_by_line_range(source: &str, range: Range) -> String {
    let lines = source.lines().collect::<Vec<_>>();
    let start = range.start.line as usize;
    if start >= lines.len() {
        return String::new();
    }
    if range.end.line as usize > start && range.end.character == 0 {
        let end = (range.end.line as usize).min(lines.len());
        let mut text = lines[start..end].join("\n");
        if end < lines.len() || source.ends_with('\n') {
            text.push('\n');
        }
        text
    } else {
        lines[start..=(range.end.line as usize).min(lines.len() - 1)].join("\n")
    }
}

fn code_actions_for_range(
    source: &str,
    uri: Url,
    range: Range,
    context: &CodeActionContext,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let mut seen_titles = HashSet::new();

    for diagnostic in &context.diagnostics {
        if diagnostic
            .code
            .as_ref()
            .is_some_and(|code| code == &NumberOrString::String("SEM-107".to_string()))
        {
            if let Some(action) = missing_purpose_action(source, &uri, diagnostic.range) {
                if seen_titles.insert(action.title.clone()) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
        }
    }

    for line in range.start.line..=range.end.line {
        if let Some(action) = connected_to_migration_action(source, &uri, line) {
            if seen_titles.insert(action.title.clone()) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
    }

    actions
}

fn missing_purpose_action(source: &str, uri: &Url, range: Range) -> Option<CodeAction> {
    let line = source.lines().nth(range.start.line as usize)?;
    if !line.trim_start().starts_with("device ") {
        return None;
    }
    let indent = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let insert_line = range.start.line + 1;
    let edit = TextEdit {
        range: Range {
            start: Position {
                line: insert_line,
                character: 0,
            },
            end: Position {
                line: insert_line,
                character: 0,
            },
        },
        new_text: format!("{indent}    purpose: \"TODO: describe device role\"\n"),
    };
    Some(workspace_edit_action(
        "Add TODO purpose field",
        CodeActionKind::QUICKFIX,
        uri,
        edit,
    ))
}

fn connected_to_migration_action(source: &str, uri: &Url, line_idx: u32) -> Option<CodeAction> {
    let line = source.lines().nth(line_idx as usize)?;
    let code = line.split('#').next().unwrap_or(line);
    let connected_to_col = code.find("connected_to")?;
    let tail = &code[connected_to_col + "connected_to".len()..];
    if !tail.trim_start().starts_with(':') {
        return None;
    }
    let value = tail
        .trim_start()
        .trim_start_matches(':')
        .trim()
        .trim_end_matches(',')
        .trim();
    let indent = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let replacement = format!(
        "{indent}# TODO migrate deprecated connected_to: {value} to relation {{ from: source.port, to: target.port, via: driven_by|reports_to|detects }}\n"
    );
    let edit = TextEdit {
        range: line_full_range(source, line_idx),
        new_text: replacement,
    };
    Some(workspace_edit_action(
        "Comment deprecated connected_to and insert relation TODO",
        CodeActionKind::QUICKFIX,
        uri,
        edit,
    ))
}

fn workspace_edit_action(
    title: &str,
    kind: CodeActionKind,
    uri: &Url,
    edit: TextEdit,
) -> CodeAction {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeAction {
        title: title.to_string(),
        kind: Some(kind),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }),
        command: None,
        is_preferred: Some(false),
        disabled: None,
        data: None,
    }
}

fn is_reserved_keyword(value: &str) -> bool {
    KEYWORDS.iter().any(|keyword| *keyword == value)
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_word_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn device_type_name(device_type: &DeviceType) -> &'static str {
    match device_type {
        DeviceType::DigitalOutput => "digital_output",
        DeviceType::DigitalInput => "digital_input",
        DeviceType::Plc => "plc",
        DeviceType::SolenoidValve => "solenoid_valve",
        DeviceType::Cylinder => "cylinder",
        DeviceType::Sensor => "sensor",
        DeviceType::Motor => "motor",
        DeviceType::StepperMotor => "stepper_motor",
        DeviceType::Vfd => "vfd",
        DeviceType::ServoDrive => "servo_drive",
        DeviceType::CamCoupling => "cam_coupling",
        DeviceType::AnalogInput => "analog_input",
        DeviceType::AnalogOutput => "analog_output",
        DeviceType::Pid => "pid",
        DeviceType::ProportionalValve => "proportional_valve",
        DeviceType::Gripper => "gripper",
        DeviceType::Conveyor => "conveyor",
        DeviceType::Pump => "pump",
        DeviceType::Heater => "heater",
        DeviceType::VisionSensor => "vision_sensor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn lsp_analysis_maps_parse_errors_to_diagnostics() {
        let source = "[topology]\n\ndevice bad: sensor {\n    connected_to: X0\n}\n";
        let analysis = analyze_document(source);

        assert!(!analysis.diagnostics.is_empty());
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code
                    == Some(NumberOrString::String("parse".to_string())))
        );
    }

    #[test]
    fn lsp_analysis_collects_symbols_for_valid_example() {
        let source = include_str!("../examples/demo.plc");
        let analysis = analyze_document(source);

        assert!(
            analysis.symbols.iter().any(|symbol| symbol.name == "cyl_a"),
            "expected device symbol from demo.plc"
        );
        assert!(
            completion_items_for_analysis(Some(&analysis))
                .iter()
                .any(|item| item.label == "cyl_a"),
            "expected symbol completion"
        );
    }

    #[test]
    fn language_snapshot_exposes_symbols_and_snippet_completions() {
        let source = include_str!("../examples/demo.plc");
        let snapshot = language_snapshot_for_source(source);

        assert!(
            snapshot
                .symbols
                .iter()
                .any(|symbol| symbol.qualified_name == "cyl_a"),
            "expected symbol in language snapshot"
        );
        assert!(
            snapshot
                .completions
                .iter()
                .any(|completion| completion.label == "device block" && completion.snippet),
            "expected snippet completion in language snapshot"
        );
    }

    #[test]
    fn rename_ranges_match_whole_identifiers_only() {
        let ranges = word_ranges(
            "relation { from: cyl_a.extended, to: sensor_a.sense, via: detects }\ntask cycle:\n    on_complete: goto cycle_done\n",
            "cycle",
        );
        assert_eq!(ranges.len(), 1);
        let device_ranges = word_ranges(
            "relation { from: cyl_a.extended, to: sensor_a.sense, via: detects }\n",
            "cyl_a",
        );
        assert_eq!(device_ranges.len(), 1);
    }

    #[test]
    fn code_action_adds_missing_purpose_todo() {
        let source = "[topology]\n\ndevice sensor_a: sensor {\n    subtype: \"limit_switch\"\n}\n\n[constraints]\n\n[tasks]\n";
        let analysis = analyze_document(source);
        let diagnostic = analysis
            .diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code == Some(NumberOrString::String("SEM-107".to_string()))
            })
            .expect("missing purpose diagnostic");
        let context = CodeActionContext {
            diagnostics: vec![diagnostic.clone()],
            only: None,
            trigger_kind: None,
        };
        let actions = code_actions_for_range(
            source,
            Url::from_str("file:///test.plc").expect("valid url"),
            diagnostic.range,
            &context,
        );
        assert!(
            actions.iter().any(|action| match action {
                CodeActionOrCommand::CodeAction(action) => action.title == "Add TODO purpose field",
                CodeActionOrCommand::Command(_) => false,
            }),
            "expected missing purpose quick fix"
        );
    }

    #[test]
    fn range_formatting_trims_only_selected_lines() {
        let source = "a\t \nb  \nc  \n";
        let edits = format_range_edits(
            source,
            Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 0,
                },
            },
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "a\nb\n");
    }
}
