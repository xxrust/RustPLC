import React, { useEffect, useMemo, useRef, useState } from 'react';
import Editor, { loader, type BeforeMount, type Monaco, type OnMount } from '@monaco-editor/react';
import * as monacoEditor from 'monaco-editor/editor/editor.api';
import EditorWorker from 'monaco-editor/editor/editor.worker?worker';
import type { editor, languages, Position as MonacoPosition } from 'monaco-editor';
import { Alert, Button, Empty, Input, Space, Tag, Typography, message } from 'antd';
import {
  CheckCircleOutlined,
  CommentOutlined,
  ExclamationCircleOutlined,
  FileTextOutlined,
  ReloadOutlined,
  SendOutlined,
  TeamOutlined,
} from '@ant-design/icons';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { buildWebSocketUrl, dslApi, plcApi, projectApi } from '../services/api';
import { useAppStore } from '../stores/appStore';
import type {
  CollabEvent,
  PlcDiagnosticIssue,
  PlcDiagnosticsResponse,
  PlcLanguageSnapshot,
  PlcRealtimeAnalysisResponse,
} from '../types';

const { Text, Title } = Typography;

loader.config({ monaco: monacoEditor });

// The RustPLC editor only needs the generic editor worker. Keeping worker creation
// explicit avoids pulling every built-in Monaco language worker into the route.
(globalThis as typeof globalThis & {
  MonacoEnvironment?: { getWorker: () => Worker };
}).MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
};

const DEFAULT_PLC_SOURCE = `[topology]
device plc_main: plc {
    purpose: "PLC editor starter controller"
    model_ref: rp2040_softplc
}
device sensor_A: sensor {
    purpose: "Starter sensor"
}

[constraints]

[tasks]
task main:
    step idle:
        action: log "ready"
`;

const STAGE_ORDER = ['parse', 'topology_gate', 'preprocess', 'semantic', 'verification'];

function configureRustPlcLanguage(monaco: Monaco) {
  if (monaco.languages.getLanguages().some((language: { id: string }) => language.id === 'rustplc')) {
    return;
  }

  monaco.languages.register({ id: 'rustplc' });
  monaco.languages.setMonarchTokensProvider('rustplc', {
    tokenizer: {
      root: [
        [/^\s*\[(topology|constraints|tasks)\]/, 'keyword'],
        [/\b(device|relation|from|to|via|task|step|action|wait|timeout|delay|on_complete|goto|purpose|model_ref)\b/, 'keyword'],
        [/\b(true|false|on|off)\b/, 'constant'],
        [/"[^"]*"/, 'string'],
        [/#.*$/, 'comment'],
        [/\b\d+(ms|s|m)?\b/, 'number'],
      ],
    },
  });
  monaco.languages.setLanguageConfiguration('rustplc', {
    comments: { lineComment: '#' },
    brackets: [
      ['{', '}'],
      ['[', ']'],
      ['(', ')'],
    ],
    autoClosingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
    ],
  });
}

function markerSeverity(monaco: Monaco, severity: PlcDiagnosticIssue['severity']) {
  if (severity === 'error') return monaco.MarkerSeverity.Error;
  if (severity === 'warning') return monaco.MarkerSeverity.Warning;
  return monaco.MarkerSeverity.Info;
}

function applyEditorMarkers(
  monaco: Monaco | null,
  editorInstance: editor.IStandaloneCodeEditor | null,
  issues: PlcDiagnosticIssue[]
) {
  const model = editorInstance?.getModel();
  if (!monaco || !model) return;

  monaco.editor.setModelMarkers(
    model,
    'rustplc',
    issues.map((issue) => {
      const line = Math.max(1, issue.line || 1);
      const column = Math.max(1, issue.column || 1);
      return {
        severity: markerSeverity(monaco, issue.severity),
        message: [issue.code, issue.message, issue.suggestion].filter(Boolean).join('\n'),
        startLineNumber: line,
        startColumn: column,
        endLineNumber: line,
        endColumn: column + 1,
        code: issue.code,
        source: issue.stage,
      };
    })
  );
}

function completionKind(monaco: Monaco, kind: string) {
  if (kind.includes('keyword')) return monaco.languages.CompletionItemKind.Keyword;
  if (kind.includes('snippet')) return monaco.languages.CompletionItemKind.Snippet;
  if (kind.includes('class')) return monaco.languages.CompletionItemKind.Class;
  if (kind.includes('module')) return monaco.languages.CompletionItemKind.Module;
  if (kind.includes('field')) return monaco.languages.CompletionItemKind.Field;
  if (kind.includes('variable')) return monaco.languages.CompletionItemKind.Variable;
  if (kind.includes('struct')) return monaco.languages.CompletionItemKind.Struct;
  return monaco.languages.CompletionItemKind.Text;
}

function lookupSymbol(snapshot: PlcLanguageSnapshot | null, word: string) {
  return snapshot?.symbols.find((symbol) => symbol.qualified_name === word || symbol.name === word) ?? null;
}

function registerLanguageProviders(
  monaco: Monaco,
  snapshotRef: React.MutableRefObject<PlcLanguageSnapshot | null>
) {
  const completionProvider = monaco.languages.registerCompletionItemProvider('rustplc', {
    triggerCharacters: ['.', ':', ' '],
    provideCompletionItems: (model: editor.ITextModel, position: MonacoPosition): languages.ProviderResult<languages.CompletionList> => {
      const word = model.getWordUntilPosition(position);
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };
      return {
        suggestions:
          snapshotRef.current?.completions.map((item) => ({
            label: item.label,
            kind: completionKind(monaco, item.kind),
            detail: item.detail,
            documentation: item.documentation,
            insertText: item.insert_text || item.label,
            insertTextRules: item.snippet ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet : undefined,
            range,
          })) || [],
      };
    },
  });

  const hoverProvider = monaco.languages.registerHoverProvider('rustplc', {
    provideHover: (model: editor.ITextModel, position: MonacoPosition): languages.ProviderResult<languages.Hover> => {
      const word = model.getWordAtPosition(position);
      if (!word) return null;
      const symbol = lookupSymbol(snapshotRef.current, word.word);
      if (!symbol) return null;
      return {
        range: new monaco.Range(position.lineNumber, word.startColumn, position.lineNumber, word.endColumn),
        contents: [{ value: symbol.documentation }],
      };
    },
  });

  const definitionProvider = monaco.languages.registerDefinitionProvider('rustplc', {
    provideDefinition: (model: editor.ITextModel, position: MonacoPosition): languages.ProviderResult<languages.Definition> => {
      const word = model.getWordAtPosition(position);
      if (!word) return null;
      const symbol = lookupSymbol(snapshotRef.current, word.word);
      if (!symbol) return null;
      return {
        uri: model.uri,
        range: new monaco.Range(symbol.line, 1, symbol.line, 1),
      };
    },
  });

  const referenceProvider = monaco.languages.registerReferenceProvider('rustplc', {
    provideReferences: (model: editor.ITextModel, position: MonacoPosition): languages.ProviderResult<languages.Location[]> => {
      const word = model.getWordAtPosition(position);
      if (!word) return [];
      return model
        .findMatches(word.word, false, false, true, null, true)
        .map((match: editor.FindMatch) => ({
          uri: model.uri,
          range: match.range,
        }));
    },
  });

  return [completionProvider, hoverProvider, definitionProvider, referenceProvider];
}

function stageRank(stage: string) {
  const index = STAGE_ORDER.indexOf(stage);
  return index === -1 ? STAGE_ORDER.length : index;
}

function safeCollabRoom(projectId: string | null): string {
  const raw = projectId || 'editor_buffer';
  const normalized = raw.replace(/[^A-Za-z0-9_.-]/g, '_').slice(0, 96);
  return normalized || 'editor_buffer';
}

const Metric: React.FC<{ title: string; value: number; color: string }> = ({ title, value, color }) => (
  <div>
    <div style={{ color: '#a0a0a0', fontSize: 12, marginBottom: 4 }}>{title}</div>
    <div style={{ color, fontSize: 18, fontWeight: 600, lineHeight: 1.2 }}>{value}</div>
  </div>
);

function capabilityStatusColor(status: string) {
  const normalized = status.toLowerCase();
  if (normalized.includes('unsupported') || normalized.includes('blocked')) return 'red';
  if (normalized.includes('partial') || normalized.includes('experimental')) return 'gold';
  if (normalized.includes('supported') || normalized.includes('stable')) return 'green';
  return 'default';
}

const PlcEditorPage: React.FC = () => {
  const { t } = useTranslation();
  const { currentProject, currentProjectContent, setCurrentProject } = useAppStore();
  const [source, setSource] = useState(currentProjectContent || DEFAULT_PLC_SOURCE);
  const [dirty, setDirty] = useState(false);
  const [diagnostics, setDiagnostics] = useState<PlcDiagnosticsResponse | null>(null);
  const [languageSnapshot, setLanguageSnapshot] = useState<PlcLanguageSnapshot | null>(null);
  const [isNarrow, setIsNarrow] = useState(() => window.innerWidth < 920);
  const [realtimeConnected, setRealtimeConnected] = useState(false);
  const [collabConnected, setCollabConnected] = useState(false);
  const [collabPeers, setCollabPeers] = useState<Record<string, CollabEvent>>({});
  const [lastRemoteEdit, setLastRemoteEdit] = useState<CollabEvent | null>(null);
  const [collabComments, setCollabComments] = useState<CollabEvent[]>([]);
  const [commentDraft, setCommentDraft] = useState('');
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const languageSnapshotRef = useRef<PlcLanguageSnapshot | null>(null);
  const languageProviderDisposablesRef = useRef<Array<{ dispose: () => void }>>([]);
  const cursorDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const realtimeWsRef = useRef<WebSocket | null>(null);
  const collabWsRef = useRef<WebSocket | null>(null);
  const realtimeRequestIdRef = useRef(0);
  const latestAcceptedRequestIdRef = useRef(0);
  const collabClientIdRef = useRef(`web-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  const collabRevisionRef = useRef(0);
  const applyingRemoteEditRef = useRef(false);

  const sourceQuery = useQuery({
    queryKey: ['project-source', currentProject],
    queryFn: () => projectApi.getProjectSource(currentProject || ''),
    enabled: Boolean(currentProject && !currentProjectContent),
    retry: false,
  });

  const capabilitiesQuery = useQuery({
    queryKey: ['dsl-capabilities'],
    queryFn: () => dslApi.getCapabilities(),
    staleTime: 5 * 60 * 1000,
    retry: false,
  });

  useEffect(() => {
    const handleResize = () => setIsNarrow(window.innerWidth < 920);
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  // The editor buffer mirrors the selected project or fetched source payload.
  useEffect(() => {
    if (currentProjectContent) {
      setSource(currentProjectContent);
      setDirty(false);
      return;
    }
    if (sourceQuery.data?.data.content) {
      setSource(sourceQuery.data.data.content);
      setDirty(false);
    }
  }, [currentProjectContent, sourceQuery.data]);

  const diagnosticsMutation = useMutation({
    mutationFn: (content: string) => plcApi.getDiagnostics(content),
    onSuccess: (response) => {
      setDiagnostics(response.data);
      applyEditorMarkers(monacoRef.current, editorRef.current, response.data.issues);
    },
    onError: () => {
      message.error(t('plcEditor.diagnosticsFailed'));
    },
  });

  const languageMutation = useMutation({
    mutationFn: (content: string) => plcApi.getLanguageSnapshot(content),
    onSuccess: (response) => {
      setLanguageSnapshot(response.data);
    },
  });

  useEffect(() => {
    languageSnapshotRef.current = languageSnapshot;
  }, [languageSnapshot]);

  const applyRealtimeAnalysis = (payload: PlcRealtimeAnalysisResponse) => {
    if (payload.request_id && payload.request_id < latestAcceptedRequestIdRef.current) {
      return;
    }
    latestAcceptedRequestIdRef.current = payload.request_id || latestAcceptedRequestIdRef.current;
    setDiagnostics(payload.diagnostics);
    setLanguageSnapshot(payload.language);
    applyEditorMarkers(monacoRef.current, editorRef.current, payload.diagnostics.issues);
  };

  const sendCollabEvent = (event: Partial<CollabEvent> & { kind: string }) => {
    const ws = collabWsRef.current;
    if (ws?.readyState !== WebSocket.OPEN) {
      return;
    }
    ws.send(JSON.stringify({
      kind: event.kind,
      client_id: collabClientIdRef.current,
      user_name: t('user.defaultName'),
      content: event.content,
      revision: event.revision,
      cursor_line: event.cursor_line,
      cursor_column: event.cursor_column,
      comment: event.comment,
    }));
  };

  const appendCollabComment = (event: CollabEvent) => {
    setCollabComments((prev) => [...prev, event].slice(-50));
  };

  const runHttpEditorAnalysis = (content: string) => {
    diagnosticsMutation.mutate(content);
    languageMutation.mutate(content);
  };

  const runEditorAnalysis = (content: string) => {
    const ws = realtimeWsRef.current;
    if (ws?.readyState === WebSocket.OPEN) {
      const requestId = realtimeRequestIdRef.current + 1;
      realtimeRequestIdRef.current = requestId;
      latestAcceptedRequestIdRef.current = requestId;
      ws.send(JSON.stringify({ content, request_id: requestId }));
      return;
    }
    runHttpEditorAnalysis(content);
  };

  useEffect(() => {
    let ws: WebSocket | null = null;
    const connectTimer = window.setTimeout(() => {
      ws = new WebSocket(buildWebSocketUrl('/ws/plc'));
      realtimeWsRef.current = ws;

      ws.onopen = () => {
        setRealtimeConnected(true);
        if (source.trim()) {
          const requestId = realtimeRequestIdRef.current + 1;
          realtimeRequestIdRef.current = requestId;
          latestAcceptedRequestIdRef.current = requestId;
          ws?.send(JSON.stringify({ content: source, request_id: requestId }));
        }
      };

      ws.onmessage = (event) => {
        try {
          applyRealtimeAnalysis(JSON.parse(event.data) as PlcRealtimeAnalysisResponse);
        } catch {
          runHttpEditorAnalysis(source);
        }
      };

      ws.onerror = () => {
        setRealtimeConnected(false);
        runHttpEditorAnalysis(source);
      };

      ws.onclose = () => {
        setRealtimeConnected(false);
        if (realtimeWsRef.current === ws) {
          realtimeWsRef.current = null;
        }
      };
    }, 0);

    return () => {
      window.clearTimeout(connectTimer);
      if (ws && realtimeWsRef.current === ws) {
        realtimeWsRef.current = null;
      }
      if (ws?.readyState === WebSocket.OPEN) {
        ws.close();
      }
    };
    // The socket is session-scoped for this page; source changes are sent by runEditorAnalysis.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const room = safeCollabRoom(currentProject);
    const ws = new WebSocket(buildWebSocketUrl(`/ws/collab/${room}`));
    collabWsRef.current = ws;
    setCollabConnected(false);
    setCollabPeers({});
    setLastRemoteEdit(null);
    setCollabComments([]);

    ws.onopen = () => {
      setCollabConnected(true);
      sendCollabEvent({ kind: 'hello', revision: collabRevisionRef.current });
    };

    ws.onmessage = (event) => {
      try {
        const payload = JSON.parse(event.data) as CollabEvent;
        if (payload.client_id === collabClientIdRef.current) {
          return;
        }
        setCollabPeers((prev) => ({ ...prev, [payload.client_id]: payload }));
        if (payload.kind === 'edit' && typeof payload.content === 'string') {
          applyingRemoteEditRef.current = true;
          setSource(payload.content);
          setDirty(true);
          setLastRemoteEdit(payload);
          window.setTimeout(() => {
            applyingRemoteEditRef.current = false;
          }, 0);
        } else if (payload.kind === 'comment' && payload.comment?.trim()) {
          appendCollabComment(payload);
        }
      } catch {
        setCollabConnected(false);
      }
    };

    ws.onerror = () => setCollabConnected(false);
    ws.onclose = () => {
      setCollabConnected(false);
      if (collabWsRef.current === ws) {
        collabWsRef.current = null;
      }
    };

    return () => {
      if (collabWsRef.current === ws) {
        collabWsRef.current = null;
      }
      if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
        ws.close();
      }
    };
    // Reconnect only when the collaboration room changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentProject]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      if (source.trim()) {
        runEditorAnalysis(source);
      }
    }, 450);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source]);

  useEffect(() => {
    applyEditorMarkers(monacoRef.current, editorRef.current, diagnostics?.issues || []);
  }, [diagnostics]);

  const beforeMount: BeforeMount = (monaco) => {
    configureRustPlcLanguage(monaco);
  };

  const handleMount: OnMount = (editorInstance, monaco) => {
    editorRef.current = editorInstance;
    monacoRef.current = monaco;
    languageProviderDisposablesRef.current.forEach((disposable) => disposable.dispose());
    languageProviderDisposablesRef.current = registerLanguageProviders(monaco, languageSnapshotRef);
    cursorDisposableRef.current?.dispose();
    cursorDisposableRef.current = editorInstance.onDidChangeCursorPosition((event) => {
      sendCollabEvent({
        kind: 'cursor',
        cursor_line: event.position.lineNumber,
        cursor_column: event.position.column,
        revision: collabRevisionRef.current,
      });
    });
    applyEditorMarkers(monaco, editorInstance, diagnostics?.issues || []);
  };

  useEffect(
    () => () => {
      languageProviderDisposablesRef.current.forEach((disposable) => disposable.dispose());
      languageProviderDisposablesRef.current = [];
      cursorDisposableRef.current?.dispose();
      cursorDisposableRef.current = null;
    },
    []
  );

  const sortedIssues = useMemo(
    () =>
      [...(diagnostics?.issues || [])].sort((a, b) => {
        const stageDelta = stageRank(a.stage) - stageRank(b.stage);
        if (stageDelta !== 0) return stageDelta;
        return (a.line || 0) - (b.line || 0);
      }),
    [diagnostics]
  );

  const handleUseAsProjectSource = () => {
    const projectId = currentProject || 'editor_buffer';
    setCurrentProject(projectId, null, source);
    setDirty(false);
    message.success(t('plcEditor.bufferStored'));
  };

  const handleSendComment = () => {
    const text = commentDraft.trim();
    if (!text) {
      return;
    }
    const position = editorRef.current?.getPosition();
    collabRevisionRef.current += 1;
    const localEvent: CollabEvent = {
      room: safeCollabRoom(currentProject),
      kind: 'comment',
      client_id: collabClientIdRef.current,
      user_name: t('user.defaultName'),
      revision: collabRevisionRef.current,
      cursor_line: position?.lineNumber,
      cursor_column: position?.column,
      comment: text,
      at_ms: Date.now(),
    };
    appendCollabComment(localEvent);
    sendCollabEvent(localEvent);
    setCommentDraft('');
  };

  const handleIssueClick = (issue: PlcDiagnosticIssue) => {
    const line = Math.max(1, issue.line || 1);
    const column = Math.max(1, issue.column || 1);
    editorRef.current?.revealPositionInCenter({ lineNumber: line, column });
    editorRef.current?.setPosition({ lineNumber: line, column });
    editorRef.current?.focus();
  };

  const valid = diagnostics?.valid ?? false;
  const errorCount = sortedIssues.filter((issue) => issue.severity === 'error').length;
  const warningCount = sortedIssues.filter((issue) => issue.severity === 'warning').length;
  const collabPeerCount = Object.keys(collabPeers).length;
  const capabilityReport = capabilitiesQuery.data?.data ?? null;
  const unsupportedCapabilities = capabilityReport?.unsupported_features.slice(0, 3) ?? [];

  return (
    <div
      style={{
        minHeight: '100%',
        display: 'grid',
        gridTemplateRows: 'auto minmax(0, 1fr)',
        background: '#1e1e1e',
        color: '#e0e0e0',
      }}
    >
      <div
        style={{
          padding: '16px 18px',
          borderBottom: '1px solid #3a3a3a',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          flexWrap: 'wrap',
          gap: 16,
        }}
      >
        <div>
          <Title level={3} style={{ color: '#f5f5f5', margin: 0, fontSize: 18 }}>
            {t('plcEditor.title')}
          </Title>
          <Text style={{ color: '#a0a0a0', fontSize: 12 }}>
            {currentProject ? `${t('common.currentProject')}: ${currentProject}` : t('common.noneSelected')}
            {dirty ? ` · ${t('topBar.unsavedChanges')}` : ''}
            {` · ${realtimeConnected ? t('plcEditor.analysisRealtime') : t('plcEditor.analysisHttpFallback')}`}
          </Text>
        </div>
        <Space>
          <Tag icon={<TeamOutlined />} color={collabConnected ? 'green' : 'default'}>
            {collabConnected
              ? t('plcEditor.collabConnected', { count: collabPeerCount })
              : t('plcEditor.collabOffline')}
          </Tag>
          {lastRemoteEdit && (
            <Tag color="blue">
              {t('plcEditor.collabLastEdit', {
                user: lastRemoteEdit.user_name || lastRemoteEdit.client_id,
              })}
            </Tag>
          )}
          <Button
            icon={<ReloadOutlined />}
            onClick={() => runEditorAnalysis(source)}
            loading={diagnosticsMutation.isPending || languageMutation.isPending}
          >
            {t('plcEditor.validate')}
          </Button>
          <Button type="primary" icon={<FileTextOutlined />} onClick={handleUseAsProjectSource}>
            {t('plcEditor.useBuffer')}
          </Button>
        </Space>
      </div>

      <div
        style={{
          minHeight: 0,
          display: 'grid',
          gridTemplateColumns: isNarrow ? 'minmax(0, 1fr)' : 'minmax(0, 1fr) minmax(320px, 360px)',
          gridTemplateRows: isNarrow ? 'minmax(420px, 1fr) minmax(260px, 42vh)' : undefined,
        }}
      >
        <div style={{ minWidth: 0, minHeight: 0 }}>
          <Editor
            height="100%"
            language="rustplc"
            theme="vs-dark"
            value={source}
            beforeMount={beforeMount}
            onMount={handleMount}
            onChange={(value) => {
              const nextSource = value || '';
              setSource(nextSource);
              setDirty(true);
              if (!applyingRemoteEditRef.current) {
                collabRevisionRef.current += 1;
                sendCollabEvent({
                  kind: 'edit',
                  content: nextSource,
                  revision: collabRevisionRef.current,
                });
              }
            }}
            options={{
              minimap: { enabled: false },
              fontSize: 13,
              fontFamily: 'JetBrains Mono, Fira Code, Consolas, monospace',
              tabSize: 4,
              insertSpaces: true,
              scrollBeyondLastLine: false,
              automaticLayout: true,
              renderLineHighlight: 'line',
              fixedOverflowWidgets: true,
            }}
          />
        </div>

        <aside
          style={{
            minHeight: 0,
            borderLeft: isNarrow ? 'none' : '1px solid #3a3a3a',
            borderTop: isNarrow ? '1px solid #3a3a3a' : 'none',
            background: '#252525',
            display: 'grid',
            gridTemplateRows: 'auto minmax(0, 1fr)',
          }}
        >
          <div style={{ padding: 16, borderBottom: '1px solid #3a3a3a' }}>
            <Alert
              type={valid ? 'success' : errorCount > 0 ? 'error' : 'info'}
              showIcon
              icon={valid ? <CheckCircleOutlined /> : <ExclamationCircleOutlined />}
              title={valid ? t('plcEditor.valid') : t('plcEditor.notValid')}
              description={diagnostics ? `${t('plcEditor.stage')}: ${diagnostics.stage}` : t('plcEditor.awaitingDiagnostics')}
            />
            <div
              style={{
                marginTop: 14,
                display: 'grid',
                gridTemplateColumns: 'repeat(3, 1fr)',
                gap: 8,
              }}
            >
              <Metric title={t('plcEditor.errors')} value={errorCount} color="#ff7875" />
              <Metric title={t('plcEditor.warnings')} value={warningCount} color="#ffc53d" />
              <Metric title={t('plcEditor.states')} value={diagnostics?.summary.states ?? 0} color="#e0e0e0" />
            </div>
          </div>

          <div style={{ overflow: 'auto', padding: 12 }}>
            <Space orientation="vertical" size={12} style={{ width: '100%' }}>
              {sortedIssues.length === 0 ? (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description={diagnostics ? t('plcEditor.noIssues') : t('plcEditor.noDiagnostics')}
                />
              ) : (
                <Space orientation="vertical" size={8} style={{ width: '100%' }}>
                  {sortedIssues.map((issue, index) => (
                    <button
                      key={`${issue.stage}-${issue.line}-${issue.column}-${index}`}
                      onClick={() => handleIssueClick(issue)}
                      style={{
                        width: '100%',
                        textAlign: 'left',
                        background: '#1e1e1e',
                        border: '1px solid #3a3a3a',
                        borderRadius: 6,
                        padding: 10,
                        color: '#e0e0e0',
                        cursor: 'pointer',
                      }}
                    >
                      <Space size={6} wrap>
                        <Tag color={issue.severity === 'error' ? 'red' : issue.severity === 'warning' ? 'gold' : 'blue'}>
                          {issue.severity}
                        </Tag>
                        <Tag>{issue.stage}</Tag>
                        {issue.code && <Tag color="cyan">{issue.code}</Tag>}
                        <Text style={{ color: '#a0a0a0', fontSize: 12 }}>
                          {issue.line}:{issue.column}
                        </Text>
                      </Space>
                      <div style={{ marginTop: 8, fontSize: 13, lineHeight: 1.45 }}>
                        {issue.message}
                      </div>
                      {issue.suggestion && (
                        <div style={{ marginTop: 8, color: '#b7eb8f', fontSize: 12, lineHeight: 1.4 }}>
                          {issue.suggestion}
                        </div>
                      )}
                    </button>
                  ))}
                </Space>
              )}

              <div
                style={{
                  borderTop: '1px solid #3a3a3a',
                  paddingTop: 12,
                }}
              >
                <Space align="center" style={{ width: '100%', justifyContent: 'space-between', marginBottom: 10 }}>
                  <Text style={{ color: '#f5f5f5', fontWeight: 600 }}>
                    <FileTextOutlined /> {t('plcEditor.dslCapabilityContract')}
                  </Text>
                  <Tag color={capabilitiesQuery.isError ? 'red' : capabilityReport ? 'green' : 'default'}>
                    {capabilitiesQuery.isLoading
                      ? t('common.loading')
                      : capabilityReport
                        ? capabilityReport.parser_contract
                        : t('common.error')}
                  </Tag>
                </Space>
                {capabilitiesQuery.isError ? (
                  <Alert type="warning" showIcon message={t('plcEditor.dslCapabilityUnavailable')} />
                ) : capabilityReport ? (
                  <Space orientation="vertical" size={8} style={{ width: '100%' }}>
                    <div
                      style={{
                        display: 'grid',
                        gridTemplateColumns: 'repeat(3, 1fr)',
                        gap: 8,
                      }}
                    >
                      <Metric
                        title={t('plcEditor.dslSupported')}
                        value={capabilityReport.supported_features.length}
                        color="#95de64"
                      />
                      <Metric
                        title={t('plcEditor.dslUnsupported')}
                        value={capabilityReport.unsupported_features.length}
                        color="#ff7875"
                      />
                      <Metric
                        title={t('plcEditor.dslTemplates')}
                        value={capabilityReport.template_assets.length}
                        color="#69c0ff"
                      />
                    </div>
                    {unsupportedCapabilities.length > 0 && (
                      <Space orientation="vertical" size={6} style={{ width: '100%' }}>
                        <Text style={{ color: '#a0a0a0', fontSize: 12 }}>
                          {t('plcEditor.dslUnsupportedTitle')}
                        </Text>
                        {unsupportedCapabilities.map((feature) => (
                          <div
                            key={feature.id}
                            style={{
                              background: '#1e1e1e',
                              border: '1px solid #3a3a3a',
                              borderRadius: 6,
                              padding: 8,
                            }}
                          >
                            <Space size={6} wrap>
                              <Tag color={capabilityStatusColor(feature.status)}>{feature.status}</Tag>
                              <Text style={{ color: '#e0e0e0', fontSize: 12 }}>{feature.id}</Text>
                            </Space>
                            <div style={{ marginTop: 6, color: '#a0a0a0', fontSize: 12, lineHeight: 1.4 }}>
                              {feature.required_contract}
                            </div>
                          </div>
                        ))}
                      </Space>
                    )}
                  </Space>
                ) : (
                  <Text style={{ color: '#a0a0a0', fontSize: 12 }}>{t('plcEditor.dslCapabilityLoading')}</Text>
                )}
              </div>

              <div
                style={{
                  borderTop: '1px solid #3a3a3a',
                  paddingTop: 12,
                }}
              >
                <Space align="center" style={{ width: '100%', justifyContent: 'space-between', marginBottom: 10 }}>
                  <Text style={{ color: '#f5f5f5', fontWeight: 600 }}>
                    <CommentOutlined /> {t('plcEditor.comments')}
                  </Text>
                  <Tag color={collabConnected ? 'green' : 'default'}>
                    {collabConnected ? t('plcEditor.live') : t('plcEditor.offline')}
                  </Tag>
                </Space>
                <Input.TextArea
                  id="plc-comment-draft"
                  name="plc-comment-draft"
                  value={commentDraft}
                  onChange={(event) => setCommentDraft(event.target.value)}
                  onPressEnter={(event) => {
                    if (!event.shiftKey) {
                      event.preventDefault();
                      handleSendComment();
                    }
                  }}
                  placeholder={t('plcEditor.commentPlaceholder')}
                  autoSize={{ minRows: 2, maxRows: 4 }}
                  style={{
                    background: '#1e1e1e',
                    borderColor: '#3a3a3a',
                    color: '#e0e0e0',
                  }}
                />
                <Button
                  type="primary"
                  icon={<SendOutlined />}
                  onClick={handleSendComment}
                  disabled={!commentDraft.trim()}
                  style={{ marginTop: 8, width: '100%' }}
                >
                  {t('plcEditor.sendComment')}
                </Button>
                <div style={{ marginTop: 12 }}>
                  {collabComments.length === 0 ? (
                    <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('plcEditor.noComments')} />
                  ) : (
                    <Space orientation="vertical" size={8} style={{ width: '100%' }}>
                      {collabComments.map((comment, index) => (
                        <div
                          key={`${comment.client_id}-${comment.at_ms}-${index}`}
                          style={{
                            background: '#1e1e1e',
                            border: '1px solid #3a3a3a',
                            borderRadius: 6,
                            padding: 10,
                          }}
                        >
                          <Space size={6} wrap>
                            <Tag color={comment.client_id === collabClientIdRef.current ? 'blue' : 'purple'}>
                              {comment.user_name || comment.client_id}
                            </Tag>
                            {comment.cursor_line && (
                              <button
                                type="button"
                                onClick={() => {
                                  const lineNumber = Math.max(1, comment.cursor_line || 1);
                                  const column = Math.max(1, comment.cursor_column || 1);
                                  editorRef.current?.revealPositionInCenter({ lineNumber, column });
                                  editorRef.current?.setPosition({ lineNumber, column });
                                  editorRef.current?.focus();
                                }}
                                style={{
                                  border: '1px solid #3a3a3a',
                                  borderRadius: 4,
                                  background: '#252525',
                                  color: '#a0a0a0',
                                  fontSize: 12,
                                  cursor: 'pointer',
                                  padding: '1px 6px',
                                }}
                              >
                                {comment.cursor_line}:{comment.cursor_column || 1}
                              </button>
                            )}
                          </Space>
                          <div style={{ marginTop: 8, fontSize: 13, lineHeight: 1.45, whiteSpace: 'pre-wrap' }}>
                            {comment.comment}
                          </div>
                        </div>
                      ))}
                    </Space>
                  )}
                </div>
              </div>
            </Space>
          </div>
        </aside>
      </div>
    </div>
  );
};

export default PlcEditorPage;
