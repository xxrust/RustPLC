import React from 'react';
import { ClockCircleOutlined, FileSearchOutlined, ToolOutlined } from '@ant-design/icons';
import type { AgentRun } from '../../types/workbench';
import { StatusPill, WorkbenchState } from '../../components/workbench/WorkbenchPrimitives';
import { formatTime } from '../../components/workbench/workbenchUtils';

const AgentRunTimelineView: React.FC<{ run?: AgentRun }> = ({ run }) => {
  if (!run) {
    return <WorkbenchState kind="empty" title="No agent run selected" detail="Select a run in the Agent Runs explorer to inspect immutable inputs, retries, anomalies, and corrections." />;
  }

  const events = run.events ?? [];
  const anomalies = run.anomalies ?? [];
  const corrections = run.corrections ?? [];

  return (
    <div className="wb-view wb-timeline-view">
      <header className="wb-view-header">
        <div>
          <div className="wb-view-title-row">
            <h1>Agent Run {run.run_id}</h1>
            <StatusPill status={run.status === 'blocked' ? 'blocked' : run.status === 'failed' ? 'warning' : 'observed'} label={run.status ?? 'unreported'} />
          </div>
          <p>{run.unattended_verdict ?? 'Unattended verdict not reported'}</p>
        </div>
        <div className="wb-header-facts">
          <span><ClockCircleOutlined /> {formatTime(run.started_at)}</span>
          <span><ToolOutlined /> {run.model ?? 'Model not recorded'}</span>
          <span><FileSearchOutlined /> {anomalies.length} anomalies / {corrections.length} corrections</span>
        </div>
      </header>

      <div className="wb-timeline-layout">
        <section className="wb-section wb-timeline-section">
          <div className="wb-section-heading"><h2>Execution timeline</h2><span>{events.length} events</span></div>
          {events.length > 0 ? (
            <ol className="wb-timeline">
              {events.map((event, index) => (
                <li key={event.event_id ?? `${event.timestamp}-${index}`}>
                  <span className="wb-timeline__rail" aria-hidden="true" />
                  <div className="wb-timeline__time">{formatTime(event.timestamp)}</div>
                  <div className="wb-timeline__content">
                    <div className="wb-timeline__title">
                      <strong>{event.task ?? 'Unlabeled task'}</strong>
                      <StatusPill status={event.status === 'failed' ? 'warning' : event.status === 'running' ? 'derived' : 'observed'} label={event.status ?? event.result ?? 'recorded'} />
                    </div>
                    <p>{event.agent ?? 'Agent'} / {event.tool ?? 'tool not recorded'} / {event.duration_ms ?? 0} ms</p>
                    {event.artifact_ref && <code>{event.artifact_ref}</code>}
                  </div>
                </li>
              ))}
            </ol>
          ) : <p className="wb-empty-copy">The run exists, but the API returned no chronological event records.</p>}
        </section>

        <section className="wb-section wb-anomaly-section">
          <div className="wb-section-heading"><h2>Anomalies</h2><span>Unresolved items remain visible</span></div>
          {anomalies.length > 0 ? anomalies.map((anomaly, index) => (
            <article className="wb-anomaly" key={anomaly.anomaly_id ?? `${anomaly.code}-${index}`}>
              <div><strong>{anomaly.code ?? 'ANOMALY'}</strong><StatusPill status={anomaly.status ?? 'warning'} /></div>
              <p>{anomaly.summary}</p>
              <dl>
                <div><dt>Root cause</dt><dd>{anomaly.root_cause ?? 'Not classified'}</dd></div>
                <div><dt>Correction</dt><dd>{anomaly.correction || 'See the correction ledger below'}</dd></div>
                {anomaly.retry_count !== undefined && <div><dt>Retries</dt><dd>{anomaly.retry_count}</dd></div>}
                {anomaly.long_search_or_trial_and_error && <div><dt>Skill signal</dt><dd>Long search or repeated trial-and-error detected</dd></div>}
              </dl>
            </article>
          )) : <p className="wb-empty-copy">No anomaly records were returned for this run.</p>}

          <div className="wb-section-heading wb-correction-heading"><h2>Correction ledger</h2><span>{corrections.length} recorded changes</span></div>
          {corrections.length > 0 ? corrections.map((correction, index) => (
            <article className="wb-anomaly wb-correction" key={correction.anomaly_id ?? `${correction.code}-${index}`}>
              <div><strong>{correction.code ?? 'CORRECTION'}</strong><StatusPill status={correction.status ?? 'derived'} /></div>
              <p>{correction.summary}</p>
            </article>
          )) : <p className="wb-empty-copy">No correction records were returned for this run.</p>}
        </section>
      </div>
    </div>
  );
};

export default AgentRunTimelineView;
