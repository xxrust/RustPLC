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
import type { EvidenceRecord, VerificationStage, WorkspaceProblem, WorkspaceTest } from '../../types/workbench';
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
  problems: WorkspaceProblem[];
  tests: WorkspaceTest[];
  verification: VerificationStage[];
  evidence: EvidenceRecord[];
  onChange: (panel: BottomPanelId) => void;
  onCollapse: () => void;
  onNavigateProblem: (problem: WorkspaceProblem) => void;
  onNavigateTest: (test: WorkspaceTest) => void;
}> = ({ active, problems, tests, verification, evidence, onChange, onCollapse, onNavigateProblem, onNavigateTest }) => (
  <section className="wb-bottom-panel" aria-label="Workbench bottom panel">
    <div className="wb-bottom-tabs" role="tablist">
      {panels.map((panel) => (
        <button key={panel.id} data-bottom-panel={panel.id} type="button" role="tab" aria-selected={active === panel.id} className={active === panel.id ? 'is-active' : undefined} onClick={() => onChange(panel.id)}>
          {panel.icon}{panel.label}
          {panel.id === 'problems' && problems.length > 0 && <span>{problems.length}</span>}
          {panel.id === 'tests' && tests.length > 0 && <span>{tests.length}</span>}
        </button>
      ))}
      <button className="wb-bottom-close" type="button" aria-label="Collapse bottom panel" onClick={onCollapse}><CloseOutlined /></button>
    </div>
    <div className="wb-bottom-content" role="tabpanel">
      {active === 'problems' && <ProblemsPanel problems={problems} onNavigate={onNavigateProblem} />}
      {active === 'tests' && <TestsPanel tests={tests} onNavigate={onNavigateTest} />}
      {active === 'verification' && <VerificationPanel stages={verification} />}
      {active === 'terminal' && <TerminalPanel />}
      {active === 'audit' && <AuditPanel evidence={evidence} />}
    </div>
  </section>
);

const ProblemsPanel: React.FC<{ problems: WorkspaceProblem[]; onNavigate: (problem: WorkspaceProblem) => void }> = ({ problems, onNavigate }) => {
  const [groupBy, setGroupBy] = useState<'project' | 'stage' | 'code'>('stage');
  const [severity, setSeverity] = useState<'all' | WorkspaceProblem['severity']>('all');
  const groups = useMemo(() => {
    const filtered = severity === 'all' ? problems : problems.filter((problem) => problem.severity === severity);
    return groupItems(filtered, (problem) => {
      if (groupBy === 'project') return problem.project_id ?? 'Unassigned project';
      if (groupBy === 'code') return problem.code ?? 'No diagnostic code';
      return problem.stage ?? 'Unassigned stage';
    });
  }, [groupBy, problems, severity]);

  if (problems.length === 0) return <WorkbenchState kind="empty" title="No indexed problems" detail="Compiler diagnostics and project blockers appear here without being converted into frontend verdicts." />;
  return (
    <div className="wb-grouped-panel">
      <PanelFilters>
        <label>Group by<select aria-label="Group problems" value={groupBy} onChange={(event) => setGroupBy(event.target.value as typeof groupBy)}><option value="stage">Stage</option><option value="project">Project</option><option value="code">Diagnostic code</option></select></label>
        <label>Severity<select aria-label="Filter problem severity" value={severity} onChange={(event) => setSeverity(event.target.value as typeof severity)}><option value="all">All</option><option value="blocked">Blocked</option><option value="error">Error</option><option value="warning">Warning</option><option value="info">Info</option></select></label>
      </PanelFilters>
      {groups.length === 0 && <WorkbenchState kind="empty" title="No matching problems" detail="Change the severity filter to include additional compiler diagnostics." />}
      {groups.map(([group, rows]) => <section key={group}><h3>{group}<span>{rows.length}</span></h3><div className="wb-panel-table">{rows.map((problem) => <button type="button" key={problem.id} onClick={() => onNavigate(problem)}><span className={`wb-problem-icon wb-problem-icon--${problem.severity}`}>●</span><strong>{problem.code ?? problem.severity}</strong><span>{problem.message}</span><code>{problem.source_ref ?? problem.stage ?? problem.project_id}</code></button>)}</div></section>)}
    </div>
  );
};

const TestsPanel: React.FC<{ tests: WorkspaceTest[]; onNavigate: (test: WorkspaceTest) => void }> = ({ tests, onNavigate }) => {
  const [groupBy, setGroupBy] = useState<'suite' | 'status' | 'project'>('suite');
  const [status, setStatus] = useState<'all' | WorkspaceTest['status']>('all');
  const groups = useMemo(() => {
    const filtered = status === 'all' ? tests : tests.filter((test) => test.status === status);
    return groupItems(filtered, (test) => groupBy === 'status' ? test.status : groupBy === 'project' ? test.project_id ?? 'Unassigned project' : test.suite ?? 'Unclassified suite');
  }, [groupBy, status, tests]);

  if (tests.length === 0) return <WorkbenchState kind="empty" title="No test results" detail="Local and CI results appear here when returned by /api/workspace/tests." />;
  return (
    <div className="wb-grouped-panel">
      <PanelFilters>
        <label>Group by<select aria-label="Group tests" value={groupBy} onChange={(event) => setGroupBy(event.target.value as typeof groupBy)}><option value="suite">Suite</option><option value="status">Status</option><option value="project">Project</option></select></label>
        <label>Status<select aria-label="Filter test status" value={status} onChange={(event) => setStatus(event.target.value as typeof status)}><option value="all">All</option><option value="pass">Pass</option><option value="fail">Fail</option><option value="blocked">Blocked</option><option value="running">Running</option><option value="skipped">Skipped</option></select></label>
      </PanelFilters>
      {groups.length === 0 && <WorkbenchState kind="empty" title="No matching tests" detail="Change the status filter to include additional test results." />}
      {groups.map(([group, rows]) => <section key={group}><h3>{group}<span>{rows.length}</span></h3><div className="wb-panel-table">{rows.map((test) => <button type="button" key={test.id} onClick={() => onNavigate(test)}><StatusPill status={test.status === 'pass' ? 'verified' : test.status === 'fail' ? 'blocked' : test.status === 'running' ? 'derived' : 'warning'} label={test.status} /><strong>{test.name}</strong><span>{test.suite ?? 'Unclassified suite'}</span><code>{test.duration_ms ?? 0} ms</code></button>)}</div></section>)}
    </div>
  );
};

const PanelFilters: React.FC<{ children: React.ReactNode }> = ({ children }) => <div className="wb-panel-filters">{children}</div>;

function groupItems<T>(items: T[], getGroup: (item: T) => string): Array<[string, T[]]> {
  const groups = new Map<string, T[]>();
  items.forEach((item) => {
    const group = getGroup(item);
    groups.set(group, [...(groups.get(group) ?? []), item]);
  });
  return Array.from(groups.entries()).sort(([left], [right]) => left.localeCompare(right));
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
