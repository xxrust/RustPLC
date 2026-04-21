import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { RunMode, UserRole } from '../types';

export const DEFAULT_PROJECT_ID = 'demo';
const LEGACY_DEFAULT_PROJECT_ID = 'component_model';

interface AppState {
  // 运行模式
  runMode: RunMode;
  setRunMode: (mode: RunMode) => void;

  // 用户信息
  currentUser: {
    id: string;
    name: string;
    role: UserRole;
  } | null;
  setCurrentUser: (user: AppState['currentUser']) => void;

  // 当前项目
  currentProject: string | null;
  currentProjectPath: string | null;
  currentProjectContent: string | null;
  setCurrentProject: (project: string | null, path?: string | null, content?: string | null) => void;

  // 未保存状态
  hasUnsavedChanges: boolean;
  setHasUnsavedChanges: (hasChanges: boolean) => void;

  // 告警计数
  alarmCount: {
    info: number;
    warning: number;
    critical: number;
  };
  setAlarmCount: (count: AppState['alarmCount']) => void;
}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      runMode: 'no_board',
      setRunMode: (mode) => set({ runMode: mode }),

      currentUser: {
        id: 'dev-user',
        name: 'Developer',
        role: 'engineer',
      },
      setCurrentUser: (user) => set({ currentUser: user }),

      currentProject: DEFAULT_PROJECT_ID,
      currentProjectPath: null,
      currentProjectContent: null,
      setCurrentProject: (project, path = null, content = null) =>
        set({ currentProject: project, currentProjectPath: path, currentProjectContent: content }),

      hasUnsavedChanges: false,
      setHasUnsavedChanges: (hasChanges) => set({ hasUnsavedChanges: hasChanges }),

      alarmCount: {
        info: 0,
        warning: 0,
        critical: 0,
      },
      setAlarmCount: (count) => set({ alarmCount: count }),
    }),
    {
      name: 'rustplc-app-storage',
      merge: (persistedState, currentState) => {
        const merged = {
          ...currentState,
          ...(persistedState as Partial<AppState> | undefined),
        };

        return {
          ...merged,
          currentProject:
            !merged.currentProject || merged.currentProject === LEGACY_DEFAULT_PROJECT_ID
              ? DEFAULT_PROJECT_ID
              : merged.currentProject,
        };
      },
      partialize: (state) => ({
        runMode: state.runMode,
        currentUser: state.currentUser,
        currentProject: state.currentProject,
      }),
    }
  )
);
