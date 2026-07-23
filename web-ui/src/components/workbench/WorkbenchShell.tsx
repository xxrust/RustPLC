import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  BellOutlined,
  BranchesOutlined,
  CloseOutlined,
  LayoutOutlined,
  LeftOutlined,
  MacCommandOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  RightOutlined,
} from '@ant-design/icons';
import ActivityBar from './ActivityBar';
import ProjectExplorer from './ProjectExplorer';
import EvidenceInspector from './EvidenceInspector';
import BottomPanel from './BottomPanel';
import CommandPalette, { type WorkbenchCommand } from './CommandPalette';
import { WorkbenchState, StatusPill } from './WorkbenchPrimitives';
import { shortCommit } from './workbenchUtils';
import ProjectOverviewView from '../../pages/workbench/ProjectOverviewView';
import AgentRunTimelineView from '../../pages/workbench/AgentRunTimelineView';
import VerificationEvidenceView from '../../pages/workbench/VerificationEvidenceView';
import ArtifactSourceView from './ArtifactSourceView';
import WiringPointChecksView from './WiringPointChecksView';
import GeometryPreview from '../geometry/GeometryPreview';
import RunPage from '../../pages/RunPage';
import ReplayPage from '../../pages/ReplayPage';
import AuditPage from '../../pages/AuditPage';
import { deliveryProjectApi } from '../../services/api';
import { useAppStore } from '../../stores/appStore';
import { useWorkbenchStore } from '../../stores/workbenchStore';
import type {
  EvidenceState,
  HoldSignatureContext,
  HoldProjectionItem,
  HumanHold,
  PointCheckProjectionPoint,
  RecordPointObservationRequest,
  SignHoldRequest,
  WiringDiagnostic,
  WorkbenchTab,
  WorkspaceProblem,
  WorkspaceProblemsProjection,
  WorkspaceTest,
  WorkspaceTestsProjection,
} from '../../types/workbench';
import './workbench.css';

const WorkbenchShell: React.FC = () => {
  const workbench = useWorkbenchStore();
  const queryClient = useQueryClient();
  const setCurrentProject = useAppStore((state) => state.setCurrentProject);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const nextProblemIndex = useRef(0);
  const selectedProjectId = workbench.selectedProjectId;
  const selectedRunId = workbench.selectedRunId;
  const setSelectedProject = workbench.setSelectedProject;
  const setSelectedRun = workbench.setSelectedRun;

  const projectsQuery = useQuery({ queryKey: ['delivery-projects'], queryFn: deliveryProjectApi.listProjects });
  const projects = useMemo(() => projectsQuery.data ?? [], [projectsQuery.data]);
  const projectById = useMemo(
    () => new Map(projects.map((item) => [item.project_id, item])),
    [projects],
  );

  useEffect(() => {
    if (projects.length === 0) return;
    const liveSelection = useWorkbenchStore.getState().selectedProjectId;
    const currentExists = projects.some((project) => project.project_id === liveSelection);
    if (!currentExists) setSelectedProject(projects[0].project_id);
  }, [projects, setSelectedProject]);

  useEffect(() => {
    setCurrentProject(selectedProjectId);
  }, [setCurrentProject, selectedProjectId]);

  const projectId = selectedProjectId;
  const projectQuery = useQuery({ queryKey: ['delivery-project', projectId], queryFn: () => deliveryProjectApi.getProject(projectId!), enabled: Boolean(projectId) });
  const runsQuery = useQuery({ queryKey: ['delivery-runs', projectId], queryFn: () => deliveryProjectApi.listRuns(projectId!), enabled: Boolean(projectId) });
  const wiringQuery = useQuery({ queryKey: ['delivery-wiring', projectId], queryFn: () => deliveryProjectApi.getWiring(projectId!), enabled: Boolean(projectId) });
  const physicalEvidenceQuery = useQuery({ queryKey: ['delivery-physical-evidence', projectId], queryFn: () => deliveryProjectApi.getPhysicalEvidence(projectId!), enabled: Boolean(projectId) });
  const holdProjectionQuery = useQuery({ queryKey: ['delivery-hold-projection', projectId], queryFn: () => deliveryProjectApi.getHoldProjection(projectId!), enabled: Boolean(projectId) });
  const releaseProjectionQuery = useQuery({ queryKey: ['delivery-release-projection', projectId], queryFn: () => deliveryProjectApi.getReleaseProjection(projectId!), enabled: Boolean(projectId) });
  const verificationQuery = useQuery({ queryKey: ['delivery-verification', projectId], queryFn: () => deliveryProjectApi.getVerification(projectId!), enabled: Boolean(projectId) });
  const evidenceQuery = useQuery({ queryKey: ['delivery-evidence', projectId], queryFn: () => deliveryProjectApi.getEvidence(projectId!), enabled: Boolean(projectId) });
  const geometryQuery = useQuery({ queryKey: ['delivery-geometry', projectId], queryFn: () => deliveryProjectApi.getGeometry(projectId!), enabled: Boolean(projectId) });
  const signaturesQuery = useQuery({
    queryKey: ['delivery-signatures', projectId],
    queryFn: () => deliveryProjectApi.getSignatures(projectId!),
    enabled: Boolean(projectId),
  });
  const problemsQuery = useQuery({ queryKey: ['workspace-problems'], queryFn: deliveryProjectApi.getWorkspaceProblems });
  const testsQuery = useQuery({ queryKey: ['workspace-tests'], queryFn: deliveryProjectApi.getWorkspaceTests });
  const signMutation = useMutation({
    mutationFn: ({ holdId, request }: { holdId: string; request: SignHoldRequest }) => deliveryProjectApi.signHold(projectId!, holdId, request),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['delivery-signatures', projectId] }),
        queryClient.invalidateQueries({ queryKey: ['delivery-project', projectId] }),
        queryClient.invalidateQueries({ queryKey: ['delivery-hold-projection', projectId] }),
        queryClient.invalidateQueries({ queryKey: ['delivery-release-projection', projectId] }),
      ]);
    },
  });
  const pointObservationMutation = useMutation({
    mutationFn: async ({ pointId, request, photo }: { pointId: string; request: RecordPointObservationRequest; photo?: File }) => {
      const upload = photo ? await deliveryProjectApi.uploadPointPhoto(projectId!, pointId, photo) : undefined;
      return deliveryProjectApi.recordPointObservation(projectId!, pointId, {
        ...request,
        photo_upload_id: upload?.upload_id,
      });
    },
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['delivery-physical-evidence', projectId] }),
        queryClient.invalidateQueries({ queryKey: ['delivery-hold-projection', projectId] }),
        queryClient.invalidateQueries({ queryKey: ['delivery-release-projection', projectId] }),
        queryClient.invalidateQueries({ queryKey: ['delivery-evidence', projectId] }),
        queryClient.invalidateQueries({ queryKey: ['delivery-project', projectId] }),
      ]);
    },
  });

  const runs = useMemo(() => runsQuery.data ?? [], [runsQuery.data]);
  useEffect(() => {
    if (runs.length === 0) return;
    const selectedExists = runs.some((run) => run.run_id === selectedRunId);
    if (!selectedExists) setSelectedRun(runs[0].run_id);
  }, [runs, selectedRunId, setSelectedRun]);

  const runQuery = useQuery({
    queryKey: ['delivery-run', projectId, selectedRunId],
    queryFn: () => deliveryProjectApi.getRun(projectId!, selectedRunId!),
    enabled: Boolean(projectId && selectedRunId),
  });

  const project = useMemo(() => {
    const base = projectQuery.data;
    if (!base) return undefined;
    const projectedHolds = holdProjectionQuery.data?.holds.map(normalizeHoldProjection);
    return {
      ...base,
      human_holds: projectedHolds ?? base.human_holds,
      release_verdict: releaseProjectionQuery.data?.status ?? base.release_verdict,
    };
  }, [holdProjectionQuery.data, projectQuery.data, releaseProjectionQuery.data?.status]);
  const projectedWiring = useMemo<PointCheckProjectionPoint[]>(() => {
    const points = physicalEvidenceQuery.data?.point_checks.points;
    const authoredPoints = wiringQuery.data?.points ?? [];
    if (points) {
      const authoredById = new Map(authoredPoints.map((authored) => [authored.point_id, authored]));
      return points.map((point) => ({
        ...point,
        authored: {
          ...authoredById.get(point.point_id),
          ...point.authored,
        },
      }));
    }
    return authoredPoints.map((authored) => ({
      point_id: authored.point_id,
      authored,
      status: authored.point_check_status ?? 'pending',
      evidence_state: authored.point_check_status === 'observed' ? 'observed' : 'authored',
      responsibility_state: 'human_action_required',
    }));
  }, [physicalEvidenceQuery.data?.point_checks.points, wiringQuery.data?.points]);
  const verification = useMemo(() => verificationQuery.data ?? [], [verificationQuery.data]);
  const evidence = useMemo(() => evidenceQuery.data ?? [], [evidenceQuery.data]);
  const problemsProjection = useMemo<WorkspaceProblemsProjection>(
    () => problemsQuery.data ?? { count: 0, partial: false, problems: [] },
    [problemsQuery.data],
  );
  const testsProjection = useMemo<WorkspaceTestsProjection>(
    () => testsQuery.data ?? { count: 0, partial: false, sources: [], tests: [] },
    [testsQuery.data],
  );
  const problems = problemsProjection.problems;
  const tests = testsProjection.tests;
  const primaryTabs = workbench.tabs.filter((tab) => (tab.group ?? 'primary') === 'primary');
  const secondaryTabs = workbench.tabs.filter((tab) => tab.group === 'secondary');
  const primaryActiveTab = primaryTabs.find((tab) => tab.id === workbench.activeTabId) ?? primaryTabs[0];
  const secondaryActiveTab = secondaryTabs.find((tab) => tab.id === workbench.secondaryActiveTabId) ?? secondaryTabs[0];
  const activeTab = workbench.activeGroup === 'secondary' && secondaryActiveTab ? secondaryActiveTab : primaryActiveTab;
  const openProblems = problems.filter((problem) => problem.severity === 'error' || problem.severity === 'blocked').length;
  const projectLoading = Boolean(projectId && projectQuery.isLoading);
  const projectError = Boolean(projectId && projectQuery.isError);
  const pointObservationError = pointObservationMutation.error instanceof Error ? pointObservationMutation.error.message : undefined;

  const refreshProject = () => {
    void Promise.all([
      projectsQuery.refetch(), projectQuery.refetch(), runsQuery.refetch(), wiringQuery.refetch(), physicalEvidenceQuery.refetch(),
      holdProjectionQuery.refetch(), releaseProjectionQuery.refetch(),
      verificationQuery.refetch(), evidenceQuery.refetch(), problemsQuery.refetch(), testsQuery.refetch(),
      signaturesQuery.refetch(),
    ]);
  };

  const openArtifact = useCallback((artifactRef: string, label: string, line?: number, column?: number) => {
    const normalized = artifactRef.replace(/\\/g, '/');
    const location = line ? `${normalized}:${line}${column ? `:${column}` : ''}` : normalized;
    workbench.openView('source', label, location);
  }, [workbench]);

  const openProblem = useCallback((problem: WorkspaceProblem) => {
    if (problem.project_id) workbench.setSelectedProject(problem.project_id);
    const sourceRef = (problem.artifact_ref ?? problem.source_ref)?.replace(/\\/g, '/');
    if (sourceRef) {
      openArtifact(sourceRef, problem.code ?? resourceLabel(sourceRef), problem.line, problem.column);
      return;
    }
    workbench.openView('verification', problem.code ?? problem.stage ?? 'Problem evidence', problem.code ?? problem.id);
  }, [openArtifact, workbench]);

  const openTest = useCallback((test: WorkspaceTest) => {
    if (test.project_id) workbench.setSelectedProject(test.project_id);
    const artifact = test.artifact_ref?.replace(/\\/g, '/');
    const runMatch = artifact?.match(/\/runs\/([^/]+)/i);
    if (runMatch?.[1]) workbench.setSelectedRun(runMatch[1]);
    if (artifact) {
      openArtifact(artifact, test.name);
      return;
    }
    workbench.openView('verification', test.name, `${test.suite ?? 'tests'}:${test.id}`);
  }, [openArtifact, workbench]);

  const focusBottomPanel = useCallback((panel: typeof workbench.bottomPanel) => {
    workbench.setBottomPanel(panel);
    window.setTimeout(() => document.querySelector<HTMLButtonElement>(`[data-bottom-panel="${panel}"]`)?.focus(), 0);
  }, [workbench]);

  const openNextProblem = useCallback(() => {
    if (problems.length === 0) return;
    const problem = problems[nextProblemIndex.current % problems.length];
    nextProblemIndex.current = (nextProblemIndex.current + 1) % problems.length;
    focusBottomPanel('problems');
    openProblem(problem);
  }, [focusBottomPanel, openProblem, problems]);

  const openPalette = useCallback((query = '') => {
    workbench.setSearchQuery(query);
    setPaletteOpen(true);
  }, [workbench]);

  const commands = useMemo<WorkbenchCommand[]>(() => {
    const viewCommands: Array<[string, string, Parameters<typeof workbench.openView>[0], string?]> = [
      ['open-overview', 'Open Project Overview', 'overview'],
      ['open-agent-run', 'Open Agent Run Timeline', 'agent-run'],
      ['open-wiring', 'Open Controller I/O and Point Checks', 'wiring'],
      ['open-verification', 'Open Verification Evidence', 'verification'],
      ['open-source', 'Open PLC Source', 'source', project?.source_entry],
      ['open-topology', 'Open Topology', 'topology'],
      ['open-run', 'Open Run Controls', 'run'],
      ['open-replay', 'Open Replay', 'replay'],
      ['open-audit', 'Open Audit Log', 'audit'],
    ];
    const base: WorkbenchCommand[] = viewCommands.map(([id, label, view, resource]) => ({
      id,
      label,
      category: 'View',
      detail: resource,
      search: {
        project: [project?.project_id, project?.name],
        commit: project?.source_commit,
        category: 'view',
      },
      execute: () => workbench.openView(view, label.replace(/^Open /, ''), resource),
    }));
    base.push(
      { id: 'toggle-explorer', label: 'Toggle Explorer', category: 'Layout', shortcut: 'Ctrl B', execute: workbench.togglePrimarySidebar },
      { id: 'toggle-inspector', label: 'Toggle Evidence Inspector', category: 'Layout', execute: workbench.toggleInspector },
      { id: 'toggle-bottom', label: 'Toggle Bottom Panel', category: 'Layout', shortcut: 'Ctrl J', execute: workbench.toggleBottomPanel },
      { id: 'split-editor', label: 'Split Active Editor', category: 'Layout', shortcut: 'Ctrl \\', execute: workbench.splitActiveTab },
      { id: 'close-split', label: 'Close Editor Split', category: 'Layout', execute: workbench.closeSplit },
      { id: 'focus-problems', label: 'Focus Problems', category: 'Panel', shortcut: 'F8', execute: () => focusBottomPanel('problems') },
      { id: 'focus-tests', label: 'Focus Tests', category: 'Panel', execute: () => focusBottomPanel('tests') },
      { id: 'focus-verification', label: 'Focus Verification', category: 'Panel', execute: () => focusBottomPanel('verification') },
    );
    projects.forEach((item) => base.push({
      id: `project-${item.project_id}`,
      label: `Open ${item.name ?? item.project_id}`,
      category: 'Project',
      detail: item.delivery_layer,
      search: {
        project: [item.project_id, item.name],
        layer: item.delivery_layer,
        status: item.status ?? 'unknown',
        evidence: item.status ?? 'unknown',
        commit: item.source_commit,
        category: 'project',
      },
      execute: () => workbench.setSelectedProject(item.project_id),
    }));
    problems.forEach((problem, index) => {
      const owner = problem.project_id ? projectById.get(problem.project_id) : undefined;
      base.push({
        id: `problem-${problem.project_id ?? 'workspace'}-${problem.id}-${problem.stage ?? 'unknown'}-${index}`,
        label: problem.code ?? problem.message,
        category: 'Diagnostic',
        detail: problem.source_ref ?? problem.artifact_ref ?? problem.stage,
        searchText: problem.message,
        search: {
          diagnostic: [problem.code, problem.id],
          stage: problem.stage ?? 'unknown',
          status: problem.severity,
          evidence: problem.severity,
          project: [problem.project_id, owner?.name],
          commit: problem.source_commit ?? owner?.source_commit,
          category: 'diagnostic',
        },
        execute: () => openProblem(problem),
      });
    });
    tests.forEach((test, index) => {
      const owner = test.project_id ? projectById.get(test.project_id) : undefined;
      base.push({
        id: `test-${test.project_id ?? 'workspace'}-${test.id}-${test.suite ?? 'unknown'}-${index}`,
        label: test.name,
        category: 'Test',
        detail: test.artifact_ref ?? test.suite,
        search: {
          test: test.id,
          suite: test.suite ?? 'unknown',
          status: test.status,
          evidence: test.status,
          project: [test.project_id, owner?.name],
          commit: owner?.source_commit,
          category: 'test',
        },
        execute: () => openTest(test),
      });
    });
    verification.forEach((stage) => base.push({
      id: `verification-${stage.stage}`,
      label: `${stage.stage} verification evidence`,
      category: 'Verification',
      detail: stage.artifact_ref ?? stage.diagnostic_code,
      searchText: stage.message,
      search: {
        project: [project?.project_id, project?.name],
        stage: stage.stage,
        status: stage.status,
        evidence: stage.status,
        diagnostic: stage.diagnostic_code ?? 'none',
        producer: stage.producer ?? 'unknown',
        commit: stage.source_commit ?? project?.source_commit,
        category: 'verification',
      },
      execute: () => stage.artifact_ref
        ? openArtifact(stage.artifact_ref, `${stage.stage} evidence`)
        : workbench.openView('verification', `${stage.stage} evidence`, stage.stage),
    }));
    evidence.forEach((item) => base.push({
      id: `evidence-${item.evidence_id}`,
      label: item.label,
      category: 'Evidence',
      detail: item.artifact_ref ?? item.producer,
      search: {
        project: [project?.project_id, project?.name],
        evidence: [item.evidence_state, item.evidence_id],
        status: item.evidence_state,
        responsibility: item.responsibility_state ?? 'unknown',
        producer: item.producer ?? 'unknown',
        commit: item.source_commit ?? project?.source_commit,
        category: 'evidence',
      },
      execute: () => item.artifact_ref
        ? openArtifact(item.artifact_ref, item.label)
        : workbench.openView('verification', item.label, item.evidence_id),
    }));
    runs.forEach((item) => base.push({
      id: `run-${item.run_id}`,
      label: `Open agent run ${item.run_id}`,
      category: 'Agent Run',
      detail: item.unattended_verdict ?? item.status,
      search: {
        project: [project?.project_id, project?.name],
        run: item.run_id,
        status: item.status ?? 'unknown',
        evidence: item.status ?? 'unknown',
        verdict: item.unattended_verdict ?? 'unknown',
        model: item.model ?? 'unknown',
        commit: item.source_commit ?? project?.source_commit,
        category: 'agent run',
      },
      execute: () => {
        workbench.setSelectedRun(item.run_id);
        workbench.openView('agent-run', `Agent Run ${item.run_id}`, item.run_id);
      },
    }));
    return base;
  }, [evidence, focusBottomPanel, openArtifact, openProblem, openTest, problems, project, projectById, projects, runs, tests, verification, workbench]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const commandKey = event.ctrlKey || event.metaKey;
      if (commandKey && event.key.toLowerCase() === 'k') { event.preventDefault(); openPalette(); return; }
      if (commandKey && event.key.toLowerCase() === 'b') { event.preventDefault(); workbench.togglePrimarySidebar(); return; }
      if (commandKey && event.key.toLowerCase() === 'j') { event.preventDefault(); workbench.toggleBottomPanel(); return; }
      if (commandKey && event.key === '\\') { event.preventDefault(); workbench.splitActiveTab(); return; }
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'e') { event.preventDefault(); workbench.setActiveActivity('projects'); return; }
      if (event.key === 'F8') { event.preventDefault(); openNextProblem(); }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [openNextProblem, openPalette, workbench]);

  const renderEditor = (tab?: WorkbenchTab) => (
    <EditorContent
      tab={tab}
      project={project}
      run={runQuery.data ?? runs.find((run) => run.run_id === selectedRunId)}
      runs={runs}
      wiring={projectedWiring}
      wiringDiagnostics={wiringQuery.data?.diagnostics ?? []}
      pointCheckSummary={physicalEvidenceQuery.data?.point_checks.summary}
      recordingPointId={pointObservationMutation.isPending ? pointObservationMutation.variables?.pointId : undefined}
      pointObservationError={pointObservationError}
      verification={verification}
      evidence={evidence}
      geometry={geometryQuery.data}
      geometryLoading={geometryQuery.isLoading}
      evidenceFilter={workbench.evidenceFilter}
      signatureContext={signaturesQuery.data}
      signingHoldId={signMutation.isPending ? signMutation.variables?.holdId : undefined}
      onSign={async (holdId, request) => { await signMutation.mutateAsync({ holdId, request }); }}
      onRecordPoint={async (pointId, request, photo) => { await pointObservationMutation.mutateAsync({ pointId, request, photo }); }}
      onEvidenceFilterChange={workbench.setEvidenceFilter}
      loading={projectLoading}
      error={projectError}
      onRetry={refreshProject}
    />
  );

  const layoutStyle = {
    '--wb-sidebar-width': `${workbench.primarySidebarWidth}px`,
    '--wb-inspector-width': `${workbench.inspectorWidth}px`,
  } as React.CSSProperties;
  const editorStyle = {
    '--wb-bottom-height': `${workbench.bottomPanelHeight}px`,
  } as React.CSSProperties;

  return (
    <div className="wb-shell">
      <header className="wb-titlebar">
        <div className="wb-titlebar__identity"><strong>RustPLC</strong><span>Autonomous Delivery Workbench</span></div>
        <button className="wb-command-center" type="button" title="Open command palette" onClick={() => openPalette()}><MacCommandOutlined /><span>{project?.name ?? project?.project_id ?? 'Select a delivery project'}</span><kbd>Ctrl K</kbd></button>
        <div className="wb-titlebar__actions"><button type="button" title="Workspace notifications"><BellOutlined /><span className="wb-visually-hidden">Notifications</span></button><button type="button" title="Split active editor" onClick={workbench.splitActiveTab}><LayoutOutlined /><span className="wb-visually-hidden">Split active editor</span></button></div>
      </header>

      <div className={`wb-main-grid${workbench.primarySidebarCollapsed ? ' is-sidebar-collapsed' : ''}${workbench.inspectorCollapsed ? ' is-inspector-collapsed' : ''}`} style={layoutStyle}>
        <ActivityBar active={workbench.activeActivity} onChange={workbench.setActiveActivity} problemCount={openProblems} />

        {!workbench.primarySidebarCollapsed ? (
          <div className="wb-pane-frame">
            <ProjectExplorer
              activity={workbench.activeActivity}
              projects={projects}
              selectedProjectId={projectId}
              project={project}
              runs={runs}
              wiring={wiringQuery.data?.points ?? []}
              verification={verification}
              evidence={evidence}
              loading={projectsQuery.isLoading}
              error={projectsQuery.isError}
              onRetry={() => void projectsQuery.refetch()}
              onSelectProject={workbench.setSelectedProject}
              onSelectRun={workbench.setSelectedRun}
              onOpenView={workbench.openView}
              searchQuery={workbench.searchQuery}
              onSearchQueryChange={workbench.setSearchQuery}
              onSubmitSearch={openPalette}
            />
            <WorkbenchSeparator className="wb-resizer--pane-right" orientation="vertical" label="Resize Explorer" valueNow={workbench.primarySidebarWidth} valueMin={190} valueMax={420} onDelta={(x) => workbench.setPrimarySidebarWidth(workbench.primarySidebarWidth + x)} onKeyStep={(step) => workbench.setPrimarySidebarWidth(workbench.primarySidebarWidth + step * 10)} />
          </div>
        ) : (
          <button className="wb-pane-restore wb-pane-restore--left" type="button" onClick={workbench.togglePrimarySidebar} title="Show primary sidebar"><MenuUnfoldOutlined /></button>
        )}

        <main className="wb-editor-workspace">
          <div className={`wb-editor-body${workbench.bottomPanelCollapsed ? ' is-bottom-collapsed' : ''}`} style={editorStyle}>
            <div className={`wb-editor-groups${workbench.splitEnabled ? ' is-split' : ''}`} style={workbench.splitEnabled ? { gridTemplateColumns: `${workbench.splitRatio}fr 5px ${1 - workbench.splitRatio}fr` } : undefined}>
              <EditorGroup group="primary" tabs={primaryTabs} activeTab={primaryActiveTab} onActivate={workbench.setActiveTab} onClose={workbench.closeTab} onMove={workbench.moveTabToGroup} onDrop={workbench.moveTabToGroup} startControl={<button className="wb-sidebar-toggle" type="button" onClick={workbench.togglePrimarySidebar} title="Toggle Explorer">{workbench.primarySidebarCollapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}</button>} endControl={!workbench.splitEnabled ? <button className="wb-inspector-toggle" type="button" onClick={workbench.toggleInspector} title="Toggle evidence inspector"><RightOutlined /></button> : undefined}>
                {renderEditor(primaryActiveTab)}
              </EditorGroup>
              {workbench.splitEnabled && (
                <>
                  <WorkbenchSeparator className="wb-resizer--split" orientation="vertical" label="Resize editor groups" valueNow={Math.round(workbench.splitRatio * 100)} valueMin={28} valueMax={72} onDelta={(x) => {
                    const workspaceWidth = document.querySelector('.wb-editor-groups')?.getBoundingClientRect().width ?? 1;
                    workbench.setSplitRatio(workbench.splitRatio + x / workspaceWidth);
                  }} onKeyStep={(step) => workbench.setSplitRatio(workbench.splitRatio + step * 0.02)} />
                  <EditorGroup group="secondary" tabs={secondaryTabs} activeTab={secondaryActiveTab} onActivate={workbench.setActiveTab} onClose={workbench.closeTab} onMove={workbench.moveTabToGroup} onDrop={workbench.moveTabToGroup} endControl={<><button className="wb-group-control" type="button" onClick={workbench.closeSplit} title="Close editor split"><CloseOutlined /></button><button className="wb-inspector-toggle" type="button" onClick={workbench.toggleInspector} title="Toggle evidence inspector"><RightOutlined /></button></>}>
                    {renderEditor(secondaryActiveTab)}
                  </EditorGroup>
                </>
              )}
            </div>
            {!workbench.bottomPanelCollapsed ? (
              <>
                <WorkbenchSeparator className="wb-resizer--bottom" orientation="horizontal" label="Resize bottom panel" valueNow={workbench.bottomPanelHeight} valueMin={120} valueMax={460} onDelta={(_, y) => workbench.setBottomPanelHeight(workbench.bottomPanelHeight - y)} onKeyStep={(step) => workbench.setBottomPanelHeight(workbench.bottomPanelHeight + step * 10)} />
                <BottomPanel active={workbench.bottomPanel} problems={problemsProjection} tests={testsProjection} problemsRequestError={queryErrorMessage(problemsQuery.error)} testsRequestError={queryErrorMessage(testsQuery.error)} verification={verification} evidence={evidence} projects={projects} onChange={workbench.setBottomPanel} onCollapse={workbench.toggleBottomPanel} onNavigateProblem={openProblem} onNavigateTest={openTest} onRetryProblems={() => { void problemsQuery.refetch(); }} onRetryTests={() => { void testsQuery.refetch(); }} />
              </>
            ) : (
              <button className="wb-bottom-restore" type="button" onClick={workbench.toggleBottomPanel}>Problems {problemsProjection.count} · Tests {testsProjection.count} · Verification {verification.length}</button>
            )}
          </div>
        </main>

        {!workbench.inspectorCollapsed ? (
          <div className="wb-pane-frame">
            <WorkbenchSeparator className="wb-resizer--pane-left" orientation="vertical" label="Resize Evidence Inspector" valueNow={workbench.inspectorWidth} valueMin={240} valueMax={460} onDelta={(x) => workbench.setInspectorWidth(workbench.inspectorWidth - x)} onKeyStep={(step) => workbench.setInspectorWidth(workbench.inspectorWidth + step * 10)} />
            <EvidenceInspector project={project} releaseProjection={releaseProjectionQuery.data} activeTab={activeTab} evidence={evidence} onCollapse={workbench.toggleInspector} />
          </div>
        ) : (
          <button className="wb-pane-restore wb-pane-restore--right" type="button" onClick={workbench.toggleInspector} title="Show evidence inspector"><RightOutlined /></button>
        )}
      </div>

      <footer className="wb-statusbar">
        <span><BranchesOutlined /> {shortCommit(project?.source_commit)}</span>
        <span>{project?.stale ? <StatusPill status="stale" /> : 'Workspace current'}</span>
        <span>Agent {runQuery.data?.status ?? runs[0]?.status ?? 'idle'}</span>
        <span>Verification {verification.filter((stage) => stage.status === 'verified').length}/{verification.length}</span>
        <span>HIL {evidence.some((item) => item.evidence_state === 'observed') ? 'evidence present' : 'unobserved'}</span>
        <span className="wb-statusbar__right">Holds {project?.human_holds?.filter((hold) => hold.status !== 'confirmed').length ?? 0} open</span>
      </footer>
      {paletteOpen && (
        <CommandPalette
          open
          commands={commands}
          initialQuery={workbench.searchQuery}
          onQueryChange={workbench.setSearchQuery}
          onClose={() => setPaletteOpen(false)}
        />
      )}
    </div>
  );
};

function resourceLabel(resource: string) {
  const normalized = resource.replace(/\\/g, '/');
  return normalized.split('/').filter(Boolean).at(-1) ?? normalized;
}

function queryErrorMessage(error: unknown): string | undefined {
  if (!error) return undefined;
  if (error instanceof Error) return error.message;
  return 'The workspace projection request failed.';
}

function normalizeHoldProjection(hold: HoldProjectionItem): HumanHold {
  const status: HumanHold['status'] = hold.status === 'human_confirmed'
    ? 'confirmed'
    : hold.status === 'human_action_required'
      ? 'pending'
      : hold.status;
  return {
    hold_id: hold.hold_id,
    label: hold.hold_id.split('_').map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join(' '),
    role: hold.required_role,
    status,
    signed_by: hold.signature?.user.name,
    signed_at: hold.signature?.signed_at,
    reason: hold.reason,
    blocker_ids: hold.blocker_ids,
  };
}

interface EditorGroupProps {
  group: 'primary' | 'secondary';
  tabs: WorkbenchTab[];
  activeTab?: WorkbenchTab;
  startControl?: React.ReactNode;
  endControl?: React.ReactNode;
  children: React.ReactNode;
  onActivate: (tabId: string, group?: 'primary' | 'secondary') => void;
  onClose: (tabId: string) => void;
  onMove: (tabId: string, group: 'primary' | 'secondary') => void;
  onDrop: (tabId: string, group: 'primary' | 'secondary') => void;
}

const EditorGroup: React.FC<EditorGroupProps> = ({ group, tabs, activeTab, startControl, endControl, children, onActivate, onClose, onMove, onDrop }) => {
  const otherGroup = group === 'primary' ? 'secondary' : 'primary';
  const focusTab = (targetIndex: number, tabList: HTMLElement | null) => {
    const next = tabs[targetIndex];
    if (!next) return;
    onActivate(next.id, group);
    window.setTimeout(() => {
      const tabButtons = Array.from(tabList?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? []);
      tabButtons[targetIndex]?.focus();
    }, 0);
  };
  const focusAdjacent = (currentId: string, direction: -1 | 1, tabList: HTMLElement | null) => {
    const index = tabs.findIndex((tab) => tab.id === currentId);
    focusTab((index + direction + tabs.length) % tabs.length, tabList);
  };
  return (
    <section className={`wb-editor-group is-${group}`} aria-label={`${group} editor group`} onDragOver={(event) => event.preventDefault()} onDrop={(event) => {
      const tabId = event.dataTransfer.getData('application/x-rustplc-workbench-tab');
      if (tabId) onDrop(tabId, group);
    }}>
      <div className="wb-editor-tabs">
        {startControl}
        <div className="wb-editor-tab-strip" role="tablist" aria-label={`${group} editor tabs`}>
          {tabs.map((tab) => (
            <div key={tab.id} className={`wb-editor-tab${tab.id === activeTab?.id ? ' is-active' : ''}`} draggable={!tab.pinned} onDragStart={(event) => {
              event.dataTransfer.effectAllowed = 'move';
              event.dataTransfer.setData('application/x-rustplc-workbench-tab', tab.id);
            }}>
              <button type="button" role="tab" data-tab-id={tab.id} aria-selected={tab.id === activeTab?.id} tabIndex={tab.id === activeTab?.id ? 0 : -1} title={tab.resource_id ?? tab.label} onClick={() => onActivate(tab.id, group)} onKeyDown={(event) => {
                const tabList = event.currentTarget.closest<HTMLElement>('[role="tablist"]');
                if (event.key === 'ArrowLeft') { event.preventDefault(); focusAdjacent(tab.id, -1, tabList); }
                if (event.key === 'ArrowRight') { event.preventDefault(); focusAdjacent(tab.id, 1, tabList); }
                if (event.key === 'Home') { event.preventDefault(); focusTab(0, tabList); }
                if (event.key === 'End') { event.preventDefault(); focusTab(tabs.length - 1, tabList); }
                if (event.key === 'Delete' && !tab.pinned) { event.preventDefault(); onClose(tab.id); }
              }}><span>{tab.label}</span></button>
              {!tab.pinned && <button className="wb-tab-action" type="button" aria-label={`Move ${tab.label} to ${otherGroup} editor group`} title={`Move to ${otherGroup} group`} onClick={() => onMove(tab.id, otherGroup)}>{group === 'primary' ? <RightOutlined /> : <LeftOutlined />}</button>}
              {!tab.pinned && <button className="wb-tab-action" type="button" aria-label={`Close ${tab.label}`} title={`Close ${tab.label}`} onClick={() => onClose(tab.id)}><CloseOutlined /></button>}
            </div>
          ))}
        </div>
        {endControl}
      </div>
      <div className="wb-editor-content">{children}</div>
    </section>
  );
};

interface WorkbenchSeparatorProps {
  orientation: 'vertical' | 'horizontal';
  label: string;
  className?: string;
  valueNow: number;
  valueMin: number;
  valueMax: number;
  onDelta: (x: number, y: number) => void;
  onKeyStep: (step: -1 | 1) => void;
}

const WorkbenchSeparator: React.FC<WorkbenchSeparatorProps> = ({ orientation, label, className = '', valueNow, valueMin, valueMax, onDelta, onKeyStep }) => (
  <div
    className={`wb-resizer ${className}`}
    role="separator"
    tabIndex={0}
    aria-label={label}
    aria-orientation={orientation}
    aria-valuemin={valueMin}
    aria-valuemax={valueMax}
    aria-valuenow={valueNow}
    onPointerDown={(event) => {
      event.preventDefault();
      const startX = event.clientX;
      const startY = event.clientY;
      const handleMove = (moveEvent: PointerEvent) => onDelta(moveEvent.clientX - startX, moveEvent.clientY - startY);
      const handleUp = () => {
        window.removeEventListener('pointermove', handleMove);
        window.removeEventListener('pointerup', handleUp);
        document.body.classList.remove('wb-is-resizing');
      };
      document.body.classList.add('wb-is-resizing');
      window.addEventListener('pointermove', handleMove);
      window.addEventListener('pointerup', handleUp, { once: true });
    }}
    onKeyDown={(event) => {
      const decrease = orientation === 'vertical' ? event.key === 'ArrowLeft' : event.key === 'ArrowDown';
      const increase = orientation === 'vertical' ? event.key === 'ArrowRight' : event.key === 'ArrowUp';
      if (decrease || increase) {
        event.preventDefault();
        onKeyStep(increase ? 1 : -1);
      }
    }}
  />
);

interface EditorContentProps {
  tab?: WorkbenchTab;
  project: ReturnType<typeof deliveryProjectApi.getProject> extends Promise<infer T> ? T | undefined : never;
  run: ReturnType<typeof deliveryProjectApi.getRun> extends Promise<infer T> ? T | undefined : never;
  runs: Awaited<ReturnType<typeof deliveryProjectApi.listRuns>>;
  wiring: PointCheckProjectionPoint[];
  wiringDiagnostics: WiringDiagnostic[];
  pointCheckSummary?: Awaited<ReturnType<typeof deliveryProjectApi.getPhysicalEvidence>>['point_checks']['summary'];
  recordingPointId?: string;
  pointObservationError?: string;
  verification: Awaited<ReturnType<typeof deliveryProjectApi.getVerification>>;
  evidence: Awaited<ReturnType<typeof deliveryProjectApi.getEvidence>>;
  geometry?: Awaited<ReturnType<typeof deliveryProjectApi.getGeometry>>;
  geometryLoading: boolean;
  evidenceFilter: 'all' | EvidenceState;
  signatureContext?: HoldSignatureContext;
  signingHoldId?: string;
  onSign: (holdId: string, request: SignHoldRequest) => Promise<void>;
  onRecordPoint: (pointId: string, request: RecordPointObservationRequest, photo?: File) => Promise<void>;
  onEvidenceFilterChange: (filter: 'all' | EvidenceState) => void;
  loading: boolean;
  error: boolean;
  onRetry: () => void;
}

const EditorContent: React.FC<EditorContentProps> = ({ tab, project, run, runs, wiring, wiringDiagnostics, pointCheckSummary, recordingPointId, pointObservationError, verification, evidence, geometry, geometryLoading, evidenceFilter, signatureContext, signingHoldId, onSign, onRecordPoint, onEvidenceFilterChange, loading, error, onRetry }) => {
  const content = useMemo(() => {
    if (loading) return <div className="wb-editor-skeleton"><span /><span /><span /><span /></div>;
    if (error) return <WorkbenchState kind="error" title="Project evidence unavailable" detail="The selected delivery project could not be loaded from the API." onRetry={onRetry} />;
    if (!project) return <WorkbenchState kind="empty" title="No project open" detail="Select a delivery project in Explorer to open its source, evidence, wiring, and release state." />;
    switch (tab?.view ?? 'overview') {
      case 'overview': return <ProjectOverviewView project={project} runs={runs} verification={verification} evidence={evidence} signatureContext={signatureContext} signingHoldId={signingHoldId} onSign={onSign} />;
      case 'agent-run': return <AgentRunTimelineView run={run} />;
      case 'wiring': return <WiringPointChecksView points={wiring} diagnostics={wiringDiagnostics} summary={pointCheckSummary} recordingPointId={recordingPointId} recordError={pointObservationError} onRecord={onRecordPoint} />;
      case 'verification': return <VerificationEvidenceView stages={verification} evidence={evidence} filter={evidenceFilter} onFilterChange={onEvidenceFilterChange} />;
      case 'source': return <ArtifactSourceView resourceId={tab?.resource_id} />;
      case 'topology': return <div className="wb-geometry-surface"><GeometryPreview artifact={geometry} loading={geometryLoading} runMode="delivery_project" /></div>;
      case 'run': return <div className="wb-legacy-view"><RunPage /></div>;
      case 'replay': return <div className="wb-legacy-view"><ReplayPage /></div>;
      case 'audit': return <div className="wb-legacy-view"><AuditPage /></div>;
      default: return null;
    }
  }, [loading, error, project, tab, runs, verification, evidence, geometry, geometryLoading, evidenceFilter, signatureContext, signingHoldId, onSign, onRecordPoint, onEvidenceFilterChange, run, wiring, wiringDiagnostics, pointCheckSummary, recordingPointId, pointObservationError, onRetry]);
  return content;
};

export default WorkbenchShell;
