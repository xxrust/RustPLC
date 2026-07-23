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
  const attribution = run.attribution;
  const sourceAuthoringVerdict = attribution?.source_authoring_verdict ?? 'not proven';
  const executionVerdict = attribution?.execution_unattended_verdict ?? 'not proven';

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
        <section className="wb-section wb-attribution-section">
          <div className="wb-section-heading"><h2>File attribution</h2><span>{attribution?.records.length ?? 0} changed files</span></div>
          {attribution ? (
            <>
              <div className="wb-attribution-verdict">
                <StatusPill status={attribution.unattended_verdict === 'proven' ? 'verified' : attribution.human_intervention_detected ? 'blocked' : 'warning'} label={attribution.unattended_verdict} />
                <span>{attribution.reason ?? 'No attribution rationale was recorded.'}</span>
              </div>
              <div className="wb-attribution-scopes" aria-label="Unattended provenance scopes">
                <span className="wb-attribution-scope" data-attribution-scope="source-authoring" data-verdict={sourceAuthoringVerdict}>
                  <small>Source authoring</small>
                  <StatusPill status={sourceAuthoringVerdict === 'proven' ? 'verified' : 'warning'} label={sourceAuthoringVerdict} />
                </span>
                <span className="wb-attribution-scope" data-attribution-scope="materialization-execution" data-verdict={executionVerdict}>
                  <small>Materialization execution</small>
                  <StatusPill status={executionVerdict === 'proven' ? 'verified' : executionVerdict === 'human_intervention_detected' ? 'blocked' : 'warning'} label={executionVerdict} />
                </span>
                <span><small>Scope</small><code>{attribution.provenance_scope ?? 'not recorded'}</code></span>
                <span><small>Authored source records</small><strong>{attribution.source_authoring_record_count ?? 0}</strong></span>
              </div>
              {attribution.validation_issues && attribution.validation_issues.length > 0 && (
                <p className="wb-attribution-issues">Integrity issues: {attribution.validation_issues.join(', ')}</p>
              )}
              <div className="wb-attribution-table" role="table" aria-label="Changed file attribution">
                {attribution.records.map((record) => (
                  <div className="wb-attribution-row" role="row" key={`${record.path}:${record.event_id ?? record.attribution_kind}`}>
                    <code role="cell" title={record.path}>{record.path}</code>
                    <strong role="cell">{humanizeAttribution(record.attribution_kind)}</strong>
                    <span role="cell">{record.agent_id ?? 'No agent'} / {record.task_id ?? 'No task'}</span>
                    <code role="cell" title={`before ${record.before_sha256 ?? 'absent'}; after ${record.after_sha256 ?? 'missing'}`}>
                      {shortDigest(record.before_sha256)} -&gt; {shortDigest(record.after_sha256)}
                    </code>
                    <span role="cell">{record.current_state ?? 'not verifiable'}</span>
                  </div>
                ))}
              </div>
            </>
          ) : <p className="wb-empty-copy">File-level provenance is unavailable. The unattended verdict remains unproven.</p>}
        </section>

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
                    {event.result && <p className="wb-timeline__result">{event.result}</p>}
                    <div className="wb-artifact-links">
                      {(event.artifact_refs ?? (event.artifact_ref ? [event.artifact_ref] : [])).map((artifact) => (
                        <a className="wb-artifact-link" href={artifactHref(artifact)} target="_blank" rel="noreferrer" key={artifact}>{artifact}</a>
                      ))}
                    </div>
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
                <div><dt>Verification</dt><dd>{anomaly.verification_result ?? 'Not recorded'}</dd></div>
                <div><dt>Affected files</dt><dd>{anomaly.affected_files?.join(', ') || 'Not recorded'}</dd></div>
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

function humanizeAttribution(value: string): string {
  const labels: Record<string, string> = {
    agent_generated: 'Agent generated',
    agent_modified: 'Agent modified',
    pre_existing_user_change: 'Pre-existing user change',
    post_run_human_change: 'Post-run human change',
    human_intervention_detected: 'Human intervention detected',
    unattributed_change: 'Unattributed change',
  };
  return labels[value] ?? value.replaceAll('_', ' ');
}

function shortDigest(value: string | null | undefined): string {
  if (value === null) return 'new';
  if (!value) return 'missing';
  return value.slice(0, 10);
}

function artifactHref(path: string): string {
  const normalized = path.replace(/\\/g, '/').replace(/^\/?artifacts\//, '').replace(/^\//, '');
  return `/api/artifacts/${normalized.split('/').map(encodeURIComponent).join('/')}`;
}

export default AgentRunTimelineView;
