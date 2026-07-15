export interface ResetCreditState {
  availableCount: number | null;
  fetchedAt: number;
  stale: boolean;
  authRequired: boolean;
}

export function isResetCreditState(value: unknown): value is ResetCreditState {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const state = value as Record<string, unknown>;
  const count = state.availableCount;
  return (
    (count === null ||
      (typeof count === 'number' && Number.isInteger(count) && count >= 0)) &&
    typeof state.fetchedAt === 'number' &&
    Number.isFinite(state.fetchedAt) &&
    typeof state.stale === 'boolean' &&
    typeof state.authRequired === 'boolean'
  );
}
