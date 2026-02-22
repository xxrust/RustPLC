import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TagDimension } from '../../types';
import { useTopologyStore } from '../../stores/topologyStore';
import {
  buildTagGroupColorMap,
  resolveLocationFocusNodeIds,
  resolveTagFilterNodeIds,
} from '../../utils/tagVisualization';

const TAG_DIMENSIONS: TagDimension[] = [
  'functional_group',
  'danger_level',
  'location_group',
];

const MAX_LEGEND_ITEMS = 6;

const TagVisualizationPanel: React.FC = () => {
  const { t } = useTranslation();
  const {
    nodes,
    edges,
    tagFilter,
    tagGrouping,
    locationFocus,
    setTagFilter,
    clearTagFilter,
    setTagGrouping,
    clearTagGrouping,
    focusLocationRegion,
    clearLocationFocus,
  } = useTopologyStore();

  const [locationInput, setLocationInput] = useState(locationFocus.locationPath);

  const matchedNodeIds = useMemo(() => {
    if (!tagFilter.enabled) {
      return new Set(nodes.map((node) => node.id));
    }
    return resolveTagFilterNodeIds(nodes, tagFilter.dimension, tagFilter.query);
  }, [nodes, tagFilter.dimension, tagFilter.enabled, tagFilter.query]);

  const filteredEdgeCount = useMemo(
    () =>
      edges.filter(
        (edge) => matchedNodeIds.has(edge.source) && matchedNodeIds.has(edge.target)
      ).length,
    [edges, matchedNodeIds]
  );

  const groupColorMap = useMemo(
    () => buildTagGroupColorMap(nodes, tagGrouping.dimension),
    [nodes, tagGrouping.dimension]
  );

  const previewFocus = useMemo(
    () => resolveLocationFocusNodeIds(nodes, edges, locationInput, true),
    [edges, locationInput, nodes]
  );

  const activeFocus = useMemo(
    () =>
      locationFocus.active
        ? resolveLocationFocusNodeIds(
            nodes,
            edges,
            locationFocus.locationPath,
            locationFocus.includeNeighbors
          )
        : null,
    [
      edges,
      locationFocus.active,
      locationFocus.includeNeighbors,
      locationFocus.locationPath,
      nodes,
    ]
  );

  return (
    <div style={{ marginBottom: 16 }}>
      <div style={{ fontSize: 11, color: '#a0a0a0', marginBottom: 8 }}>
        {t('properties.tagViewTitle')}
      </div>
      <div style={{ background: '#1e1e1e', border: '1px solid #3a3a3a', borderRadius: 4, padding: 12 }}>
        <div style={{ marginBottom: 10 }}>
          <label style={labelStyle}>{t('properties.tagViewFilterDimension')}</label>
          <select
            value={tagFilter.dimension}
            onChange={(event) =>
              setTagFilter(event.target.value as TagDimension, tagFilter.query)
            }
            style={inputStyle}
          >
            {TAG_DIMENSIONS.map((dimension) => (
              <option key={dimension} value={dimension}>
                {dimension}
              </option>
            ))}
          </select>
          <input
            type="text"
            value={tagFilter.query}
            onChange={(event) =>
              setTagFilter(tagFilter.dimension, event.target.value)
            }
            placeholder={t('properties.tagViewFilterPlaceholder')}
            style={{ ...inputStyle, marginTop: 6 }}
          />
          <div style={{ marginTop: 4, fontSize: 10, color: '#7a7a7a' }}>
            {t('properties.tagViewFilterSummary', {
              nodes: matchedNodeIds.size,
              edges: filteredEdgeCount,
            })}
          </div>
          {tagFilter.enabled && (
            <button onClick={clearTagFilter} style={{ ...secondaryButtonStyle, marginTop: 6 }}>
              {t('properties.tagViewClearFilter')}
            </button>
          )}
        </div>

        <div style={{ marginBottom: 10 }}>
          <label style={labelStyle}>{t('properties.tagViewGroupingDimension')}</label>
          <select
            value={tagGrouping.dimension}
            onChange={(event) =>
              setTagGrouping(tagGrouping.enabled, event.target.value as TagDimension)
            }
            style={inputStyle}
          >
            {TAG_DIMENSIONS.map((dimension) => (
              <option key={dimension} value={dimension}>
                {dimension}
              </option>
            ))}
          </select>
          <label style={{ ...labelStyle, display: 'flex', alignItems: 'center', gap: 6, marginTop: 6 }}>
            <input
              type="checkbox"
              checked={tagGrouping.enabled}
              onChange={(event) => setTagGrouping(event.target.checked)}
            />
            {t('properties.tagViewGroupingToggle')}
          </label>
          {tagGrouping.enabled && (
            <div style={{ marginTop: 6, background: '#151515', border: '1px solid #333', borderRadius: 3, padding: 8 }}>
              {Array.from(groupColorMap.entries())
                .slice(0, MAX_LEGEND_ITEMS)
                .map(([tagValue, color]) => (
                  <div key={tagValue} style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 10, color: '#9cced9', marginBottom: 4 }}>
                    <span style={{ width: 10, height: 10, borderRadius: '50%', background: color, display: 'inline-block' }} />
                    <span>{tagValue}</span>
                  </div>
                ))}
              {groupColorMap.size > MAX_LEGEND_ITEMS && (
                <div style={{ fontSize: 10, color: '#7a7a7a' }}>
                  {t('properties.tagViewGroupingMore', {
                    count: groupColorMap.size - MAX_LEGEND_ITEMS,
                  })}
                </div>
              )}
              <button onClick={clearTagGrouping} style={{ ...secondaryButtonStyle, marginTop: 6 }}>
                {t('properties.tagViewClearGrouping')}
              </button>
            </div>
          )}
        </div>

        <div>
          <label style={labelStyle}>{t('properties.tagViewLocationLocate')}</label>
          <input
            type="text"
            value={locationInput}
            onChange={(event) => setLocationInput(event.target.value)}
            placeholder={t('properties.tagViewLocationPlaceholder')}
            style={inputStyle}
          />
          <div style={{ marginTop: 4, fontSize: 10, color: '#7a7a7a' }}>
            {t('properties.tagViewLocationPreview', {
              region: previewFocus.regionNodeIds.size,
              neighbors: previewFocus.focusNodeIds.size,
            })}
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 6 }}>
            <button
              onClick={() => focusLocationRegion(locationInput, true)}
              style={primaryButtonStyle}
              disabled={!locationInput.trim()}
            >
              {t('properties.tagViewLocateButton')}
            </button>
            <button
              onClick={() => {
                clearLocationFocus();
                setLocationInput('');
              }}
              style={secondaryButtonStyle}
            >
              {t('properties.tagViewClearLocate')}
            </button>
          </div>
          {activeFocus && (
            <div style={{ marginTop: 6, fontSize: 10, color: '#9dd8e5' }}>
              {t('properties.tagViewLocationActive', {
                region: activeFocus.regionNodeIds.size,
                neighbors: activeFocus.focusNodeIds.size,
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

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

export default TagVisualizationPanel;
