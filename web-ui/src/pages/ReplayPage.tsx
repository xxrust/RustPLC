import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Card, Empty, List, Select, Slider, Space, Tag, Typography } from 'antd';
import {
  FastBackwardOutlined,
  FastForwardOutlined,
  PauseOutlined,
  PlayCircleOutlined,
  StepBackwardOutlined,
  StepForwardOutlined,
} from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import RunReviewCockpit from '../components/review/RunReviewCockpit';
import { geometryApi, runApi, traceApi } from '../services/api';
import { useAppStore } from '../stores/appStore';
import type { NodeData } from '../stores/topologyStore';
import { useTopologyStore } from '../stores/topologyStore';
import type { RunStatus, TickSnapshot } from '../types';
import { formatTimestamp } from '../utils/time';

const { Option } = Select;
const { Paragraph, Text, Title } = Typography;

type SignalKind = 'digital' | 'analog';

interface SignalSeries {
  id: string;
  label: string;
  groupLabel: string;
  kind: SignalKind;
  values: Array<boolean | number>;
  min?: number;
  max?: number;
  color: string;
}

const WAVEFORM_WIDTH = 720;
const WAVEFORM_ROW_HEIGHT = 28;

const RUN_PRESETS: Record<
  string,
  { plcFile?: string; topologyFile?: string; scenarioFile?: string }
> = {
  demo: {
    plcFile: 'examples/demo.plc',
    scenarioFile: 'examples/demo.scenario.json',
  },
  component_model: {
    topologyFile: 'examples/component_model/topology.json',
    scenarioFile: 'examples/component_model/scenario_normal.json',
  },
  topology_perf_500: {
    plcFile: 'examples/topology_perf_500.plc',
    topologyFile: 'examples/topology_perf_500.topology.json',
    scenarioFile: 'examples/topology_perf_500.scenario.json',
  },
};

function localizeRunStatus(status: string, t: (key: string) => string): string {
  const map: Record<string, string> = {
    running: 'run.statusRunning',
    pass: 'run.statusPass',
    fail: 'run.statusFail',
  };
  return map[status] ? t(map[status]) : status;
}

function localizeEventCategory(category: string, t: (key: string) => string): string {
  const map: Record<string, string> = {
    sensor_event: 'review.categorySensorEvent',
    switch_event: 'review.categorySwitchEvent',
  };
  return map[category] ? t(map[category]) : category;
}

function frameDelta(previous: TickSnapshot | undefined, current: TickSnapshot | undefined): string[] {
  if (!previous || !current?.component_states) return [];

  const currentStates = current.component_states as Record<string, Record<string, unknown>>;
  const previousStates = (previous?.component_states ?? {}) as Record<string, Record<string, unknown>>;

  return Object.entries(currentStates)
    .map(([componentId, component]) => {
      const prior = previousStates[componentId];
      const currentState =
        typeof component.state === 'string'
          ? component.state
          : typeof component.status === 'string'
            ? component.status
            : 'unknown';
      const previousState =
        typeof prior?.state === 'string'
          ? prior.state
          : typeof prior?.status === 'string'
            ? prior.status
            : undefined;
      const currentOutputs =
        component.outputs && typeof component.outputs === 'object'
          ? (component.outputs as Record<string, unknown>)
          : undefined;
      const previousOutputs =
        prior?.outputs && typeof prior.outputs === 'object'
          ? (prior.outputs as Record<string, unknown>)
          : undefined;

      if (previousState && previousState !== currentState) {
        return `${componentId}: ${previousState} -> ${currentState}`;
      }

      const currentPosition = currentOutputs?.position_steps;
      const previousPosition = previousOutputs?.position_steps;
      if (typeof currentPosition === 'number' && typeof previousPosition === 'number' && currentPosition !== previousPosition) {
        return `${componentId}: position ${previousPosition} -> ${currentPosition}`;
      }

      const currentOutputsText = currentOutputs ? JSON.stringify(currentOutputs) : '';
      const previousOutputsText = previousOutputs ? JSON.stringify(previousOutputs) : '';
      if (currentOutputsText !== previousOutputsText) {
        return `${componentId}: output signals changed`;
      }

      return null;
    })
    .filter((item): item is string => Boolean(item));
}

function buildSignalSeries(
  ticks: TickSnapshot[],
  t: (key: string) => string
): SignalSeries[] {
  return [
    ...buildBooleanSeries(
      ticks,
      'digital_inputs',
      t('replayPage.digitalInputs'),
      'DI',
      '#2563eb'
    ),
    ...buildBooleanSeries(
      ticks,
      'digital_outputs',
      t('replayPage.digitalOutputs'),
      'DO',
      '#16a34a'
    ),
    ...buildNumberSeries(
      ticks,
      'analog_inputs',
      t('replayPage.analogInputs'),
      'AI',
      '#d97706'
    ),
    ...buildNumberSeries(
      ticks,
      'analog_outputs',
      t('replayPage.analogOutputs'),
      'AO',
      '#7c3aed'
    ),
  ];
}

function buildBooleanSeries(
  ticks: TickSnapshot[],
  field: 'digital_inputs' | 'digital_outputs',
  groupLabel: string,
  prefix: string,
  color: string
): SignalSeries[] {
  const width = Math.max(...ticks.map((tick) => tick[field]?.length ?? 0), 0);
  return Array.from({ length: width }, (_, index) => ({
    id: `${field}-${index}`,
    label: `${prefix}${index}`,
    groupLabel,
    kind: 'digital' as const,
    values: ticks.map((tick) => tick[field]?.[index] === true),
    color,
  }));
}

function buildNumberSeries(
  ticks: TickSnapshot[],
  field: 'analog_inputs' | 'analog_outputs',
  groupLabel: string,
  prefix: string,
  color: string
): SignalSeries[] {
  const width = Math.max(...ticks.map((tick) => tick[field]?.length ?? 0), 0);
  return Array.from({ length: width }, (_, index) => {
    const values = ticks.map((tick) => {
      const value = tick[field]?.[index];
      return typeof value === 'number' && Number.isFinite(value) ? value : 0;
    });
    const min = Math.min(...values);
    const max = Math.max(...values);
    return {
      id: `${field}-${index}`,
      label: `${prefix}${index}`,
      groupLabel,
      kind: 'analog' as const,
      values,
      min,
      max,
      color,
    };
  });
}

function xForIndex(index: number, total: number): number {
  if (total <= 1) {
    return 0;
  }
  return (index / (total - 1)) * WAVEFORM_WIDTH;
}

function digitalPath(values: Array<boolean | number>): string {
  if (values.length === 0) {
    return '';
  }
  const highY = 7;
  const lowY = WAVEFORM_ROW_HEIGHT - 7;
  const yFor = (value: boolean | number) => (value === true || value === 1 ? highY : lowY);
  const parts = [`M 0 ${yFor(values[0])}`];

  for (let index = 1; index < values.length; index += 1) {
    const x = xForIndex(index, values.length);
    parts.push(`L ${x} ${yFor(values[index - 1])}`);
    parts.push(`L ${x} ${yFor(values[index])}`);
  }

  if (values.length === 1) {
    parts.push(`L ${WAVEFORM_WIDTH} ${yFor(values[0])}`);
  }

  return parts.join(' ');
}

function analogPath(values: Array<boolean | number>, min = 0, max = 0): string {
  if (values.length === 0) {
    return '';
  }
  const top = 5;
  const bottom = WAVEFORM_ROW_HEIGHT - 5;
  const span = max - min || 1;
  const yFor = (value: boolean | number) => {
    const numeric = typeof value === 'number' && Number.isFinite(value) ? value : 0;
    return bottom - ((numeric - min) / span) * (bottom - top);
  };

  return values
    .map((value, index) => `${index === 0 ? 'M' : 'L'} ${xForIndex(index, values.length)} ${yFor(value)}`)
    .join(' ');
}

function formatSignalValue(value: boolean | number | undefined): string {
  if (typeof value === 'boolean') {
    return value ? '1' : '0';
  }
  if (typeof value === 'number' && Number.isFinite(value)) {
    return Number.isInteger(value) ? String(value) : value.toFixed(2);
  }
  return '-';
}

interface TraceWaveformPanelProps {
  ticks: TickSnapshot[];
  currentFrameIndex: number;
  activeTick?: number;
  tickMs?: number;
  t: (key: string, options?: Record<string, unknown>) => string;
}

const TraceWaveformPanel: React.FC<TraceWaveformPanelProps> = ({
  ticks,
  currentFrameIndex,
  activeTick,
  tickMs = 0,
  t,
}) => {
  const series = useMemo(() => buildSignalSeries(ticks, t), [ticks, t]);
  const markerX = xForIndex(currentFrameIndex, ticks.length);
  const groupedSeries = useMemo(
    () =>
      series.reduce<Array<{ groupLabel: string; rows: SignalSeries[] }>>((groups, item) => {
        const currentGroup = groups[groups.length - 1];
        if (currentGroup?.groupLabel === item.groupLabel) {
          currentGroup.rows.push(item);
        } else {
          groups.push({ groupLabel: item.groupLabel, rows: [item] });
        }
        return groups;
      }, []),
    [series]
  );

  return (
    <Card title={t('replayPage.signalWaveforms')}>
      {series.length > 0 ? (
        <div style={{ display: 'grid', gap: 16 }}>
          <Space wrap size={[8, 8]}>
            <Tag>
              {typeof activeTick === 'number'
                ? t('replayPage.tickRange', { current: activeTick, total: ticks[ticks.length - 1]?.tick ?? 0 })
                : t('replayPage.noTrace')}
            </Tag>
            <Tag>{t('replayPage.timeMsValue', { value: (activeTick ?? 0) * tickMs })}</Tag>
          </Space>

          <div
            style={{
              border: '1px solid #e2e8f0',
              borderRadius: 8,
              overflow: 'hidden',
              background: '#ffffff',
            }}
          >
            {groupedSeries.map((group) => (
              <div key={group.groupLabel}>
                <div
                  style={{
                    padding: '8px 12px',
                    background: '#f8fafc',
                    borderBottom: '1px solid #e2e8f0',
                    color: '#475569',
                    fontSize: 12,
                    fontWeight: 600,
                  }}
                >
                  {group.groupLabel}
                </div>
                {group.rows.map((row) => {
                  const currentValue = row.values[currentFrameIndex];
                  const path =
                    row.kind === 'digital'
                      ? digitalPath(row.values)
                      : analogPath(row.values, row.min, row.max);

                  return (
                    <div
                      key={row.id}
                      style={{
                        display: 'grid',
                        gridTemplateColumns: '88px minmax(360px, 1fr) 72px',
                        alignItems: 'center',
                        minHeight: WAVEFORM_ROW_HEIGHT + 10,
                        borderBottom: '1px solid #f1f5f9',
                      }}
                    >
                      <Text
                        strong
                        style={{
                          paddingLeft: 12,
                          fontSize: 12,
                          color: '#334155',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        {row.label}
                      </Text>
                      <div style={{ overflowX: 'auto', padding: '4px 0' }}>
                        <svg
                          width={WAVEFORM_WIDTH}
                          height={WAVEFORM_ROW_HEIGHT}
                          viewBox={`0 0 ${WAVEFORM_WIDTH} ${WAVEFORM_ROW_HEIGHT}`}
                          role="img"
                          aria-label={`${row.groupLabel} ${row.label}`}
                          style={{ display: 'block' }}
                        >
                          <line
                            x1="0"
                            y1={WAVEFORM_ROW_HEIGHT - 7}
                            x2={WAVEFORM_WIDTH}
                            y2={WAVEFORM_ROW_HEIGHT - 7}
                            stroke="#e2e8f0"
                            strokeWidth="1"
                          />
                          {row.kind === 'analog' ? (
                            <line
                              x1="0"
                              y1={WAVEFORM_ROW_HEIGHT / 2}
                              x2={WAVEFORM_WIDTH}
                              y2={WAVEFORM_ROW_HEIGHT / 2}
                              stroke="#f1f5f9"
                              strokeWidth="1"
                            />
                          ) : null}
                          <path
                            d={path}
                            fill="none"
                            stroke={row.color}
                            strokeWidth={row.kind === 'digital' ? 2.5 : 2}
                            strokeLinejoin="round"
                            strokeLinecap="round"
                          />
                          <line
                            x1={markerX}
                            y1="0"
                            x2={markerX}
                            y2={WAVEFORM_ROW_HEIGHT}
                            stroke="#0f172a"
                            strokeWidth="1"
                            strokeDasharray="3 3"
                          />
                        </svg>
                      </div>
                      <Text
                        style={{
                          justifySelf: 'end',
                          paddingRight: 12,
                          fontVariantNumeric: 'tabular-nums',
                          color: '#475569',
                          fontSize: 12,
                        }}
                      >
                        {formatSignalValue(currentValue)}
                      </Text>
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      ) : (
        <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('replayPage.noWaveformData')} />
      )}
    </Card>
  );
};

const ReplayPage: React.FC = () => {
  const { t } = useTranslation();
  const { currentProject } = useAppStore();
  const { mergeNodeDataById } = useTopologyStore();
  const topologyNodeSignature = useTopologyStore((state) =>
    state.nodes.map((node) => node.id).join('|')
  );
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [frameState, setFrameState] = useState<{ runId: string | null; index: number }>({
    runId: null,
    index: 0,
  });
  const [isPlaying, setIsPlaying] = useState(false);
  const [playSpeed, setPlaySpeed] = useState(1);

  const { data: runsData } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runApi.listRuns(20),
  });

  const runs = useMemo(() => runsData?.data ?? [], [runsData?.data]);
  const preferredRunId = useMemo(() => {
    const preferredRun =
      runs.find((run) => runMatchesCurrentProject(run, currentProject)) ??
      runs.find((run) => run.status === 'fail') ??
      runs[0];
    return preferredRun?.run_id ?? null;
  }, [currentProject, runs]);
  const activeRunId = selectedRunId ?? preferredRunId;

  const selectedRun = useMemo(
    () => runs.find((run) => run.run_id === activeRunId),
    [activeRunId, runs]
  );

  const { data: runStatusData, isLoading: isRunLoading } = useQuery({
    queryKey: ['replay-run-status', activeRunId],
    queryFn: () => runApi.getRunStatus(activeRunId!),
    enabled: Boolean(activeRunId),
  });

  const { data: traceData, isLoading: isTraceLoading } = useQuery({
    queryKey: ['trace', activeRunId],
    queryFn: () => traceApi.getTrace(activeRunId!),
    enabled: Boolean(activeRunId),
  });

  const { data: geometryData, isLoading: isGeometryLoading } = useQuery({
    queryKey: ['replay-geometry', activeRunId],
    queryFn: () => geometryApi.getGeometry(activeRunId!),
    enabled: Boolean(activeRunId),
  });

  const { data: keypointsData, isLoading: isKeypointsLoading } = useQuery({
    queryKey: ['replay-keypoints', activeRunId],
    queryFn: () => traceApi.getKeypoints(activeRunId!),
    enabled: Boolean(activeRunId),
  });

  const run = runStatusData?.data ?? selectedRun;
  const trace = traceData?.data;

  const ticks = trace?.ticks || [];
  const maxFrameIndex = Math.max(ticks.length - 1, 0);
  const currentFrameIndex =
    frameState.runId === activeRunId ? Math.min(frameState.index, maxFrameIndex) : 0;
  const currentSnapshot = ticks[currentFrameIndex];
  const previousSnapshot = currentFrameIndex > 0 ? ticks[currentFrameIndex - 1] : undefined;
  const activeTick = currentSnapshot?.tick;
  const maxTickValue = ticks.length > 0 ? ticks[ticks.length - 1].tick : 0;
  const nearbyKeypoints = (keypointsData?.data.keypoints ?? []).filter(
    (item) => typeof activeTick === 'number' && Math.abs(item.tick - activeTick) <= 1
  );
  const changedLines = frameDelta(previousSnapshot, currentSnapshot);
  const setCurrentFrameIndex = useCallback((next: number | ((previous: number) => number)) => {
    setFrameState((previous) => {
      const base = previous.runId === activeRunId ? Math.min(previous.index, maxFrameIndex) : 0;
      const rawIndex = typeof next === 'function' ? next(base) : next;
      return {
        runId: activeRunId,
        index: Math.min(Math.max(rawIndex, 0), maxFrameIndex),
      };
    });
  }, [activeRunId, maxFrameIndex]);

  useEffect(() => {
    if (!isPlaying || currentFrameIndex >= maxFrameIndex) {
      return;
    }
    const interval = window.setInterval(() => {
      setCurrentFrameIndex((prev) => Math.min(prev + 1, maxFrameIndex));
    }, 1000 / playSpeed);
    return () => window.clearInterval(interval);
  }, [currentFrameIndex, isPlaying, maxFrameIndex, playSpeed, setCurrentFrameIndex]);

  useEffect(() => {
    if (!currentSnapshot) {
      return;
    }
    mergeNodeDataById(normalizeReplaySnapshot(currentSnapshot), false);
  }, [currentSnapshot, mergeNodeDataById, topologyNodeSignature]);

  return (
    <div style={{ display: 'grid', gap: 24 }}>
      <div>
        <Title level={2} style={{ marginBottom: 8 }}>
          {t('replayPage.title')}
        </Title>
        <Paragraph style={{ color: '#94a3b8', marginBottom: 0 }}>
          {t('replayPage.intro')}
        </Paragraph>
      </div>

      <Card title={t('replayPage.selectRun')}>
        <Select
          style={{ width: 460 }}
          placeholder={t('replayPage.selectRunPlaceholder')}
          value={activeRunId ?? undefined}
          onChange={(value) => {
            setSelectedRunId(value);
            setIsPlaying(false);
          }}
        >
          {runs.map((runItem) => (
            <Option key={runItem.run_id} value={runItem.run_id}>
              {runItem.run_id.slice(0, 12)} - {localizeRunStatus(runItem.status, t)} -{' '}
              {formatTimestamp(runItem.triggered_at, runItem.triggered_at_ms)}
            </Option>
          ))}
        </Select>
      </Card>

      {activeRunId ? (
        <>
          <RunReviewCockpit
            run={run}
            geometry={geometryData?.data}
            keypoints={keypointsData?.data}
            trace={trace}
            currentTick={activeTick}
            onSelectTick={(tick) => setCurrentFrameIndex(findSnapshotIndexForTick(ticks, tick))}
            title={t('replayPage.reviewFocus')}
            loading={isRunLoading || isGeometryLoading || isKeypointsLoading || isTraceLoading}
          />

          <Card title={t('replayPage.playback')}>
            <Space direction="vertical" style={{ width: '100%' }} size="large">
              <Space wrap size={[8, 8]}>
                <Button
                  icon={<FastBackwardOutlined />}
                  onClick={() => setCurrentFrameIndex((prev) => Math.max(prev - 10, 0))}
                  disabled={typeof activeTick !== 'number' || currentFrameIndex === 0}
                >
                  -10
                </Button>
                <Button
                  icon={<StepBackwardOutlined />}
                  onClick={() => setCurrentFrameIndex((prev) => Math.max(prev - 1, 0))}
                  disabled={typeof activeTick !== 'number' || currentFrameIndex === 0}
                >
                  {t('replayPage.prev')}
                </Button>
                {isPlaying ? (
                  <Button icon={<PauseOutlined />} onClick={() => setIsPlaying(false)} type="primary">
                    {t('replay.pause')}
                  </Button>
                ) : (
                  <Button
                    icon={<PlayCircleOutlined />}
                    onClick={() => setIsPlaying(true)}
                    type="primary"
                    disabled={typeof activeTick !== 'number' || currentFrameIndex >= maxFrameIndex}
                  >
                    {t('replay.play')}
                  </Button>
                )}
                <Button
                  icon={<StepForwardOutlined />}
                  onClick={() => setCurrentFrameIndex((prev) => Math.min(prev + 1, maxFrameIndex))}
                  disabled={typeof activeTick !== 'number' || currentFrameIndex >= maxFrameIndex}
                >
                  {t('replayPage.next')}
                </Button>
                <Button
                  icon={<FastForwardOutlined />}
                  onClick={() => setCurrentFrameIndex((prev) => Math.min(prev + 10, maxFrameIndex))}
                  disabled={typeof activeTick !== 'number' || currentFrameIndex >= maxFrameIndex}
                >
                  +10
                </Button>
              </Space>

              <Space wrap size={[12, 12]}>
                <Tag>
                  {typeof activeTick === 'number'
                    ? t('replayPage.tickRange', { current: activeTick, total: maxTickValue })
                    : t('replayPage.noTrace')}
                </Tag>
                <Tag>
                  {currentSnapshot
                    ? t('replayPage.timeMsValue', { value: currentSnapshot.tick * (trace?.tick_ms ?? 0) })
                    : t('replayPage.noTime')}
                </Tag>
                <Text>{t('replayPage.speed')}</Text>
                <Select value={playSpeed} onChange={setPlaySpeed} style={{ width: 120 }}>
                  <Option value={0.5}>0.5x</Option>
                  <Option value={1}>1x</Option>
                  <Option value={2}>2x</Option>
                  <Option value={5}>5x</Option>
                  <Option value={10}>10x</Option>
                </Select>
              </Space>

              <Slider
                min={0}
                max={maxFrameIndex}
                value={typeof activeTick === 'number' ? currentFrameIndex : 0}
                onChange={(value) => setCurrentFrameIndex(Array.isArray(value) ? value[0] : value)}
                disabled={typeof activeTick !== 'number'}
                tooltip={{
                  formatter: (value) =>
                    t('review.tickLabel', {
                      tick: ticks[Array.isArray(value) ? value[0] : value]?.tick ?? value,
                    }),
                }}
              />
            </Space>
          </Card>

          <TraceWaveformPanel
            ticks={ticks}
            currentFrameIndex={currentFrameIndex}
            activeTick={activeTick}
            tickMs={trace?.tick_ms}
            t={t}
          />

          <Card title={t('replayPage.frameDelta')}>
            <Space direction="vertical" size="middle" style={{ width: '100%' }}>
              {nearbyKeypoints.length > 0 ? (
                <List
                  size="small"
                  header={t('replayPage.nearbyKeypoints')}
                  dataSource={nearbyKeypoints}
                  renderItem={(item) => (
                    <List.Item>
                      {t('review.tickEventLine', { tick: item.tick, label: item.label })} |{' '}
                      {localizeEventCategory(item.category, t)}
                    </List.Item>
                  )}
                />
              ) : (
                <Text type="secondary">{t('replayPage.noKeypointOnTick')}</Text>
              )}

              <List
                size="small"
                header={t('replayPage.componentDelta')}
                dataSource={changedLines}
                locale={{
                  emptyText: previousSnapshot
                    ? t('replayPage.noComponentDelta')
                    : t('replayPage.firstTickNoPreviousFrame'),
                }}
                renderItem={(item) => <List.Item>{item}</List.Item>}
              />
            </Space>
          </Card>
        </>
      ) : (
        <Card>
          <Text type="secondary">{t('replayPage.noRunsAvailable')}</Text>
        </Card>
      )}
    </div>
  );
};

export default ReplayPage;

function normalizePath(path?: string | null): string | undefined {
  return path?.replace(/\\/g, '/');
}

function findSnapshotIndexForTick(ticks: TickSnapshot[], tick: number): number {
  if (ticks.length === 0) {
    return 0;
  }

  const exactIndex = ticks.findIndex((snapshot) => snapshot.tick === tick);
  if (exactIndex >= 0) {
    return exactIndex;
  }

  const nextIndex = ticks.findIndex((snapshot) => snapshot.tick >= tick);
  if (nextIndex >= 0) {
    return nextIndex;
  }

  return ticks.length - 1;
}

function runMatchesCurrentProject(
  run: RunStatus,
  currentProject: string | null
): boolean {
  if (!currentProject) {
    return false;
  }
  const preset = RUN_PRESETS[currentProject];
  if (!preset) {
    return false;
  }

  return (
    normalizePath(run.plc_file) === preset.plcFile ||
    normalizePath(run.topology_file) === preset.topologyFile ||
    normalizePath(run.scenario_file) === preset.scenarioFile
  );
}

function normalizeReplaySnapshot(snapshot: TickSnapshot): Record<string, Partial<NodeData>> {
  const rawComponents = snapshot?.component_states;
  if (!rawComponents || typeof rawComponents !== 'object') {
    return {};
  }

  return Object.entries(rawComponents).reduce<Record<string, Partial<NodeData>>>(
    (acc, [componentId, raw]) => {
      if (!raw || typeof raw !== 'object') {
        return acc;
      }
      acc[componentId] = normalizeReplayComponent(raw as Record<string, unknown>, snapshot.tick);
      return acc;
    },
    {}
  );
}

function normalizeReplayComponent(
  raw: Record<string, unknown>,
  tick: number
): Partial<NodeData> {
  const outputs =
    raw.outputs && typeof raw.outputs === 'object'
      ? (raw.outputs as Record<string, unknown>)
      : {};
  const inputs =
    raw.inputs && typeof raw.inputs === 'object'
      ? (raw.inputs as Record<string, unknown>)
      : {};
  const componentType =
    typeof raw.component_type === 'string' ? raw.component_type.toLowerCase() : '';
  const state =
    typeof raw.state === 'string'
      ? raw.state
      : typeof raw.status === 'string'
        ? raw.status
        : '';
  const hasFault = Array.isArray(raw.active_faults) && raw.active_faults.length > 0;

  let status = state;
  if (componentType === 'switch') {
    if (state === 'on') status = 'closed';
    if (state === 'off') status = 'open';
  } else if (componentType === 'stepper_pd') {
    if (state === 'enabled') status = 'running';
    if (state === 'disabled') status = 'idle';
  }
  if (hasFault) {
    status = 'fault';
  }

  const normalized: Partial<NodeData> = {
    lastReplayTick: tick,
    replayComponentType: componentType || undefined,
    replayInputs: Object.keys(inputs).length > 0 ? inputs : undefined,
    replayOutputs: Object.keys(outputs).length > 0 ? outputs : undefined,
    replayFaults: hasFault ? raw.active_faults : undefined,
  };

  if (status) {
    normalized.status = status;
  }
  if (typeof outputs.state === 'boolean') {
    normalized.value = outputs.state;
  } else if (typeof raw.value === 'boolean' || typeof raw.value === 'number') {
    normalized.value = raw.value;
  }
  return normalized;
}
