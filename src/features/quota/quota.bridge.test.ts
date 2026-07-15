import { beforeEach, describe, expect, it, vi } from 'vitest';
import { startQuotaBridge, type QuotaBridgeApi } from './quota.bridge';
import { useQuotaStore } from './quota.store';
import type { RateLimitState } from './quota.types';

const initialSnapshot: RateLimitState = {
  fiveHour: {
    kind: 'fiveHour',
    usedPercent: 27,
    remainingPercent: 73,
    windowDurationMins: 300,
    resetsAt: 1_800_000_000,
  },
  weekly: null,
  other: [],
  planType: 'plus',
  reachedType: null,
  fetchedAt: 1_799_999_000,
  source: 'read',
  stale: false,
};

type EventHandler = (event: { payload: unknown }) => void;

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function createApi(
  snapshot: RateLimitState | null = initialSnapshot,
  status = 'live',
) {
  const handlers = new Map<string, EventHandler>();
  const unlisteners = [vi.fn(), vi.fn(), vi.fn()];
  let listenerIndex = 0;
  const api: QuotaBridgeApi = {
    invoke: vi.fn(async (command: string) =>
      command === 'get_quota_bridge_state' ? { snapshot, status } : undefined,
    ),
    listen: vi.fn(async (event: string, handler: EventHandler) => {
      handlers.set(event, handler);
      return unlisteners[listenerIndex++];
    }),
    open: vi.fn(async () => undefined),
  };
  return { api, handlers, unlisteners };
}

describe('quota bridge', () => {
  beforeEach(() => {
    useQuotaStore.setState({ snapshot: null, status: 'starting' });
  });

  it('registers all whitelisted listeners before reading retained state and status', async () => {
    const { promise, resolve } = createDeferred<unknown>();
    const { api } = createApi(null);
    vi.mocked(api.invoke).mockImplementation(async (command: string) =>
      command === 'get_quota_bridge_state' ? promise : undefined,
    );

    const bridgePromise = startQuotaBridge(api);

    await vi.waitFor(() => {
      expect(api.listen).toHaveBeenCalledTimes(3);
      expect(api.invoke).toHaveBeenNthCalledWith(1, 'get_quota_bridge_state');
    });

    resolve({ snapshot: initialSnapshot, status: 'live' });
    const bridge = await bridgePromise;

    expect(api.listen).toHaveBeenCalledWith(
      'rate-limits://updated',
      expect.any(Function),
    );
    expect(api.listen).toHaveBeenCalledWith(
      'rate-limits://status',
      expect.any(Function),
    );
    expect(api.listen).toHaveBeenCalledWith(
      'account://login-url',
      expect.any(Function),
    );
    for (const listenOrder of vi.mocked(api.listen).mock.invocationCallOrder) {
      expect(listenOrder).toBeLessThan(
        vi.mocked(api.invoke).mock.invocationCallOrder[0],
      );
    }
    expect(useQuotaStore.getState().snapshot).toEqual(initialSnapshot);
    expect(useQuotaStore.getState().status).toBe('live');

    await bridge.stop();
  });

  it('does not let an old bootstrap overwrite snapshot and status events received while it is pending', async () => {
    const { promise, resolve } = createDeferred<unknown>();
    const { api, handlers } = createApi(null);
    vi.mocked(api.invoke).mockImplementation(async (command: string) =>
      command === 'get_quota_bridge_state' ? promise : undefined,
    );
    const updated: RateLimitState = {
      ...initialSnapshot,
      fetchedAt: initialSnapshot.fetchedAt + 1,
      source: 'updated',
    };

    const bridgePromise = startQuotaBridge(api);
    await vi.waitFor(() => {
      expect(api.invoke).toHaveBeenCalledWith('get_quota_bridge_state');
    });
    handlers.get('rate-limits://updated')?.({ payload: updated });
    handlers.get('rate-limits://status')?.({ payload: { status: 'offline' } });
    resolve({ snapshot: initialSnapshot, status: 'starting' });

    const bridge = await bridgePromise;
    expect(useQuotaStore.getState()).toMatchObject({
      snapshot: updated,
      status: 'offline',
    });

    await bridge.stop();
  });

  it('ignores malformed retained state and updated events without changing store state', async () => {
    const { api, handlers } = createApi(null);
    vi.mocked(api.invoke).mockResolvedValueOnce({
      snapshot: { stale: false },
      status: 'polluted',
    });

    const bridge = await startQuotaBridge(api);

    expect(useQuotaStore.getState()).toEqual({
      snapshot: null,
      status: 'starting',
      applySnapshot: expect.any(Function),
      setStatus: expect.any(Function),
    });

    handlers.get('rate-limits://updated')?.({ payload: null });
    handlers.get('rate-limits://updated')?.({
      payload: { stale: false, source: 'updated', status: 'polluted' },
    });

    expect(useQuotaStore.getState()).toEqual({
      snapshot: null,
      status: 'starting',
      applySnapshot: expect.any(Function),
      setStatus: expect.any(Function),
    });

    await bridge.stop();
  });

  it('cleans up listeners that were registered before a later listener setup failure', async () => {
    const firstUnlisten = vi.fn();
    const registrationFailure = new Error('status listener failed');
    const api: QuotaBridgeApi = {
      invoke: vi.fn(async () => null),
      listen: vi
        .fn()
        .mockResolvedValueOnce(firstUnlisten)
        .mockRejectedValueOnce(registrationFailure),
      open: vi.fn(async () => undefined),
    };

    await expect(startQuotaBridge(api)).rejects.toBe(registrationFailure);

    expect(api.listen).toHaveBeenCalledTimes(2);
    expect(firstUnlisten).toHaveBeenCalledTimes(1);
  });

  it('calls every retained listener cleanup even when one cleanup fails during setup rollback', async () => {
    const cleanupFailure = new Error('cleanup failed');
    const firstUnlisten = vi.fn(() => {
      throw cleanupFailure;
    });
    const secondUnlisten = vi.fn();
    const registrationFailure = new Error('login listener failed');
    const api: QuotaBridgeApi = {
      invoke: vi.fn(async () => null),
      listen: vi
        .fn()
        .mockResolvedValueOnce(firstUnlisten)
        .mockResolvedValueOnce(secondUnlisten)
        .mockRejectedValueOnce(registrationFailure),
      open: vi.fn(async () => undefined),
    };

    await expect(startQuotaBridge(api)).rejects.toBe(registrationFailure);

    expect(api.listen).toHaveBeenCalledTimes(3);
    expect(firstUnlisten).toHaveBeenCalledTimes(1);
    expect(secondUnlisten).toHaveBeenCalledTimes(1);
  });

  it('applies updated and safe status event payloads', async () => {
    const { api, handlers } = createApi(null);
    const updated: RateLimitState = {
      ...initialSnapshot,
      fiveHour: { ...initialSnapshot.fiveHour!, usedPercent: 31, remainingPercent: 69 },
      source: 'updated',
    };
    const bridge = await startQuotaBridge(api);

    handlers.get('rate-limits://updated')?.({ payload: updated });
    handlers.get('rate-limits://status')?.({
      payload: { status: 'offline', message: 'Unable to refresh quota.' },
    });

    expect(useQuotaStore.getState()).toMatchObject({
      snapshot: updated,
      status: 'offline',
    });

    await bridge.stop();
  });

  it('opens only a controlled HTTPS login URL payload', async () => {
    const { api, handlers } = createApi(null);
    const bridge = await startQuotaBridge(api);
    const login = handlers.get('account://login-url');

    login?.({ payload: { loginId: 'login-1', authUrl: 'https://auth.openai.com/start' } });
    login?.({ payload: { loginId: 'login-2', authUrl: 'file:///secrets' } });
    login?.({ payload: new Error('raw login failure') });

    expect(api.open).toHaveBeenCalledTimes(1);
    expect(api.open).toHaveBeenCalledWith('https://auth.openai.com/start');

    await bridge.stop();
  });

  it('forwards refresh reasons and removes every listener during cleanup', async () => {
    const { api, unlisteners } = createApi();
    const bridge = await startQuotaBridge(api);

    await bridge.refresh('manual');
    await bridge.stop();

    expect(api.invoke).toHaveBeenLastCalledWith('refresh_rate_limits', {
      reason: 'manual',
    });
    for (const unlisten of unlisteners) {
      expect(unlisten).toHaveBeenCalledTimes(1);
    }
  });
});
