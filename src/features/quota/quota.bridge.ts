import { useQuotaStore } from './quota.store';
import type { QuotaConnectionStatus, RateLimitState } from './quota.types';

interface BridgeEvent {
  payload: unknown;
}

export interface QuotaBridgeApi {
  invoke(command: string, args?: Record<string, unknown>): Promise<unknown>;
  listen(event: string, handler: (event: BridgeEvent) => void): Promise<() => void>;
  open(url: string): Promise<void> | void;
}

export interface QuotaBridge {
  refresh(reason: string): Promise<void>;
  stop(): Promise<void>;
}

const connectionStatuses = new Set<QuotaConnectionStatus>([
  'starting',
  'live',
  'stale',
  'offline',
  'loginRequired',
]);

const windowKinds = new Set(['fiveHour', 'weekly', 'other']);
const rateLimitSources = new Set(['read', 'updated', 'cache']);

function hasOwn(object: object, property: string): boolean {
  return Object.prototype.hasOwnProperty.call(object, property);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isQuotaWindowForKind(payload: unknown, expectedKind?: string): boolean {
  if (typeof payload !== 'object' || payload === null) {
    return false;
  }
  const window = payload as Record<string, unknown>;
  const kind = window.kind;
  return (
    typeof kind === 'string' &&
    windowKinds.has(kind) &&
    (expectedKind === undefined || kind === expectedKind) &&
    isFiniteNumber(window.usedPercent) &&
    isFiniteNumber(window.remainingPercent) &&
    isFiniteNumber(window.windowDurationMins) &&
    (window.resetsAt === null || isFiniteNumber(window.resetsAt))
  );
}

function isRateLimitState(payload: unknown): payload is RateLimitState {
  if (typeof payload !== 'object' || payload === null) {
    return false;
  }
  const state = payload as Record<string, unknown>;
  const requiredKeys = [
    'fiveHour',
    'weekly',
    'other',
    'planType',
    'reachedType',
    'fetchedAt',
    'source',
    'stale',
  ];
  return (
    requiredKeys.every((key) => hasOwn(state, key)) &&
    (state.fiveHour === null || isQuotaWindowForKind(state.fiveHour, 'fiveHour')) &&
    (state.weekly === null || isQuotaWindowForKind(state.weekly, 'weekly')) &&
    Array.isArray(state.other) &&
    state.other.every((window) => isQuotaWindowForKind(window, 'other')) &&
    (state.planType === null || typeof state.planType === 'string') &&
    (state.reachedType === null || typeof state.reachedType === 'string') &&
    isFiniteNumber(state.fetchedAt) &&
    typeof state.source === 'string' &&
    rateLimitSources.has(state.source) &&
    typeof state.stale === 'boolean'
  );
}

function statusFrom(payload: unknown): QuotaConnectionStatus | null {
  if (typeof payload !== 'object' || payload === null || !('status' in payload)) {
    return null;
  }
  const status = payload.status;
  return typeof status === 'string' && connectionStatuses.has(status as QuotaConnectionStatus)
    ? (status as QuotaConnectionStatus)
    : null;
}

interface QuotaBootstrapState {
  snapshot: RateLimitState | null;
  status: QuotaConnectionStatus;
}

function bootstrapStateFrom(payload: unknown): QuotaBootstrapState | null {
  if (
    typeof payload !== 'object' ||
    payload === null ||
    !hasOwn(payload, 'snapshot') ||
    !hasOwn(payload, 'status')
  ) {
    return null;
  }
  const state = payload as Record<string, unknown>;
  const status = statusFrom(state);
  if (
    status === null ||
    (state.snapshot !== null && !isRateLimitState(state.snapshot))
  ) {
    return null;
  }
  return { snapshot: state.snapshot, status };
}

function loginUrlFrom(payload: unknown): string | null {
  if (
    typeof payload !== 'object' ||
    payload === null ||
    !('loginId' in payload) ||
    typeof payload.loginId !== 'string' ||
    !('authUrl' in payload) ||
    typeof payload.authUrl !== 'string'
  ) {
    return null;
  }
  try {
    const url = new URL(payload.authUrl);
    return url.protocol === 'https:' ? url.toString() : null;
  } catch {
    return null;
  }
}

async function callAllUnlisteners(unlisteners: Array<() => void>): Promise<void> {
  const cleanupResults = await Promise.allSettled(
    unlisteners.map(async (unlisten) => {
      unlisten();
    }),
  );
  const cleanupFailure = cleanupResults.find(
    (result): result is PromiseRejectedResult => result.status === 'rejected',
  );
  if (cleanupFailure !== undefined) {
    throw cleanupFailure.reason;
  }
}

export async function startQuotaBridge(api: QuotaBridgeApi): Promise<QuotaBridge> {
  const unlisteners: Array<() => void> = [];
  let snapshotEventSeen = false;
  let statusEventSeen = false;
  try {
    unlisteners.push(await api.listen('rate-limits://updated', ({ payload }) => {
      if (isRateLimitState(payload)) {
        snapshotEventSeen = true;
        useQuotaStore.getState().applySnapshot(payload);
      }
    }));
    unlisteners.push(await api.listen('rate-limits://status', ({ payload }) => {
      const status = statusFrom(payload);
      if (status !== null) {
        statusEventSeen = true;
        useQuotaStore.getState().setStatus(status);
      }
    }));
    unlisteners.push(await api.listen('account://login-url', ({ payload }) => {
      const url = loginUrlFrom(payload);
      if (url !== null) {
        try {
          void Promise.resolve(api.open(url)).catch(() => undefined);
        } catch {
          // Ignore opener failures; login can be retried by the next backend event.
        }
      }
    }));

    const initial = bootstrapStateFrom(
      await api.invoke('get_quota_bridge_state'),
    );
    if (initial !== null) {
      const eventStatus = useQuotaStore.getState().status;
      if (!snapshotEventSeen && initial.snapshot !== null) {
        useQuotaStore.getState().applySnapshot(initial.snapshot);
      }
      if (statusEventSeen) {
        useQuotaStore.getState().setStatus(eventStatus);
      } else if (!snapshotEventSeen) {
        useQuotaStore.getState().setStatus(initial.status);
      }
    }
  } catch (error) {
    try {
      await callAllUnlisteners(unlisteners);
    } catch {
      // Preserve the listener registration failure while still attempting all rollback cleanup.
    }
    throw error;
  }

  let stopped = false;

  return {
    async refresh(reason) {
      await api.invoke('refresh_rate_limits', { reason });
    },
    async stop() {
      if (stopped) {
        return;
      }
      stopped = true;
      await callAllUnlisteners(unlisteners);
    },
  };
}
