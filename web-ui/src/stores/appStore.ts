import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { RunMode, UserRole } from '../types';

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
  setCurrentProject: (project: string | null) => void;

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

      currentProject: null,
      setCurrentProject: (project) => set({ currentProject: project }),

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
      partialize: (state) => ({
        runMode: state.runMode,
        currentUser: state.currentUser,
        currentProject: state.currentProject,
      }),
    }
  )
);
