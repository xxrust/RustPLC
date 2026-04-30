import { createContext, useContext } from 'react';

interface CanvasInteractionState {
  readOnly: boolean;
  liveSimulationEnabled: boolean;
}

const CanvasInteractionContext = createContext<CanvasInteractionState>({
  readOnly: false,
  liveSimulationEnabled: false,
});

export const CanvasInteractionProvider = CanvasInteractionContext.Provider;

export function useCanvasInteraction(): CanvasInteractionState {
  return useContext(CanvasInteractionContext);
}
