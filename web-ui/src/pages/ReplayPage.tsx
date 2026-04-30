import React, { useEffect, useMemo, useState } from 'react';
import { Button, Card, List, Select, Slider, Space, Tag, Typography } from 'antd';
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

const ReplayPage: React.FC = () => {
  const { t } = useTranslation();
  const { currentProject } = useAppStore();
  const { mergeNodeDataById } = useTopologyStore();
  const topologyNodeSignature = useTopologyStore((state) =>
    state.nodes.map((node) => node.id).join('|')
  );
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [currentFrameIndex, setCurrentFrameIndex] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [playSpeed, setPlaySpeed] = useState(1);

  const { data: runsData } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runApi.listRuns(20),
  });

  const runs = runsData?.data ?? [];

  useEffect(() => {
    if (!selectedRunId && runs.length > 0) {
      const preferredRun =
        runs.find((run) => runMatchesCurrentProject(run, currentProject)) ??
        runs.find((run) => run.status === 'fail') ??
        runs[0];
      setSelectedRunId(preferredRun?.run_id ?? null);
    }
  }, [currentProject, runs, selectedRunId]);

  const selectedRun = useMemo(
    () => runs.find((run) => run.run_id === selectedRunId),
    [runs, selectedRunId]
  );

  const { data: runStatusData, isLoading: isRunLoading } = useQuery({
    queryKey: ['replay-run-status', selectedRunId],
    queryFn: () => runApi.getRunStatus(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  const { data: traceData, isLoading: isTraceLoading } = useQuery({
    queryKey: ['trace', selectedRunId],
    queryFn: () => traceApi.getTrace(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  const { data: geometryData, isLoading: isGeometryLoading } = useQuery({
    queryKey: ['replay-geometry', selectedRunId],
    queryFn: () => geometryApi.getGeometry(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  const { data: keypointsData, isLoading: isKeypointsLoading } = useQuery({
    queryKey: ['replay-keypoints', selectedRunId],
    queryFn: () => traceApi.getKeypoints(selectedRunId!),
    enabled: Boolean(selectedRunId),
  });

  const run = runStatusData?.data ?? selectedRun;
  const trace = traceData?.data;

  const ticks = trace?.ticks || [];
  const maxFrameIndex = Math.max(ticks.length - 1, 0);
  const currentSnapshot = ticks[currentFrameIndex];
  const previousSnapshot = currentFrameIndex > 0 ? ticks[currentFrameIndex - 1] : undefined;
  const activeTick = currentSnapshot?.tick;
  const maxTickValue = ticks.length > 0 ? ticks[ticks.length - 1].tick : 0;
  const nearbyKeypoints = (keypointsData?.data.keypoints ?? []).filter(
    (item) => typeof activeTick === 'number' && Math.abs(item.tick - activeTick) <= 1
  );
  const changedLines = frameDelta(previousSnapshot, currentSnapshot);

  useEffect(() => {
    setCurrentFrameIndex(0);
    setIsPlaying(false);
  }, [selectedRunId]);

  useEffect(() => {
    if (currentFrameIndex > maxFrameIndex) {
      setCurrentFrameIndex(maxFrameIndex);
    }
  }, [currentFrameIndex, maxFrameIndex]);

  useEffect(() => {
    if (!isPlaying || currentFrameIndex >= maxFrameIndex) {
      return;
    }
    const interval = window.setInterval(() => {
      setCurrentFrameIndex((prev) => Math.min(prev + 1, maxFrameIndex));
    }, 1000 / playSpeed);
    return () => window.clearInterval(interval);
  }, [currentFrameIndex, isPlaying, maxFrameIndex, playSpeed]);

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
          value={selectedRunId ?? undefined}
          onChange={(value) => setSelectedRunId(value)}
        >
          {runs.map((runItem) => (
            <Option key={runItem.run_id} value={runItem.run_id}>
              {runItem.run_id.slice(0, 12)} - {localizeRunStatus(runItem.status, t)} -{' '}
              {formatTimestamp(runItem.triggered_at, runItem.triggered_at_ms)}
            </Option>
          ))}
        </Select>
      </Card>

      {selectedRunId ? (
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
