import React, { useMemo, useState } from 'react';
import type { Edge, Node } from '@xyflow/react';
import { useTranslation } from 'react-i18next';
import { topologyApi } from '../../services/api';
import { useAppStore } from '../../stores/appStore';
import { useTopologyStore } from '../../stores/topologyStore';
import type { NodeData } from '../../stores/topologyStore';
import type { TagDimension } from '../../types';
import {
  downloadTopologyAsJson,
  toComponentTopology,
} from '../../utils/topologySerialization';

type EdgeScope = 'touched' | 'internal';
type EdgeSignalMode = 'no_change' | 'set' | 'clear';

interface BatchDraft {
  filterDimension: TagDimension;
  filterQuery: string;
  nodePatchJson: string;
  renamePrefix: string;
  renameSuffix: string;
  renameSearch: string;
  renameReplace: string;
  edgeScope: EdgeScope;
  edgeSignalMode: EdgeSignalMode;
  edgeSignalValue: string;
}

interface NodeChangePreview {
  nodeId: string;
  beforeLabel: string;
  afterLabel: string;
  changedKeys: string[];
}

interface EdgeChangePreview {
  edgeId: string;
  from: string;
  to: string;
  beforeSignal?: string;
  afterSignal?: string;
}

interface BatchPreview {
  matchedNodeIds: string[];
  changedNodes: NodeChangePreview[];
  changedEdges: EdgeChangePreview[];
  nextNodes: Array<Node<NodeData>>;
  nextEdges: Edge[];
}

interface TopologySnapshot {
  nodes: Array<Node<NodeData>>;
  edges: Edge[];
}

const PREVIEW_ITEM_LIMIT = 6;

const DEFAULT_DRAFT: BatchDraft = {
  filterDimension: 'functional_group',
  filterQuery: '',
  nodePatchJson: '{}',
  renamePrefix: '',
  renameSuffix: '',
  renameSearch: '',
  renameReplace: '',
  edgeScope: 'touched',
  edgeSignalMode: 'no_change',
  edgeSignalValue: '',
};

const TagBatchEditor: React.FC = () => {
  const { t } = useTranslation();
  const { currentProject } = useAppStore();
  const {
    nodes,
    edges,
    findNodeIdsByTag,
    findNodeIdsByLocationPath,
    replaceTopology,
    setHasUnsavedChanges,
  } = useTopologyStore();

  const [draft, setDraft] = useState<BatchDraft>(DEFAULT_DRAFT);
  const [preview, setPreview] = useState<BatchPreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [lastSnapshot, setLastSnapshot] = useState<TopologySnapshot | null>(null);
  const [writeBackMessage, setWriteBackMessage] = useState<string | null>(null);
  const [isWritingBack, setIsWritingBack] = useState(false);

  const canApplyPreview = Boolean(
    preview &&
      (preview.changedNodes.length > 0 || preview.changedEdges.length > 0)
  );

  const matchedNodeHint = useMemo(() => {
    const query = draft.filterQuery.trim();
    if (!query) {
      return t('properties.batchFilterHintEmpty');
    }
    const ids = resolveMatchedNodeIds(
      query,
      draft.filterDimension,
      nodes,
      findNodeIdsByTag,
      findNodeIdsByLocationPath
    );
    return t('properties.batchFilterHint', { count: ids.length });
  }, [
    draft.filterDimension,
    draft.filterQuery,
    findNodeIdsByLocationPath,
    findNodeIdsByTag,
    nodes,
    t,
  ]);

  const updateDraft = <K extends keyof BatchDraft>(
    key: K,
    value: BatchDraft[K]
  ) => {
    setDraft((prev) => ({ ...prev, [key]: value }));
    setPreview(null);
    setPreviewError(null);
  };

  const handlePreview = () => {
    try {
      setWriteBackMessage(null);
      const nextPreview = buildPreview({
        draft,
        nodes,
        edges,
        findNodeIdsByTag,
        findNodeIdsByLocationPath,
      });
      setPreview(nextPreview);
      setPreviewError(null);
    } catch (error) {
      const message = error instanceof Error ? error.message : t('properties.batchPreviewError');
      setPreview(null);
      setPreviewError(message);
    }
  };

  const handleApply = () => {
    if (!preview) {
      return;
    }

    setLastSnapshot({
      nodes: cloneNodes(nodes),
      edges: cloneEdges(edges),
    });
    replaceTopology(preview.nextNodes, preview.nextEdges, true);
    setPreview(null);
    setPreviewError(null);
    setWriteBackMessage(t('properties.batchApplySuccess'));
  };

  const handleRollback = () => {
    if (!lastSnapshot) {
      return;
    }

    replaceTopology(lastSnapshot.nodes, lastSnapshot.edges, true);
    setPreview(null);
    setPreviewError(null);
    setWriteBackMessage(t('properties.batchRollbackSuccess'));
    setLastSnapshot(null);
  };

  const handleExport = () => {
    const topology = toComponentTopology(nodes, edges);
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    downloadTopologyAsJson(topology, `topology-${timestamp}.json`);
    setWriteBackMessage(t('properties.batchExportSuccess'));
  };

  const handleWriteBack = async () => {
    if (!currentProject) {
      setWriteBackMessage(t('properties.batchWriteBackNeedProject'));
      return;
    }

    try {
      setIsWritingBack(true);
      const topology = toComponentTopology(nodes, edges);
      await topologyApi.saveTopology(currentProject, topology);
      setHasUnsavedChanges(false);
      setWriteBackMessage(t('properties.batchWriteBackSuccess'));
    } catch {
      setWriteBackMessage(t('properties.batchWriteBackFailed'));
    } finally {
      setIsWritingBack(false);
    }
  };

  return (
    <div style={{ marginBottom: 16 }}>
      <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>
        {t('properties.batchTitle')}
      </div>
      <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12 }}>
        <div style={{ marginBottom: 8 }}>
          <label style={labelStyle}>{t('properties.batchDimension')}</label>
          <select
            value={draft.filterDimension}
            onChange={(event) =>
              updateDraft('filterDimension', event.target.value as TagDimension)
            }
            style={inputStyle}
          >
            <option value="functional_group">functional_group</option>
            <option value="danger_level">danger_level</option>
            <option value="location_group">location_group</option>
          </select>
        </div>

        <div style={{ marginBottom: 8 }}>
          <label style={labelStyle}>{t('properties.batchFilter')}</label>
          <input
            type="text"
            value={draft.filterQuery}
            onChange={(event) => updateDraft('filterQuery', event.target.value)}
            placeholder={t('properties.batchFilterPlaceholder')}
            style={inputStyle}
          />
          <div style={{ marginTop: 4, fontSize: 10, color: '#7a7a7a' }}>{matchedNodeHint}</div>
        </div>

        <div style={{ marginBottom: 8 }}>
          <label style={labelStyle}>{t('properties.batchNodePatch')}</label>
          <textarea
            rows={3}
            value={draft.nodePatchJson}
            onChange={(event) => updateDraft('nodePatchJson', event.target.value)}
            style={{ ...inputStyle, fontFamily: 'monospace', resize: 'vertical' }}
          />
        </div>

        <div style={{ marginBottom: 8 }}>
          <label style={labelStyle}>{t('properties.batchRename')}</label>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
            <input
              type="text"
              value={draft.renamePrefix}
              onChange={(event) => updateDraft('renamePrefix', event.target.value)}
              placeholder={t('properties.batchRenamePrefix')}
              style={inputStyle}
            />
            <input
              type="text"
              value={draft.renameSuffix}
              onChange={(event) => updateDraft('renameSuffix', event.target.value)}
              placeholder={t('properties.batchRenameSuffix')}
              style={inputStyle}
            />
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6, marginTop: 6 }}>
            <input
              type="text"
              value={draft.renameSearch}
              onChange={(event) => updateDraft('renameSearch', event.target.value)}
              placeholder={t('properties.batchRenameSearch')}
              style={inputStyle}
            />
            <input
              type="text"
              value={draft.renameReplace}
              onChange={(event) => updateDraft('renameReplace', event.target.value)}
              placeholder={t('properties.batchRenameReplace')}
              style={inputStyle}
            />
          </div>
        </div>

        <div style={{ marginBottom: 12 }}>
          <label style={labelStyle}>{t('properties.batchEdgeUpdate')}</label>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
            <select
              value={draft.edgeScope}
              onChange={(event) =>
                updateDraft('edgeScope', event.target.value as EdgeScope)
              }
              style={inputStyle}
            >
              <option value="touched">{t('properties.batchEdgeScopeTouched')}</option>
              <option value="internal">{t('properties.batchEdgeScopeInternal')}</option>
            </select>
            <select
              value={draft.edgeSignalMode}
              onChange={(event) =>
                updateDraft('edgeSignalMode', event.target.value as EdgeSignalMode)
              }
              style={inputStyle}
            >
              <option value="no_change">{t('properties.batchEdgeSignalKeep')}</option>
              <option value="set">{t('properties.batchEdgeSignalSet')}</option>
              <option value="clear">{t('properties.batchEdgeSignalClear')}</option>
            </select>
          </div>
          {draft.edgeSignalMode === 'set' && (
            <input
              type="text"
              value={draft.edgeSignalValue}
              onChange={(event) =>
                updateDraft('edgeSignalValue', event.target.value)
              }
              placeholder={t('properties.batchEdgeSignalPlaceholder')}
              style={{ ...inputStyle, marginTop: 6 }}
            />
          )}
        </div>

        <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
          <button onClick={handlePreview} style={primaryButtonStyle}>
            {t('properties.batchPreview')}
          </button>
          <button
            onClick={handleApply}
            disabled={!canApplyPreview}
            style={{
              ...secondaryButtonStyle,
              opacity: canApplyPreview ? 1 : 0.5,
              cursor: canApplyPreview ? 'pointer' : 'not-allowed',
            }}
          >
            {t('properties.batchApply')}
          </button>
        </div>

        <div style={{ display: 'flex', gap: 8, marginBottom: 8 }}>
          <button
            onClick={handleRollback}
            disabled={!lastSnapshot}
            style={{
              ...secondaryButtonStyle,
              opacity: lastSnapshot ? 1 : 0.5,
              cursor: lastSnapshot ? 'pointer' : 'not-allowed',
            }}
          >
            {t('properties.batchRollback')}
          </button>
          <button onClick={handleExport} style={secondaryButtonStyle}>
            {t('properties.batchExport')}
          </button>
          <button
            onClick={handleWriteBack}
            disabled={isWritingBack}
            style={secondaryButtonStyle}
          >
            {isWritingBack
              ? t('properties.batchWriteBackSaving')
              : t('properties.batchWriteBack')}
          </button>
        </div>

        {previewError && (
          <div style={errorStyle}>
            {previewError}
          </div>
        )}

        {writeBackMessage && (
          <div style={{ ...infoStyle, borderColor: '#3a6c7a', color: '#9dd8e5' }}>
            {writeBackMessage}
          </div>
        )}

        {preview && (
          <div style={infoStyle}>
            <div style={{ marginBottom: 4 }}>
              {t('properties.batchPreviewSummary', {
                matched: preview.matchedNodeIds.length,
                nodeChanges: preview.changedNodes.length,
                edgeChanges: preview.changedEdges.length,
              })}
            </div>

            {preview.changedNodes.length > 0 && (
              <div style={{ marginBottom: 6 }}>
                <div style={{ fontSize: 10, color: '#9cced9' }}>
                  {t('properties.batchPreviewNodeChanges')}
                </div>
                {preview.changedNodes.slice(0, PREVIEW_ITEM_LIMIT).map((item) => (
                  <div key={item.nodeId} style={{ fontSize: 10, color: '#9cced9', marginTop: 2 }}>
                    {item.nodeId}: {item.beforeLabel} → {item.afterLabel}
                    {item.changedKeys.length > 0
                      ? ` (${item.changedKeys.join(', ')})`
                      : ''}
                  </div>
                ))}
              </div>
            )}

            {preview.changedEdges.length > 0 && (
              <div>
                <div style={{ fontSize: 10, color: '#9cced9' }}>
                  {t('properties.batchPreviewEdgeChanges')}
                </div>
                {preview.changedEdges.slice(0, PREVIEW_ITEM_LIMIT).map((item) => (
                  <div key={item.edgeId} style={{ fontSize: 10, color: '#9cced9', marginTop: 2 }}>
                    {item.from} → {item.to}: {item.beforeSignal || '∅'} → {item.afterSignal || '∅'}
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
};

interface BuildPreviewInput {
  draft: BatchDraft;
  nodes: Array<Node<NodeData>>;
  edges: Edge[];
  findNodeIdsByTag: (dimension: TagDimension, tag: string) => string[];
  findNodeIdsByLocationPath: (locationPath: string) => string[];
}

function buildPreview({
  draft,
  nodes,
  edges,
  findNodeIdsByTag,
  findNodeIdsByLocationPath,
}: BuildPreviewInput): BatchPreview {
  const query = draft.filterQuery.trim();
  if (!query) {
    throw new Error('Tag filter is required. Use * to target all nodes.');
  }

  if (draft.edgeSignalMode === 'set' && !draft.edgeSignalValue.trim()) {
    throw new Error('Signal label is required when edge mode is set.');
  }

  const matchedNodeIds = resolveMatchedNodeIds(
    query,
    draft.filterDimension,
    nodes,
    findNodeIdsByTag,
    findNodeIdsByLocationPath
  );
  if (matchedNodeIds.length === 0) {
    throw new Error('No nodes match the selected tag filter.');
  }

  const nodePatch = parseNodePatch(draft.nodePatchJson);
  const matchedSet = new Set(matchedNodeIds);
  const hasRenameRule = hasRenameRules(draft);

  const changedNodes: NodeChangePreview[] = [];
  const nextNodes = nodes.map((node) => {
    if (!matchedSet.has(node.id)) {
      return node;
    }

    const patchedData: NodeData = {
      ...node.data,
      ...nodePatch,
    };
    const nextData = hasRenameRule
      ? {
          ...patchedData,
          label: applyRenameRule(
            typeof patchedData.label === 'string'
              ? patchedData.label
              : typeof node.data.label === 'string'
              ? node.data.label
              : node.id,
            draft
          ),
        }
      : patchedData;

    if (stableStringify(node.data) === stableStringify(nextData)) {
      return node;
    }

    changedNodes.push({
      nodeId: node.id,
      beforeLabel:
        typeof node.data.label === 'string' ? node.data.label : node.id,
      afterLabel: typeof nextData.label === 'string' ? nextData.label : node.id,
      changedKeys: collectChangedKeys(node.data, nextData),
    });

    return {
      ...node,
      data: nextData,
    };
  });

  const changedEdges: EdgeChangePreview[] = [];
  const nextEdges = edges.map((edge) => {
    const isTouched =
      matchedSet.has(edge.source) || matchedSet.has(edge.target);
    const isInternal =
      matchedSet.has(edge.source) && matchedSet.has(edge.target);
    const shouldUpdate =
      draft.edgeScope === 'touched' ? isTouched : isInternal;

    if (!shouldUpdate || draft.edgeSignalMode === 'no_change') {
      return edge;
    }

    const beforeSignal = readEdgeSignal(edge);
    const afterSignal =
      draft.edgeSignalMode === 'set'
        ? draft.edgeSignalValue.trim()
        : undefined;

    if (beforeSignal === afterSignal) {
      return edge;
    }

    changedEdges.push({
      edgeId: edge.id,
      from: edge.source,
      to: edge.target,
      beforeSignal,
      afterSignal,
    });

    if (afterSignal) {
      return {
        ...edge,
        label: afterSignal,
      };
    }

    return {
      ...edge,
      label: undefined,
    };
  });

  return {
    matchedNodeIds,
    changedNodes,
    changedEdges,
    nextNodes,
    nextEdges,
  };
}

function resolveMatchedNodeIds(
  query: string,
  dimension: TagDimension,
  nodes: Array<Node<NodeData>>,
  findNodeIdsByTag: (dimension: TagDimension, tag: string) => string[],
  findNodeIdsByLocationPath: (locationPath: string) => string[]
): string[] {
  if (query === '*') {
    return nodes.map((node) => node.id);
  }

  if (dimension === 'location_group') {
    return findNodeIdsByLocationPath(query);
  }
  return findNodeIdsByTag(dimension, query);
}

function parseNodePatch(rawJson: string): Record<string, unknown> {
  const trimmed = rawJson.trim();
  if (!trimmed) {
    return {};
  }
  const parsed: unknown = JSON.parse(trimmed);
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('Node patch must be a JSON object.');
  }
  return parsed as Record<string, unknown>;
}

function hasRenameRules(draft: BatchDraft): boolean {
  return Boolean(
    draft.renamePrefix ||
      draft.renameSuffix ||
      draft.renameSearch ||
      draft.renameReplace
  );
}

function applyRenameRule(label: string, draft: BatchDraft): string {
  let nextLabel = label;

  if (draft.renameSearch) {
    nextLabel = nextLabel.split(draft.renameSearch).join(draft.renameReplace);
  }

  return `${draft.renamePrefix}${nextLabel}${draft.renameSuffix}`;
}

function collectChangedKeys(
  beforeData: NodeData,
  afterData: NodeData
): string[] {
  const keys = new Set<string>([
    ...Object.keys(beforeData),
    ...Object.keys(afterData),
  ]);

  return Array.from(keys).filter(
    (key) => stableStringify(beforeData[key]) !== stableStringify(afterData[key])
  );
}

function readEdgeSignal(edge: Edge): string | undefined {
  if (typeof edge.label === 'string') {
    const normalized = edge.label.trim();
    return normalized.length > 0 ? normalized : undefined;
  }
  if (typeof edge.label === 'number') {
    return String(edge.label);
  }
  return undefined;
}

function cloneNodes(nodes: Array<Node<NodeData>>): Array<Node<NodeData>> {
  return nodes.map((node) => ({
    ...node,
    position: { ...node.position },
    data: {
      ...node.data,
      tags: node.data.tags
        ? {
            functional_group: [...node.data.tags.functional_group],
            danger_level: [...node.data.tags.danger_level],
            location_group: [...node.data.tags.location_group],
          }
        : undefined,
    },
  }));
}

function cloneEdges(edges: Edge[]): Edge[] {
  return edges.map((edge) => ({
    ...edge,
    data:
      edge.data && typeof edge.data === 'object'
        ? { ...(edge.data as Record<string, unknown>) }
        : edge.data,
  }));
}

function stableStringify(value: unknown): string {
  return JSON.stringify(value);
}

const labelStyle: React.CSSProperties = {
  display: 'block',
  fontSize: 10,
  color: '#8a8a8a',
  marginBottom: 4,
};

const inputStyle: React.CSSProperties = {
  width: '100%',
  background: '#151515',
  border: '1px solid #3a3a3a',
  borderRadius: 3,
  color: '#e0e0e0',
  fontSize: 11,
  padding: '6px 8px',
  boxSizing: 'border-box',
};

const primaryButtonStyle: React.CSSProperties = {
  flex: 1,
  padding: '6px 10px',
  background: '#00bcd4',
  border: 'none',
  borderRadius: 3,
  color: '#1e1e1e',
  fontSize: 11,
  fontWeight: 600,
  cursor: 'pointer',
};

const secondaryButtonStyle: React.CSSProperties = {
  flex: 1,
  padding: '6px 10px',
  background: '#2d2d2d',
  border: '1px solid #4a4a4a',
  borderRadius: 3,
  color: '#d0d0d0',
  fontSize: 11,
  cursor: 'pointer',
};

const errorStyle: React.CSSProperties = {
  marginTop: 8,
  padding: 8,
  background: '#5a1f1f',
  border: '1px solid #8f2d2d',
  borderRadius: 4,
  fontSize: 10,
  color: '#ffb4b4',
};

const infoStyle: React.CSSProperties = {
  marginTop: 8,
  padding: 8,
  background: '#12313a',
  border: '1px solid #2e5d6a',
  borderRadius: 4,
  fontSize: 10,
  color: '#b8e6f2',
};

export default TagBatchEditor;
