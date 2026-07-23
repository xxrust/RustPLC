import React, { useMemo, useState } from 'react';
import {
  ApartmentOutlined,
  AuditOutlined,
  CaretDownOutlined,
  CaretRightOutlined,
  CodeOutlined,
  FileTextOutlined,
  FolderOutlined,
  HistoryOutlined,
  NodeIndexOutlined,
  SafetyCertificateOutlined,
  SearchOutlined,
} from '@ant-design/icons';
import type { ActivityId } from '../../stores/workbenchStore';
import type {
  AgentRun,
  DeliveryProjectDetail,
  DeliveryProjectSummary,
  EvidenceRecord,
  VerificationStage,
  WiringPoint,
  WorkbenchView,
} from '../../types/workbench';
import { StatusPill, WorkbenchState } from './WorkbenchPrimitives';
import { shortCommit } from './workbenchUtils';

interface ProjectExplorerProps {
  activity: ActivityId;
  projects: DeliveryProjectSummary[];
  selectedProjectId: string | null;
  project?: DeliveryProjectDetail;
  runs: AgentRun[];
  wiring: WiringPoint[];
  verification: VerificationStage[];
  evidence: EvidenceRecord[];
  loading: boolean;
  error: boolean;
  onRetry: () => void;
  onSelectProject: (projectId: string) => void;
  onSelectRun: (runId: string) => void;
  onOpenView: (view: WorkbenchView, label: string, resourceId?: string) => void;
  searchQuery: string;
  onSearchQueryChange: (query: string) => void;
  onSubmitSearch: (query: string) => void;
}

const ProjectExplorer: React.FC<ProjectExplorerProps> = (props) => {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({
    contract: true,
    agent: true,
    plc: true,
    wiring: true,
    verification: true,
    execution: true,
    release: true,
  });

  if (props.loading) {
    return <div className="wb-explorer"><ExplorerHeader title="Projects" /><div className="wb-tree-skeleton">{Array.from({ length: 9 }, (_, index) => <span key={index} />)}</div></div>;
  }
  if (props.error) {
    return <div className="wb-explorer"><ExplorerHeader title="Projects" /><WorkbenchState kind="error" title="Delivery registry unavailable" detail="The workbench could not load /api/delivery-projects." onRetry={props.onRetry} /></div>;
  }
  if (props.projects.length === 0) {
    return <div className="wb-explorer"><ExplorerHeader title="Projects" /><WorkbenchState kind="empty" title="No delivery projects" detail="Register or import a delivery project to populate the workspace." /></div>;
  }

  const toggle = (key: string) => setExpanded((state) => ({ ...state, [key]: !state[key] }));

  return (
    <aside className="wb-explorer" aria-label="Primary sidebar">
      <ExplorerHeader title={activityTitle(props.activity)} />
      {props.activity === 'projects' && (
        <ProjectTree {...props} expanded={expanded} onToggle={toggle} />
      )}
      {props.activity === 'runs' && (
        <RunList runs={props.runs} onSelectRun={props.onSelectRun} onOpenView={props.onOpenView} />
      )}
      {props.activity === 'wiring' && (
        <CompactEvidenceList title="Wiring points" rows={props.wiring.map((point) => ({ id: point.point_id, label: point.alias ?? point.point_id, detail: `${point.channel ?? 'channel?'} → ${point.device_terminal ?? 'unbound'}`, status: point.compiler_status }))} onOpen={() => props.onOpenView('wiring', 'Wiring')} />
      )}
      {props.activity === 'verification' && (
        <CompactEvidenceList title="Compiler stages" rows={props.verification.map((stage) => ({ id: stage.stage, label: stage.stage, detail: stage.diagnostic_code ?? stage.message ?? 'artifact-backed', status: stage.status }))} onOpen={() => props.onOpenView('verification', 'Verification & Evidence')} />
      )}
      {props.activity === 'evidence' && (
        <CompactEvidenceList title="Evidence records" rows={props.evidence.map((item) => ({ id: item.evidence_id, label: item.label, detail: item.producer ?? item.artifact_ref ?? 'unknown producer', status: item.evidence_state }))} onOpen={() => props.onOpenView('verification', 'Verification & Evidence')} />
      )}
      {props.activity === 'search' && (
        <SearchExplorer
          value={props.searchQuery}
          onChange={props.onSearchQueryChange}
          onSubmit={props.onSubmitSearch}
        />
      )}
      {props.activity === 'source-control' && <SourceControlExplorer project={props.project} />}
    </aside>
  );
};

const ExplorerHeader: React.FC<{ title: string }> = ({ title }) => (
  <div className="wb-explorer-header"><strong>{title}</strong><button type="button" aria-label={`More ${title} actions`}>•••</button></div>
);

function activityTitle(activity: ActivityId): string {
  const labels: Record<ActivityId, string> = {
    projects: 'Explorer', runs: 'Agent Runs', wiring: 'Wiring', verification: 'Verification', evidence: 'Evidence', search: 'Search', 'source-control': 'Source Control',
  };
  return labels[activity];
}

type ProjectTreeProps = ProjectExplorerProps & {
  expanded: Record<string, boolean>;
  onToggle: (key: string) => void;
};

const ProjectTree: React.FC<ProjectTreeProps> = ({
  projects, selectedProjectId, project, onSelectProject, onOpenView, expanded, onToggle,
}) => (
  <div className="wb-tree">
    <div className="wb-project-switcher">
      {projects.map((item) => (
        <button key={item.project_id} data-project-id={item.project_id} type="button" className={selectedProjectId === item.project_id ? 'is-selected' : undefined} onClick={() => onSelectProject(item.project_id)}>
          <span><FolderOutlined /> {item.name ?? item.project_id}</span>
          <StatusPill status={item.stale ? 'stale' : item.status} />
        </button>
      ))}
    </div>
    {project && (
      <div className="wb-tree-sections">
        <TreeSection id="contract" label="Contract" expanded={expanded.contract} onToggle={onToggle}>
          <TreeLeaf icon={<FileTextOutlined />} label="Project Overview" onClick={() => onOpenView('overview', 'Project Overview')} />
          <TreeLeaf icon={<FileTextOutlined />} label={project.system_contract ?? 'system.md'} onClick={() => onOpenView('source', 'System Contract', project.system_contract)} />
        </TreeSection>
        <TreeSection id="agent" label="Agent Runs" expanded={expanded.agent} onToggle={onToggle}>
          <TreeLeaf icon={<HistoryOutlined />} label="Run Timeline" onClick={() => onOpenView('agent-run', 'Agent Run Timeline')} />
          <TreeLeaf icon={<AuditOutlined />} label="Audit Log" onClick={() => onOpenView('audit', 'Audit Log')} />
        </TreeSection>
        <TreeSection id="plc" label="PLC" expanded={expanded.plc} onToggle={onToggle}>
          <TreeLeaf icon={<CodeOutlined />} label={project.source_entry ?? 'Source Bundle'} onClick={() => onOpenView('source', 'PLC Source')} />
          <TreeLeaf icon={<NodeIndexOutlined />} label="Topology" onClick={() => onOpenView('topology', 'Topology')} />
        </TreeSection>
        <TreeSection id="wiring" label="Wiring" expanded={expanded.wiring} onToggle={onToggle}>
          <TreeLeaf icon={<ApartmentOutlined />} label="Controller I/O and Point Checks" onClick={() => onOpenView('wiring', 'Wiring')} />
        </TreeSection>
        <TreeSection id="verification" label="Verification" expanded={expanded.verification} onToggle={onToggle}>
          <TreeLeaf icon={<SafetyCertificateOutlined />} label="Formal and Observed Evidence" onClick={() => onOpenView('verification', 'Verification & Evidence')} />
        </TreeSection>
        <TreeSection id="execution" label="Execution" expanded={expanded.execution} onToggle={onToggle}>
          <TreeLeaf icon={<HistoryOutlined />} label="Run" onClick={() => onOpenView('run', 'Run')} />
          <TreeLeaf icon={<HistoryOutlined />} label="Trace Replay" onClick={() => onOpenView('replay', 'Trace Replay')} />
        </TreeSection>
        <TreeSection id="release" label="Release" expanded={expanded.release} onToggle={onToggle}>
          <TreeLeaf icon={<SafetyCertificateOutlined />} label={project.release_verdict ?? 'Release verdict pending'} onClick={() => onOpenView('overview', 'Project Overview')} />
        </TreeSection>
      </div>
    )}
  </div>
);

const TreeSection: React.FC<{ id: string; label: string; expanded?: boolean; onToggle: (key: string) => void; children: React.ReactNode }> = ({ id, label, expanded, onToggle, children }) => (
  <div className="wb-tree-section">
    <button type="button" onClick={() => onToggle(id)} aria-expanded={expanded}><span>{expanded ? <CaretDownOutlined /> : <CaretRightOutlined />}</span>{label}</button>
    {expanded && <div>{children}</div>}
  </div>
);

const TreeLeaf: React.FC<{ icon: React.ReactNode; label: string; onClick: () => void }> = ({ icon, label, onClick }) => (
  <button className="wb-tree-leaf" type="button" onClick={onClick}>{icon}<span>{label}</span></button>
);

const RunList: React.FC<{ runs: AgentRun[]; onSelectRun: (runId: string) => void; onOpenView: ProjectExplorerProps['onOpenView'] }> = ({ runs, onSelectRun, onOpenView }) => {
  if (runs.length === 0) return <WorkbenchState kind="empty" title="No agent runs" detail="Imported and executed runs appear here with provenance and anomaly records." />;
  return <div className="wb-compact-list">{runs.map((run) => <button key={run.run_id} type="button" onClick={() => { onSelectRun(run.run_id); onOpenView('agent-run', `Run ${run.run_id}`, run.run_id); }}><span><strong>{run.run_id}</strong><small>{run.unattended_verdict ?? run.status ?? 'unreported'}</small></span><StatusPill status={run.status === 'blocked' ? 'blocked' : run.status === 'failed' ? 'warning' : 'observed'} /></button>)}</div>;
};

const CompactEvidenceList: React.FC<{ title: string; rows: Array<{ id: string; label: string; detail: string; status?: Parameters<typeof StatusPill>[0]['status'] }>; onOpen: () => void }> = ({ title, rows, onOpen }) => (
  <div className="wb-compact-list"><div className="wb-list-caption">{title}<span>{rows.length}</span></div>{rows.length > 0 ? rows.map((row) => <button type="button" key={row.id} onClick={onOpen}><span><strong>{row.label}</strong><small>{row.detail}</small></span><StatusPill status={row.status} /></button>) : <WorkbenchState kind="empty" title={`No ${title.toLowerCase()}`} detail="The delivery-project API returned no records for this project." />}</div>
);

const SearchExplorer: React.FC<{
  value: string;
  onChange: (value: string) => void;
  onSubmit: (value: string) => void;
}> = ({ value, onChange, onSubmit }) => {
  const examples = useMemo(() => ['stage:codegen status:blocked', 'diagnostic:SEM-110', 'evidence:stale'], []);
  return (
    <div className="wb-search-explorer">
      <label>
        <SearchOutlined />
        <span className="wb-visually-hidden">Search project evidence</span>
        <input
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => { if (event.key === 'Enter') onSubmit(value); }}
          placeholder="Search project evidence"
        />
      </label>
      <p>Queries search projects, runs, diagnostics, semantic objects, and holds.</p>
      {examples.map((example) => (
        <button type="button" key={example} onClick={() => { onChange(example); onSubmit(example); }}>
          {example}
        </button>
      ))}
    </div>
  );
};

const SourceControlExplorer: React.FC<{ project?: DeliveryProjectDetail }> = ({ project }) => (
  <div className="wb-source-control"><div><strong>{shortCommit(project?.source_commit)}</strong><span>Selected project revision</span></div><p>Workspace cleanliness and attributed file changes are supplied by run provenance. The frontend does not infer authorship.</p></div>
);

export default ProjectExplorer;
