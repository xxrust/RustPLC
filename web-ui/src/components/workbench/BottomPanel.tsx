import React, { useMemo, useState } from 'react';
import {
  AuditOutlined,
  CheckSquareOutlined,
  CloseOutlined,
  CodeOutlined,
  ExclamationCircleOutlined,
  SafetyCertificateOutlined,
} from '@ant-design/icons';
import type { BottomPanelId } from '../../stores/workbenchStore';
import type {
  DeliveryProjectSummary,
  EvidenceRecord,
  VerificationStage,
  WorkspaceProblem,
  WorkspaceProblemsProjection,
  WorkspaceTest,
  WorkspaceTestSource,
  WorkspaceTestsProjection,
} from '../../types/workbench';
import { StatusPill, WorkbenchState } from './WorkbenchPrimitives';

const panels: Array<{ id: BottomPanelId; label: string; icon: React.ReactNode }> = [
  { id: 'problems', label: 'Problems', icon: <ExclamationCircleOutlined /> },
  { id: 'tests', label: 'Tests', icon: <CheckSquareOutlined /> },
  { id: 'verification', label: 'Verification', icon: <SafetyCertificateOutlined /> },
  { id: 'terminal', label: 'Terminal', icon: <CodeOutlined /> },
  { id: 'audit', label: 'Audit Log', icon: <AuditOutlined /> },
];

const BottomPanel: React.FC<{
  active: BottomPanelId;
  problems: WorkspaceProblemsProjection;
  tests: WorkspaceTestsProjection;
  problemsRequestError?: string;
  testsRequestError?: string;
  verification: VerificationStage[];
  evidence: EvidenceRecord[];
  projects: DeliveryProjectSummary[];
  onChange: (panel: BottomPanelId) => void;
  onCollapse: () => void;
  onNavigateProblem: (problem: WorkspaceProblem) => void;
  onNavigateTest: (test: WorkspaceTest) => void;
  onRetryProblems: () => void;
  onRetryTests: () => void;
}> = ({ active, problems, tests, problemsRequestError, testsRequestError, verification, evidence, projects, onChange, onCollapse, onNavigateProblem, onNavigateTest, onRetryProblems, onRetryTests }) => (
  <section className="wb-bottom-panel" aria-label="Workbench bottom panel">
    <div className="wb-bottom-tabs" role="tablist">
      {panels.map((panel) => (
        <button
          id={`wb-bottom-tab-${panel.id}`}
          key={panel.id}
          data-bottom-panel={panel.id}
          type="button"
          role="tab"
          tabIndex={active === panel.id ? 0 : -1}
          aria-selected={active === panel.id}
          aria-controls="wb-bottom-tabpanel"
          className={active === panel.id ? 'is-active' : undefined}
          onClick={() => onChange(panel.id)}
          onKeyDown={(event) => handlePanelTabKey(event, panel.id, onChange)}
        >
          {panel.icon}{panel.label}
          {panel.id === 'problems' && problems.count > 0 && <span>{problems.count}</span>}
          {panel.id === 'tests' && tests.count > 0 && <span>{tests.count}</span>}
        </button>
      ))}
      <button className="wb-bottom-close" type="button" aria-label="Collapse bottom panel" onClick={onCollapse}><CloseOutlined /></button>
    </div>
    <div id="wb-bottom-tabpanel" className="wb-bottom-content" role="tabpanel" aria-labelledby={`wb-bottom-tab-${active}`}>
      {active === 'problems' && <ProblemsPanel projection={problems} projects={projects} requestError={problemsRequestError} onNavigate={onNavigateProblem} onRetry={onRetryProblems} />}
      {active === 'tests' && <TestsPanel projection={tests} requestError={testsRequestError} onNavigate={onNavigateTest} onRetry={onRetryTests} />}
      {active === 'verification' && <VerificationPanel stages={verification} />}
      {active === 'terminal' && <TerminalPanel />}
      {active === 'audit' && <AuditPanel evidence={evidence} />}
    </div>
  </section>
);

const ProblemsPanel: React.FC<{
  projection: WorkspaceProblemsProjection;
  projects: DeliveryProjectSummary[];
  requestError?: string;
  onNavigate: (problem: WorkspaceProblem) => void;
  onRetry: () => void;
}> = ({ projection, projects, requestError, onNavigate, onRetry }) => {
  const problems = projection.problems;
  const [groupBy, setGroupBy] = useState<'project' | 'stage' | 'code' | 'commit'>('stage');
  const [severity, setSeverity] = useState<'all' | WorkspaceProblem['severity']>('all');
  const projectCommits = useMemo(
    () => new Map(projects.map((project) => [project.project_id, project.source_commit])),
    [projects],
  );
  const groups = useMemo(() => {
    const filtered = severity === 'all' ? problems : problems.filter((problem) => problem.severity === severity);
    return groupItems(filtered, (problem) => {
      if (groupBy === 'project') return problem.project_id ?? 'Unassigned project';
      if (groupBy === 'code') return problem.code ?? 'No diagnostic code';
      if (groupBy === 'commit') return problem.source_commit ?? projectCommits.get(problem.project_id ?? '') ?? 'Source commit unavailable';
      return problem.stage ?? 'Unassigned stage';
    });
  }, [groupBy, problems, projectCommits, severity]);

  if (requestError && problems.length === 0) return <WorkbenchState kind="error" title="Problems unavailable" detail={requestError} onRetry={onRetry} />;
  if (problems.length === 0) return <WorkbenchState kind={projection.partial ? 'stale' : 'empty'} title={projection.partial ? 'Problem index is partial' : 'No indexed problems'} detail={projection.partial ? 'The server could not inspect every configured problem source. An empty partial result is not evidence that the workspace is clear.' : 'Compiler diagnostics and project blockers appear here without being converted into frontend verdicts.'} onRetry={projection.partial ? onRetry : undefined} />;
  return (
    <div className="wb-grouped-panel">
      {requestError && <ProjectionNotice status="blocked" title="Problem refresh failed" detail={`${requestError} Showing the last returned projection.`} onRetry={onRetry} />}
      {projection.partial && <ProjectionNotice status="warning" title="Partial problem index" detail={`${projection.count} indexed records are visible. One or more configured sources could not be inspected.`} />}
      <PanelFilters>
        <label>Group by<select aria-label="Group problems" value={groupBy} onChange={(event) => setGroupBy(event.target.value as typeof groupBy)}><option value="stage">Stage</option><option value="project">Project</option><option value="commit">Source commit</option><option value="code">Diagnostic code</option></select></label>
        <label>Severity<select aria-label="Filter problem severity" value={severity} onChange={(event) => setSeverity(event.target.value as typeof severity)}><option value="all">All</option><option value="blocked">Blocked</option><option value="error">Error</option><option value="warning">Warning</option><option value="info">Info</option></select></label>
      </PanelFilters>
      {groups.length === 0 && <WorkbenchState kind="empty" title="No matching problems" detail="Change the severity filter to include additional compiler diagnostics." />}
      {groups.map(([group, rows]) => <section key={group}><h3>{group}<span>{rows.length}</span></h3><div className="wb-panel-table">{rows.map((problem) => <button type="button" key={problem.id} onClick={() => onNavigate(problem)}><span className={`wb-problem-icon wb-problem-icon--${problem.severity}`}>●</span><strong>{problem.code ?? problem.severity}</strong><span>{problem.message}</span><code>{problem.source_ref ?? problem.stage ?? problem.project_id}</code></button>)}</div></section>)}
    </div>
  );
};

const TestsPanel: React.FC<{
  projection: WorkspaceTestsProjection;
  requestError?: string;
  onNavigate: (test: WorkspaceTest) => void;
  onRetry: () => void;
}> = ({ projection, requestError, onNavigate, onRetry }) => {
  const tests = projection.tests;
  const [groupBy, setGroupBy] = useState<'source' | 'suite' | 'status' | 'project'>('source');
  const [status, setStatus] = useState<'all' | WorkspaceTest['status']>('all');
  const groups = useMemo(() => {
    const filtered = status === 'all' ? tests : tests.filter((test) => test.status === status);
    const grouped = groupItems(filtered, (test) => {
      if (groupBy === 'status') return test.status;
      if (groupBy === 'project') return test.project_id ?? 'Unassigned project';
      if (groupBy === 'suite') return test.suite ?? 'Unclassified suite';
      return testExecutionSource(test);
    });
    if (groupBy !== 'source') return grouped;
    const byName = new Map(grouped);
    return ['Library', 'Integration', 'Canonical example', 'Delivery project']
      .map((name) => [name, byName.get(name) ?? []] as [string, WorkspaceTest[]]);
  }, [groupBy, status, tests]);

  if (requestError && tests.length === 0 && projection.sources.length === 0) return <WorkbenchState kind="error" title="Tests unavailable" detail={requestError} onRetry={onRetry} />;
  return (
    <div className="wb-grouped-panel">
      {requestError && <ProjectionNotice status="blocked" title="Test refresh failed" detail={`${requestError} Showing the last returned projection.`} onRetry={onRetry} />}
      {projection.partial && <ProjectionNotice status="warning" title="Partial test projection" detail={`${projection.count} test records are visible. Source rows below identify unavailable or incomplete evidence.`} />}
      {projection.sources.length > 0 && <TestSources sources={projection.sources} />}
      {tests.length === 0 && <WorkbenchState kind={projection.partial ? 'stale' : 'empty'} title={projection.partial ? 'No complete test result set' : 'No test results'} detail={projection.partial ? 'Review the source availability and freshness records above before treating this workspace as tested.' : 'Local and CI results appear here when returned by /api/workspace/tests.'} onRetry={projection.partial ? onRetry : undefined} />}
      {tests.length > 0 && <>
      <PanelFilters>
        <label>Group by<select aria-label="Group tests" value={groupBy} onChange={(event) => setGroupBy(event.target.value as typeof groupBy)}><option value="source">Test scope</option><option value="suite">Suite</option><option value="status">Status</option><option value="project">Project</option></select></label>
        <label>Status<select aria-label="Filter test status" value={status} onChange={(event) => setStatus(event.target.value as typeof status)}><option value="all">All</option><option value="pass">Pass</option><option value="fail">Fail</option><option value="blocked">Blocked</option><option value="running">Running</option><option value="skipped">Skipped</option></select></label>
      </PanelFilters>
      {groups.length === 0 && <WorkbenchState kind="empty" title="No matching tests" detail="Change the status filter to include additional test results." />}
      {groups.map(([group, rows]) => <section key={group}><h3>{group}<span>{rows.length}</span></h3><div className="wb-panel-table">{rows.map((test) => <button type="button" key={test.id} onClick={() => onNavigate(test)}><StatusPill status={test.status === 'pass' ? 'verified' : test.status === 'fail' ? 'blocked' : test.status === 'running' ? 'derived' : 'warning'} label={test.status} /><strong>{test.name}</strong><span>{test.suite ?? 'Unclassified suite'}</span><code>{test.duration_ms ?? 0} ms</code></button>)}</div></section>)}
      </>}
    </div>
  );
};

const ProjectionNotice: React.FC<{
  status: 'warning' | 'blocked';
  title: string;
  detail: string;
  onRetry?: () => void;
}> = ({ status, title, detail, onRetry }) => (
  <div className={`wb-projection-notice wb-projection-notice--${status}`} role={status === 'blocked' ? 'alert' : 'status'}>
    <StatusPill status={status} label={title} />
    <span>{detail}</span>
    {onRetry && <button type="button" onClick={onRetry}>Retry</button>}
  </div>
);

const TestSources: React.FC<{ sources: WorkspaceTestSource[] }> = ({ sources }) => (
  <section className="wb-test-sources" aria-label="Test evidence sources">
    <h3>Evidence sources<span>{sources.length}</span></h3>
    <div className="wb-test-source-list">
      {sources.map((source, index) => {
        const freshness = source.freshness?.state ?? 'unknown';
        const detail = source.freshness?.error_code
          ? `${source.freshness.error_code}: ${source.freshness.reason ?? 'No evidence source was configured.'}`
          : freshnessDetail(source);
        return (
          <div className="wb-test-source-row" key={`${source.project_id ?? 'workspace'}:${source.execution_source}:${index}`}>
            <StatusPill status={sourceStatus(source)} label={source.status} />
            <strong>{source.project_id ?? 'Workspace'}</strong>
            <span>{source.execution_source} | freshness: {freshness} | {source.test_count} tests</span>
            <code title={detail}>{detail}</code>
          </div>
        );
      })}
    </div>
  </section>
);

function sourceStatus(source: WorkspaceTestSource): 'verified' | 'warning' | 'blocked' | 'stale' {
  const state = source.freshness?.state?.toLowerCase();
  if (source.status.toLowerCase() === 'unavailable' || state === 'unavailable' || state === 'blocked') return 'blocked';
  if (state === 'stale') return 'stale';
  if (state === 'current') return 'verified';
  return 'warning';
}

function freshnessDetail(source: WorkspaceTestSource): string {
  const runs = source.freshness?.runs ?? [];
  if (runs.length === 0) return source.freshness?.reason ?? 'Run freshness was not recorded.';
  const stateCounts = new Map<string, number>();
  runs.forEach((run) => {
    const state = run.freshness?.state ?? 'unknown';
    stateCounts.set(state, (stateCounts.get(state) ?? 0) + 1);
  });
  return Array.from(stateCounts.entries()).map(([state, count]) => `${count} ${state}`).join(', ');
}

const PanelFilters: React.FC<{ children: React.ReactNode }> = ({ children }) => <div className="wb-panel-filters">{children}</div>;

function handlePanelTabKey(
  event: React.KeyboardEvent<HTMLButtonElement>,
  current: BottomPanelId,
  onChange: (panel: BottomPanelId) => void,
) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  const currentIndex = panels.findIndex((panel) => panel.id === current);
  const targetIndex = event.key === 'Home'
    ? 0
    : event.key === 'End'
      ? panels.length - 1
      : (currentIndex + (event.key === 'ArrowRight' ? 1 : -1) + panels.length) % panels.length;
  const target = panels[targetIndex];
  if (!target) return;
  event.preventDefault();
  onChange(target.id);
  window.setTimeout(() => document.getElementById(`wb-bottom-tab-${target.id}`)?.focus(), 0);
}

function groupItems<T>(items: T[], getGroup: (item: T) => string): Array<[string, T[]]> {
  const groups = new Map<string, T[]>();
  items.forEach((item) => {
    const group = getGroup(item);
    groups.set(group, [...(groups.get(group) ?? []), item]);
  });
  return Array.from(groups.entries()).sort(([left], [right]) => left.localeCompare(right));
}

function testExecutionSource(test: WorkspaceTest): string {
  const scope = test.test_scope?.trim().toLowerCase();
  if (scope === 'library' || scope === 'unit') return 'Library';
  if (scope === 'integration') return 'Integration';
  if (scope === 'canonical_example' || scope === 'canonical-example' || scope === 'example') return 'Canonical example';
  if (scope === 'delivery_project' || scope === 'delivery-project') return 'Delivery project';
  const explicit = test.execution_source?.trim().toLowerCase();
  if (explicit === 'library' || explicit === 'unit') return 'Library';
  if (explicit === 'integration') return 'Integration';
  if (explicit === 'canonical_example' || explicit === 'canonical-example' || explicit === 'example') return 'Canonical example';
  if (explicit === 'delivery_project' || explicit === 'delivery-project') return 'Delivery project';
  const description = `${test.suite ?? ''} ${test.name} ${test.artifact_ref ?? ''}`.toLowerCase();
  if (/canonical|examples?[\\/._ -]/.test(description)) return 'Canonical example';
  if (/integration|end.to.end|e2e/.test(description)) return 'Integration';
  if (/library|unit|cargo test --lib/.test(description)) return 'Library';
  if (test.project_id) return 'Delivery project';
  return explicit ? explicit.replaceAll('_', ' ') : 'Unclassified source';
}

const VerificationPanel: React.FC<{ stages: VerificationStage[] }> = ({ stages }) => stages.length === 0 ? (
  <WorkbenchState kind="empty" title="No verification output" detail="Select a project with compiler verification artifacts." />
) : (
  <div className="wb-panel-table">{stages.map((stage) => <button type="button" key={stage.stage}><StatusPill status={stage.status} /><strong>{stage.stage}</strong><span>{stage.message ?? stage.producer ?? 'Artifact-backed result'}</span><code>{stage.diagnostic_code ?? stage.artifact_ref}</code></button>)}</div>
);

const TerminalPanel: React.FC = () => (
  <div className="wb-terminal" aria-label="Terminal output"><div><span>rustplc</span> project-check &lt;source&gt; --require-process-model</div><div className="wb-terminal-muted">Terminal execution output will stream here when a project command is active.</div><div><span>›</span><span className="wb-terminal-cursor" aria-hidden="true" /></div></div>
);

const AuditPanel: React.FC<{ evidence: EvidenceRecord[] }> = ({ evidence }) => evidence.length === 0 ? (
  <WorkbenchState kind="empty" title="No audit records" detail="Append-only run, signature, and evidence events appear here when supplied by the API." />
) : (
  <div className="wb-panel-table">{evidence.slice(0, 20).map((item) => <button type="button" key={item.evidence_id}><StatusPill status={item.evidence_state} /><strong>{item.producer ?? 'Unknown producer'}</strong><span>{item.label}</span><code>{item.timestamp ?? item.source_commit}</code></button>)}</div>
);

export default BottomPanel;
