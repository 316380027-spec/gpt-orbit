import { create } from 'zustand';
import type { ResetCreditState } from './reset-credits.types';

interface ResetCreditStore {
  snapshot: ResetCreditState | null;
  applySnapshot(snapshot: ResetCreditState): void;
  clear(): void;
}

export const useResetCreditStore = create<ResetCreditStore>((set) => ({
  snapshot: null,
  applySnapshot: (snapshot) => set({ snapshot }),
  clear: () => set({ snapshot: null }),
}));
