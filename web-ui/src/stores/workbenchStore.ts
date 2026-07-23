import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { EvidenceState, WorkbenchTab, WorkbenchView } from '../types/workbench';

export type ActivityId =
  | 'projects'
  | 'runs'
  | 'wiring'
  | 'verification'
  | 'evidence'
  | 'search'
  | 'source-control';

export type BottomPanelId = 'problems' | 'tests' | 'verification' | 'terminal' | 'audit';

const defaultTab: WorkbenchTab = {
  id: 'overview',
  label: 'Project Overview',
  view: 'overview',
  pinned: true,
};

interface ProjectWorkbenchSession {
  selectedRunId: string | null;
  activeActivity: ActivityId;
  tabs: WorkbenchTab[];
  activeTabId: string;
  secondaryActiveTabId: string | null;
  activeGroup: 'primary' | 'secondary';
  splitEnabled: boolean;
  primarySidebarWidth: number;
  inspectorWidth: number;
  bottomPanelHeight: number;
  splitRatio: number;
  primarySidebarCollapsed: boolean;
  inspectorCollapsed: boolean;
  bottomPanelCollapsed: boolean;
  bottomPanel: BottomPanelId;
  evidenceFilter: 'all' | EvidenceState;
}

const createDefaultSession = (): ProjectWorkbenchSession => ({
  selectedRunId: null,
  activeActivity: 'projects',
  tabs: [{ ...defaultTab }],
  activeTabId: defaultTab.id,
  secondaryActiveTabId: null,
  activeGroup: 'primary',
  splitEnabled: false,
  primarySidebarWidth: 260,
  inspectorWidth: 300,
  bottomPanelHeight: 218,
  splitRatio: 0.5,
  primarySidebarCollapsed: false,
  inspectorCollapsed: false,
  bottomPanelCollapsed: false,
  bottomPanel: 'problems',
  evidenceFilter: 'all',
});

function captureSession(state: ProjectWorkbenchSession): ProjectWorkbenchSession {
  return {
    selectedRunId: state.selectedRunId,
    activeActivity: state.activeActivity,
    tabs: state.tabs.map((tab) => ({ ...tab })),
    activeTabId: state.activeTabId,
    secondaryActiveTabId: state.secondaryActiveTabId,
    activeGroup: state.activeGroup,
    splitEnabled: state.splitEnabled,
    primarySidebarWidth: state.primarySidebarWidth,
    inspectorWidth: state.inspectorWidth,
    bottomPanelHeight: state.bottomPanelHeight,
    splitRatio: state.splitRatio,
    primarySidebarCollapsed: state.primarySidebarCollapsed,
    inspectorCollapsed: state.inspectorCollapsed,
    bottomPanelCollapsed: state.bottomPanelCollapsed,
    bottomPanel: state.bottomPanel,
    evidenceFilter: state.evidenceFilter,
  };
}

function normalizeSession(value: unknown): ProjectWorkbenchSession {
  const fallback = createDefaultSession();
  if (!value || typeof value !== 'object') return fallback;
  const candidate = value as Partial<ProjectWorkbenchSession>;
  const tabs = Array.isArray(candidate.tabs) && candidate.tabs.length > 0
    ? candidate.tabs.map((tab) => ({ ...tab }))
    : fallback.tabs;
  const primaryTabs = tabs.filter((tab) => (tab.group ?? 'primary') === 'primary');
  const secondaryTabs = tabs.filter((tab) => tab.group === 'secondary');
  return {
    ...fallback,
    ...candidate,
    tabs,
    activeTabId: tabs.some((tab) => tab.id === candidate.activeTabId)
      ? candidate.activeTabId!
      : primaryTabs[0]?.id ?? tabs[0].id,
    secondaryActiveTabId: secondaryTabs.some((tab) => tab.id === candidate.secondaryActiveTabId)
      ? candidate.secondaryActiveTabId!
      : secondaryTabs[0]?.id ?? null,
    splitEnabled: secondaryTabs.length > 0 && Boolean(candidate.splitEnabled),
  };
}

interface WorkbenchState {
  selectedProjectId: string | null;
  projectSessions: Record<string, ProjectWorkbenchSession>;
  selectedRunId: string | null;
  activeActivity: ActivityId;
  tabs: WorkbenchTab[];
  activeTabId: string;
  secondaryActiveTabId: string | null;
  activeGroup: 'primary' | 'secondary';
  splitEnabled: boolean;
  primarySidebarWidth: number;
  inspectorWidth: number;
  bottomPanelHeight: number;
  splitRatio: number;
  primarySidebarCollapsed: boolean;
  inspectorCollapsed: boolean;
  bottomPanelCollapsed: boolean;
  bottomPanel: BottomPanelId;
  searchQuery: string;
  evidenceFilter: 'all' | EvidenceState;
  setSelectedProject: (projectId: string | null) => void;
  setSelectedRun: (runId: string | null) => void;
  setActiveActivity: (activity: ActivityId) => void;
  openView: (view: WorkbenchView, label: string, resourceId?: string) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string, group?: 'primary' | 'secondary') => void;
  splitActiveTab: () => void;
  moveTabToGroup: (tabId: string, group: 'primary' | 'secondary') => void;
  closeSplit: () => void;
  setPrimarySidebarWidth: (width: number) => void;
  setInspectorWidth: (width: number) => void;
  setBottomPanelHeight: (height: number) => void;
  setSplitRatio: (ratio: number) => void;
  togglePrimarySidebar: () => void;
  toggleInspector: () => void;
  toggleBottomPanel: () => void;
  setBottomPanel: (panel: BottomPanelId) => void;
  setSearchQuery: (query: string) => void;
  setEvidenceFilter: (filter: 'all' | EvidenceState) => void;
}

export const useWorkbenchStore = create<WorkbenchState>()(
  persist(
    (set) => {
      const defaultSession = createDefaultSession();
      return {
      selectedProjectId: null,
      projectSessions: {},
      ...defaultSession,
      searchQuery: '',
      setSelectedProject: (selectedProjectId) =>
        set((state) => {
          if (state.selectedProjectId === selectedProjectId) return {};
          const projectSessions = { ...state.projectSessions };
          if (state.selectedProjectId) {
            projectSessions[state.selectedProjectId] = captureSession(state);
          }
          const nextSession = selectedProjectId
            ? normalizeSession(projectSessions[selectedProjectId])
            : createDefaultSession();
          return { selectedProjectId, projectSessions, ...nextSession };
        }),
      setSelectedRun: (selectedRunId) => set({ selectedRunId }),
      setActiveActivity: (activeActivity) =>
        set({ activeActivity, primarySidebarCollapsed: false }),
      openView: (view, label, resourceId) =>
        set((state) => {
          const id = resourceId ? `${view}:${resourceId}` : view;
          const existing = state.tabs.find((tab) => tab.id === id);
          if (existing) {
            const group = existing.group ?? 'primary';
            return group === 'secondary'
              ? { secondaryActiveTabId: existing.id, activeGroup: 'secondary' }
              : { activeTabId: existing.id, activeGroup: 'primary' };
          }
          return {
            tabs: [...state.tabs, { id, label, view, resource_id: resourceId, group: state.activeGroup }],
            activeTabId: state.activeGroup === 'primary' ? id : state.activeTabId,
            secondaryActiveTabId: state.activeGroup === 'secondary' ? id : state.secondaryActiveTabId,
            splitEnabled: state.activeGroup === 'secondary' ? true : state.splitEnabled,
          };
        }),
      closeTab: (tabId) =>
        set((state) => {
          const tabs = state.tabs.filter((tab) => tab.id !== tabId);
          const nextTabs = tabs.length > 0 ? tabs : [defaultTab];
          const nextSecondary = nextTabs.filter((tab) => tab.group === 'secondary');
          return {
            tabs: nextTabs,
            activeTabId:
              state.activeTabId === tabId
                ? (nextTabs.find((tab) => (tab.group ?? 'primary') === 'primary') ?? nextTabs[nextTabs.length - 1]).id
                : state.activeTabId,
            secondaryActiveTabId:
              state.secondaryActiveTabId === tabId
                ? nextSecondary[0]?.id ?? null
                : state.secondaryActiveTabId,
            splitEnabled: nextSecondary.length > 0,
            activeGroup: state.activeGroup === 'secondary' && nextSecondary.length === 0 ? 'primary' : state.activeGroup,
          };
        }),
      setActiveTab: (tabId, group = 'primary') =>
        set(group === 'secondary'
          ? { secondaryActiveTabId: tabId, activeGroup: 'secondary' }
          : { activeTabId: tabId, activeGroup: 'primary' }),
      splitActiveTab: () =>
        set((state) => {
          const sourceId = state.activeGroup === 'secondary'
            ? state.secondaryActiveTabId
            : state.activeTabId;
          const source = state.tabs.find((tab) => tab.id === sourceId);
          if (!source) return {};
          if (source.group === 'secondary') return { splitEnabled: true };
          let cloneId = `${source.id}:split`;
          let suffix = 2;
          while (state.tabs.some((tab) => tab.id === cloneId)) {
            cloneId = `${source.id}:split-${suffix}`;
            suffix += 1;
          }
          const clone = { ...source, id: cloneId, pinned: false, group: 'secondary' as const };
          return {
            tabs: [...state.tabs, clone],
            secondaryActiveTabId: clone.id,
            activeGroup: 'secondary',
            splitEnabled: true,
          };
        }),
      moveTabToGroup: (tabId, group) =>
        set((state) => {
          const target = state.tabs.find((tab) => tab.id === tabId);
          if (!target || target.pinned) return {};
          const tabs = state.tabs.map((tab) => tab.id === tabId ? { ...tab, group } : tab);
          const secondaryTabs = tabs.filter((tab) => tab.group === 'secondary');
          const primaryTabs = tabs.filter((tab) => (tab.group ?? 'primary') === 'primary');
          return {
            tabs,
            splitEnabled: secondaryTabs.length > 0,
            secondaryActiveTabId: group === 'secondary'
              ? tabId
              : state.secondaryActiveTabId === tabId
                ? secondaryTabs[0]?.id ?? null
                : state.secondaryActiveTabId,
            activeTabId: group === 'primary'
              ? tabId
              : state.activeTabId === tabId
                ? primaryTabs[0]?.id ?? defaultTab.id
                : state.activeTabId,
            activeGroup: group,
          };
        }),
      closeSplit: () =>
        set((state) => ({
          tabs: state.tabs.map((tab) => ({ ...tab, group: 'primary' as const })),
          splitEnabled: false,
          secondaryActiveTabId: null,
          activeGroup: 'primary',
        })),
      setPrimarySidebarWidth: (primarySidebarWidth) => set({ primarySidebarWidth: Math.min(420, Math.max(190, primarySidebarWidth)) }),
      setInspectorWidth: (inspectorWidth) => set({ inspectorWidth: Math.min(460, Math.max(240, inspectorWidth)) }),
      setBottomPanelHeight: (bottomPanelHeight) => set({ bottomPanelHeight: Math.min(460, Math.max(120, bottomPanelHeight)) }),
      setSplitRatio: (splitRatio) => set({ splitRatio: Math.min(0.72, Math.max(0.28, splitRatio)) }),
      togglePrimarySidebar: () =>
        set((state) => ({ primarySidebarCollapsed: !state.primarySidebarCollapsed })),
      toggleInspector: () =>
        set((state) => ({ inspectorCollapsed: !state.inspectorCollapsed })),
      toggleBottomPanel: () =>
        set((state) => ({ bottomPanelCollapsed: !state.bottomPanelCollapsed })),
      setBottomPanel: (bottomPanel) => set({ bottomPanel, bottomPanelCollapsed: false }),
      setSearchQuery: (searchQuery) => set({ searchQuery }),
      setEvidenceFilter: (evidenceFilter) => set({ evidenceFilter }),
    };
    },
    {
      name: 'rustplc-workbench-layout',
      version: 2,
      migrate: (persistedState) => {
        if (!persistedState || typeof persistedState !== 'object') return persistedState as WorkbenchState;
        const previous = persistedState as Partial<WorkbenchState>;
        const projectSessions = { ...(previous.projectSessions ?? {}) };
        if (previous.selectedProjectId) {
          projectSessions[previous.selectedProjectId] = normalizeSession(previous);
        }
        return { ...previous, projectSessions } as WorkbenchState;
      },
      partialize: (state) => ({
        selectedProjectId: state.selectedProjectId,
        projectSessions: state.selectedProjectId
          ? { ...state.projectSessions, [state.selectedProjectId]: captureSession(state) }
          : state.projectSessions,
        selectedRunId: state.selectedRunId,
        activeActivity: state.activeActivity,
        tabs: state.tabs,
        activeTabId: state.activeTabId,
        secondaryActiveTabId: state.secondaryActiveTabId,
        activeGroup: state.activeGroup,
        splitEnabled: state.splitEnabled,
        primarySidebarWidth: state.primarySidebarWidth,
        inspectorWidth: state.inspectorWidth,
        bottomPanelHeight: state.bottomPanelHeight,
        splitRatio: state.splitRatio,
        primarySidebarCollapsed: state.primarySidebarCollapsed,
        inspectorCollapsed: state.inspectorCollapsed,
        bottomPanelCollapsed: state.bottomPanelCollapsed,
        bottomPanel: state.bottomPanel,
        searchQuery: state.searchQuery,
        evidenceFilter: state.evidenceFilter,
      }),
    }
  )
);
