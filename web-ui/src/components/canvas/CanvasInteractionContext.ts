import { createContext, useContext } from 'react';

interface CanvasInteractionState {
  readOnly: boolean;
  liveSimulationEnabled: boolean;
}

export const CanvasInteractionContext = createContext<CanvasInteractionState>({
  readOnly: false,
  liveSimulationEnabled: false,
});

export function useCanvasInteraction(): CanvasInteractionState {
  return useContext(CanvasInteractionContext);
}
