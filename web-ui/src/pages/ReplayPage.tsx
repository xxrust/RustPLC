import React, { useState } from 'react';
import { Card, Button, Space, Slider, Table, Typography, Select, Row, Col, Statistic } from 'antd';
import {
  PlayCircleOutlined,
  PauseOutlined,
  StepForwardOutlined,
  StepBackwardOutlined,
  FastForwardOutlined,
  FastBackwardOutlined,
} from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { traceApi, runApi } from '../services/api';

const { Title, Text } = Typography;
const { Option } = Select;

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

  const { data: traceData } = useQuery({
    queryKey: ['trace', selectedRunId],
    queryFn: () => traceApi.getTrace(selectedRunId!),
    enabled: !!selectedRunId,
  });

  const trace = traceData?.data;
  const ticks = trace?.ticks || [];
  const maxTick = ticks.length - 1;
  const currentSnapshot = ticks[currentTick];

  React.useEffect(() => {
    if (!isPlaying || currentTick >= maxTick) return;
    const interval = setInterval(() => {
      setCurrentTick((prev) => Math.min(prev + 1, maxTick));
    }, 1000 / playSpeed);
    return () => clearInterval(interval);
  }, [isPlaying, currentTick, maxTick, playSpeed]);

  const handlePlay = () => setIsPlaying(true);
  const handlePause = () => setIsPlaying(false);
  const handleStepForward = () => setCurrentTick((prev) => Math.min(prev + 1, maxTick));
  const handleStepBackward = () => setCurrentTick((prev) => Math.max(prev - 1, 0));
  const handleFastForward = () => setCurrentTick((prev) => Math.min(prev + 10, maxTick));
  const handleFastBackward = () => setCurrentTick((prev) => Math.max(prev - 10, 0));

  return (
    <div>
      <Title level={2}>{t('tabs.replay')}</Title>

      <Card title={t('replayPage.selectRun')} style={{ marginBottom: 24 }}>
        <Select
          style={{ width: 400 }}
          placeholder={t('replayPage.selectRunPlaceholder')}
          value={selectedRunId}
          onChange={(value) => { setSelectedRunId(value); setCurrentTick(0); setIsPlaying(false); }}
        >
          {runsData?.data?.map((run: any) => (
            <Option key={run.run_id} value={run.run_id}>
              {run.run_id.slice(0, 12)} - {run.status} - {new Date(run.triggered_at).toLocaleString()}
            </Option>
          ))}
        </Select>
      </Card>

      {selectedRunId && (
        <>
          <Card title={t('replayPage.playbackControl')} style={{ marginBottom: 24 }}>
            <Space direction="vertical" style={{ width: '100%' }} size="large">
              <Space>
                <Button icon={<FastBackwardOutlined />} onClick={handleFastBackward} disabled={currentTick === 0}>-10</Button>
                <Button icon={<StepBackwardOutlined />} onClick={handleStepBackward} disabled={currentTick === 0}>
                  {t('replayPage.prevFrame')}
                </Button>
                {isPlaying ? (
                  <Button icon={<PauseOutlined />} onClick={handlePause} type="primary">{t('replay.pause')}</Button>
                ) : (
                  <Button icon={<PlayCircleOutlined />} onClick={handlePlay} type="primary" disabled={currentTick >= maxTick}>
                    {t('replay.play')}
                  </Button>
                )}
                <Button icon={<StepForwardOutlined />} onClick={handleStepForward} disabled={currentTick >= maxTick}>
                  {t('replayPage.nextFrame')}
                </Button>
                <Button icon={<FastForwardOutlined />} onClick={handleFastForward} disabled={currentTick >= maxTick}>+10</Button>
              </Space>

              <div>
                <Text>{t('replayPage.playSpeed')}: </Text>
                <Select value={playSpeed} onChange={setPlaySpeed} style={{ width: 120 }}>
                  <Option value={0.5}>0.5x</Option>
                  <Option value={1}>1x</Option>
                  <Option value={2}>2x</Option>
                  <Option value={5}>5x</Option>
                  <Option value={10}>10x</Option>
                </Select>
              </div>

              <div>
                <Text>{t('replay.tick')}: {currentTick} / {maxTick}</Text>
                <Slider
                  min={0}
                  max={maxTick}
                  value={currentTick}
                  onChange={setCurrentTick}
                  tooltip={{ formatter: (value) => `Tick ${value}` }}
                />
              </div>
            </Space>
          </Card>

          {currentSnapshot && (
            <>
              <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
                <Col span={6}>
                  <Card><Statistic title={t('replayPage.currentTick')} value={currentSnapshot.tick} /></Card>
                </Col>
                <Col span={6}>
                  <Card><Statistic title={t('replayPage.timeMs')} value={currentSnapshot.tick * (trace?.tick_ms || 10)} /></Card>
                </Col>
                <Col span={6}>
                  <Card>
                    <Statistic
                      title={t('replayPage.digitalInputs')}
                      value={currentSnapshot.digital_inputs?.filter(Boolean).length || 0}
                      suffix={`/ ${currentSnapshot.digital_inputs?.length || 0}`}
                    />
                  </Card>
                </Col>
                <Col span={6}>
                  <Card>
                    <Statistic
                      title={t('replayPage.digitalOutputs')}
                      value={currentSnapshot.digital_outputs?.filter(Boolean).length || 0}
                      suffix={`/ ${currentSnapshot.digital_outputs?.length || 0}`}
                    />
                  </Card>
                </Col>
              </Row>

              <Card title={t('replayPage.digitalSignals')} style={{ marginBottom: 24 }}>
                <Row gutter={[16, 16]}>
                  <Col span={12}>
                    <Title level={5}>{t('replayPage.inputs')}</Title>
                    <SignalTable signals={currentSnapshot.digital_inputs || []} prefix="DI" />
                  </Col>
                  <Col span={12}>
                    <Title level={5}>{t('replayPage.outputs')}</Title>
                    <SignalTable signals={currentSnapshot.digital_outputs || []} prefix="DO" />
                  </Col>
                </Row>
              </Card>

              {(currentSnapshot.analog_inputs?.length || currentSnapshot.analog_outputs?.length) && (
                <Card title={t('replayPage.analogSignals')}>
                  <Row gutter={[16, 16]}>
                    {currentSnapshot.analog_inputs && currentSnapshot.analog_inputs.length > 0 && (
                      <Col span={12}>
                        <Title level={5}>{t('replayPage.inputs')}</Title>
                        <AnalogTable signals={currentSnapshot.analog_inputs} prefix="AI" />
                      </Col>
                    )}
                    {currentSnapshot.analog_outputs && currentSnapshot.analog_outputs.length > 0 && (
                      <Col span={12}>
                        <Title level={5}>{t('replayPage.outputs')}</Title>
                        <AnalogTable signals={currentSnapshot.analog_outputs} prefix="AO" />
                      </Col>
                    )}
                  </Row>
                </Card>
              )}
            </>
          )}
        </>
      )}
    </div>
  );
};

const SignalTable: React.FC<{ signals: boolean[]; prefix: string }> = ({ signals, prefix }) => {
  const { t } = useTranslation();
  const data = signals.map((value, index) => ({ key: index, name: `${prefix}${index}`, value }));
  const columns = [
    { title: t('replayPage.signal'), dataIndex: 'name', key: 'name' },
    {
      title: t('replayPage.state'),
      dataIndex: 'value',
      key: 'value',
      render: (value: boolean) => (
        <Text strong style={{ color: value ? '#52c41a' : '#d9d9d9', fontSize: '16px' }}>
          {value ? `● ${t('properties.statusOn')}` : `○ ${t('properties.statusOff')}`}
        </Text>
      ),
    },
  ];
  return <Table dataSource={data} columns={columns} pagination={false} size="small" />;
};

const AnalogTable: React.FC<{ signals: number[]; prefix: string }> = ({ signals, prefix }) => {
  const { t } = useTranslation();
  const data = signals.map((value, index) => ({ key: index, name: `${prefix}${index}`, value }));
  const columns = [
    { title: t('replayPage.signal'), dataIndex: 'name', key: 'name' },
    {
      title: t('properties.value'),
      dataIndex: 'value',
      key: 'value',
      render: (value: number) => <Text code>{value.toFixed(2)}</Text>,
    },
  ];
  return <Table dataSource={data} columns={columns} pagination={false} size="small" />;
};

export default ReplayPage;
