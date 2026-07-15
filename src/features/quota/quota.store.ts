import { create } from 'zustand';
import type { QuotaConnectionStatus, RateLimitState } from './quota.types';

interface QuotaStore {
  snapshot: RateLimitState | null;
  status: QuotaConnectionStatus;
  applySnapshot(snapshot: RateLimitState): void;
  setStatus(status: QuotaConnectionStatus): void;
}

export const useQuotaStore = create<QuotaStore>((set) => ({
  snapshot: null,
  status: 'starting',
  applySnapshot: (snapshot) =>
    set({ snapshot, status: snapshot.stale ? 'stale' : 'live' }),
  setStatus: (status) => set({ status }),
}));
