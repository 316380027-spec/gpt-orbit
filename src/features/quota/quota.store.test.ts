import { beforeEach, describe, expect, it } from 'vitest';
import { useQuotaStore } from './quota.store';
import type { RateLimitState } from './quota.types';

const snapshot: RateLimitState = {
  fiveHour: {
    kind: 'fiveHour',
    usedPercent: 27,
    remainingPercent: 73,
    windowDurationMins: 300,
    resetsAt: 1_800_000_000,
  },
  weekly: {
    kind: 'weekly',
    usedPercent: 42,
    remainingPercent: 58,
    windowDurationMins: 10_080,
    resetsAt: 1_800_500_000,
  },
  other: [],
  planType: 'plus',
  reachedType: null,
  fetchedAt: 1_799_999_000,
  source: 'read',
  stale: false,
};

describe('quota store', () => {
  beforeEach(() => {
    useQuotaStore.setState({ snapshot: null, status: 'starting' });
  });

  it('retains the complete backend-independent snapshot contract', () => {
    useQuotaStore.getState().applySnapshot(snapshot);

    expect(useQuotaStore.getState().snapshot).toEqual(snapshot);
    expect(useQuotaStore.getState().status).toBe('live');
  });

  it('maps a cached snapshot to stale display status', () => {
    const cached = { ...snapshot, source: 'cache' as const, stale: true };

    useQuotaStore.getState().applySnapshot(cached);

    expect(useQuotaStore.getState().snapshot).toEqual(cached);
    expect(useQuotaStore.getState().status).toBe('stale');
  });
});
