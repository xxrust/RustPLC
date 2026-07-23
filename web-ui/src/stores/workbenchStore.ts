import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { WorkbenchTab, WorkbenchView } from '../types/workbench';

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

interface WorkbenchState {
  selectedProjectId: string | null;
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
}

export const useWorkbenchStore = create<WorkbenchState>()(
  persist(
    (set) => ({
      selectedProjectId: null,
      selectedRunId: null,
      activeActivity: 'projects',
      tabs: [defaultTab],
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
      setSelectedProject: (selectedProjectId) =>
        set({ selectedProjectId, selectedRunId: null }),
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
    }),
    {
      name: 'rustplc-workbench-layout',
      partialize: (state) => ({
        selectedProjectId: state.selectedProjectId,
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
      }),
    }
  )
);
