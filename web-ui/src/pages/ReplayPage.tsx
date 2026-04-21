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
import type { TickSnapshot } from '../types';
import { formatTimestamp } from '../utils/time';

const { Option } = Select;
const { Paragraph, Text, Title } = Typography;

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
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [currentTick, setCurrentTick] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [playSpeed, setPlaySpeed] = useState(1);

  const { data: runsData } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runApi.listRuns(20),
  });

  const runs = runsData?.data ?? [];

  useEffect(() => {
    if (!selectedRunId && runs.length > 0) {
      setSelectedRunId(runs.find((run) => run.status === 'fail')?.run_id ?? runs[0].run_id);
    }
  }, [runs, selectedRunId]);

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
  const maxTick = Math.max(ticks.length - 1, 0);
  const activeTick = ticks.length > 0 ? currentTick : undefined;
  const currentSnapshot = ticks[currentTick];
  const previousSnapshot = currentTick > 0 ? ticks[currentTick - 1] : undefined;
  const nearbyKeypoints = (keypointsData?.data.keypoints ?? []).filter(
    (item) => typeof activeTick === 'number' && Math.abs(item.tick - activeTick) <= 1
  );
  const changedLines = frameDelta(previousSnapshot, currentSnapshot);

  useEffect(() => {
    setCurrentTick(0);
    setIsPlaying(false);
  }, [selectedRunId]);

  useEffect(() => {
    if (currentTick > maxTick) {
      setCurrentTick(maxTick);
    }
  }, [currentTick, maxTick]);

  useEffect(() => {
    if (!isPlaying || currentTick >= maxTick) {
      return;
    }
    const interval = window.setInterval(() => {
      setCurrentTick((prev) => Math.min(prev + 1, maxTick));
    }, 1000 / playSpeed);
    return () => window.clearInterval(interval);
  }, [currentTick, isPlaying, maxTick, playSpeed]);

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
            onSelectTick={(tick) => setCurrentTick(Math.min(Math.max(tick, 0), maxTick))}
            title={t('replayPage.reviewFocus')}
            loading={isRunLoading || isGeometryLoading || isKeypointsLoading || isTraceLoading}
          />

          <Card title={t('replayPage.playback')}>
            <Space direction="vertical" style={{ width: '100%' }} size="large">
              <Space wrap size={[8, 8]}>
                <Button
                  icon={<FastBackwardOutlined />}
                  onClick={() => setCurrentTick((prev) => Math.max(prev - 10, 0))}
                  disabled={typeof activeTick !== 'number' || currentTick === 0}
                >
                  -10
                </Button>
                <Button
                  icon={<StepBackwardOutlined />}
                  onClick={() => setCurrentTick((prev) => Math.max(prev - 1, 0))}
                  disabled={typeof activeTick !== 'number' || currentTick === 0}
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
                    disabled={typeof activeTick !== 'number' || currentTick >= maxTick}
                  >
                    {t('replay.play')}
                  </Button>
                )}
                <Button
                  icon={<StepForwardOutlined />}
                  onClick={() => setCurrentTick((prev) => Math.min(prev + 1, maxTick))}
                  disabled={typeof activeTick !== 'number' || currentTick >= maxTick}
                >
                  {t('replayPage.next')}
                </Button>
                <Button
                  icon={<FastForwardOutlined />}
                  onClick={() => setCurrentTick((prev) => Math.min(prev + 10, maxTick))}
                  disabled={typeof activeTick !== 'number' || currentTick >= maxTick}
                >
                  +10
                </Button>
              </Space>

              <Space wrap size={[12, 12]}>
                <Tag>
                  {typeof activeTick === 'number'
                    ? t('replayPage.tickRange', { current: activeTick, total: maxTick })
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
                max={maxTick}
                value={typeof activeTick === 'number' ? currentTick : 0}
                onChange={(value) => setCurrentTick(Array.isArray(value) ? value[0] : value)}
                disabled={typeof activeTick !== 'number'}
                tooltip={{ formatter: (value) => t('review.tickLabel', { tick: value }) }}
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
