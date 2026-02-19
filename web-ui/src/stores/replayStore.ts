import { create } from 'zustand';

export interface TickSnapshot {
  tick: number;
  components: Record<string, { status: string; value?: number | boolean }>;
  io?: {
    di?: Record<string, boolean>;
    do?: Record<string, boolean>;
    ai?: Record<string, number>;
    ao?: Record<string, number>;
  };
  events?: Array<{ type: 'error' | 'info' | 'warn'; message: string }>;
}

interface ReplayState {
  snapshots: TickSnapshot[];
  currentTick: number;
  maxTick: number;
  isPlaying: boolean;
  playSpeed: number;

  setSnapshots: (snapshots: TickSnapshot[]) => void;
  setCurrentTick: (tick: number) => void;
  setIsPlaying: (playing: boolean) => void;
  setPlaySpeed: (speed: number) => void;
  stepForward: () => void;
  stepBackward: () => void;
}

export const useReplayStore = create<ReplayState>((set, get) => ({
  snapshots: [],
  currentTick: 0,
  maxTick: 0,
  isPlaying: false,
  playSpeed: 1,

  setSnapshots: (snapshots) =>
    set({ snapshots, maxTick: snapshots.length > 0 ? snapshots.length - 1 : 0, currentTick: 0 }),

  setCurrentTick: (tick) => set({ currentTick: Math.max(0, Math.min(tick, get().maxTick)) }),

  setIsPlaying: (playing) => set({ isPlaying: playing }),

  setPlaySpeed: (speed) => set({ playSpeed: speed }),

  stepForward: () => {
    const { currentTick, maxTick } = get();
    if (currentTick < maxTick) set({ currentTick: currentTick + 1 });
    else set({ isPlaying: false });
  },

  stepBackward: () => {
    const { currentTick } = get();
    if (currentTick > 0) set({ currentTick: currentTick - 1 });
  },
}));
