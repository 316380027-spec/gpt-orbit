import { beforeEach, describe, expect, it } from 'vitest';
import { useResetCreditStore } from './reset-credits.store';
import { isResetCreditState, type ResetCreditState } from './reset-credits.types';

const valid: ResetCreditState = {
  availableCount: 3,
  fetchedAt: 1_800_000_000,
  stale: false,
  authRequired: false,
};

describe('reset credit state', () => {
  beforeEach(() => {
    useResetCreditStore.setState({ snapshot: null });
  });

  it('accepts a non-negative integer count and null availability', () => {
    expect(isResetCreditState(valid)).toBe(true);
    expect(isResetCreditState({ ...valid, availableCount: null })).toBe(true);
  });

  it.each([-1, 1.5, '3'])('rejects an invalid available count: %s', (availableCount) => {
    expect(isResetCreditState({ ...valid, availableCount })).toBe(false);
  });

  it('rejects malformed timestamps and stale flags', () => {
    expect(isResetCreditState({ ...valid, fetchedAt: Number.POSITIVE_INFINITY })).toBe(false);
    expect(isResetCreditState({ ...valid, stale: 'false' })).toBe(false);
  });

  it('requires a boolean authentication-required flag', () => {
    const { authRequired: _, ...missing } = valid;
    expect(isResetCreditState(missing)).toBe(false);
    expect(isResetCreditState({ ...valid, authRequired: 'true' })).toBe(false);
    expect(isResetCreditState({ ...valid, authRequired: true })).toBe(true);
  });

  it('stores a complete snapshot and clears it', () => {
    expect(useResetCreditStore.getState().snapshot).toBeNull();

    useResetCreditStore.getState().applySnapshot(valid);
    expect(useResetCreditStore.getState().snapshot).toEqual(valid);

    useResetCreditStore.getState().clear();
    expect(useResetCreditStore.getState().snapshot).toBeNull();
  });
});
