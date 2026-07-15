import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  startResetCreditBridge,
  type ResetCreditBridgeApi,
} from './reset-credits.bridge';
import { useResetCreditStore } from './reset-credits.store';
import type { ResetCreditState } from './reset-credits.types';

const initialSnapshot: ResetCreditState = {
  availableCount: 3,
  fetchedAt: 1_800_000_000,
  stale: false,
  authRequired: false,
};

type EventHandler = (event: { payload: unknown }) => void;

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

function createApi(snapshot: unknown = initialSnapshot) {
  const handlers = new Map<string, EventHandler>();
  const unlisten = vi.fn();
  const api: ResetCreditBridgeApi = {
    invoke: vi.fn(async (command: string) =>
      command === 'get_reset_credits' ? snapshot : undefined,
    ),
    listen: vi.fn(async (event: string, handler: EventHandler) => {
      handlers.set(event, handler);
      return unlisten;
    }),
  };
  return { api, handlers, unlisten };
}

describe('reset credit bridge', () => {
  beforeEach(() => {
    useResetCreditStore.setState({ snapshot: null });
  });

  it('listens for reset credit updates before reading retained state', async () => {
    const { api } = createApi();

    const bridge = await startResetCreditBridge(api);

    expect(api.invoke).toHaveBeenNthCalledWith(1, 'get_reset_credits');
    expect(api.listen).toHaveBeenCalledWith(
      'reset-credits://updated',
      expect.any(Function),
    );
    expect(vi.mocked(api.listen).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(api.invoke).mock.invocationCallOrder[0],
    );
    expect(useResetCreditStore.getState().snapshot).toEqual(initialSnapshot);

    await bridge.stop();
  });

  it('does not let an old retained state overwrite an update received while invoke is pending', async () => {
    const { promise, resolve } = createDeferred<unknown>();
    const { api, handlers } = createApi();
    vi.mocked(api.invoke).mockReturnValueOnce(promise);
    const updated = {
      ...initialSnapshot,
      availableCount: 2,
      fetchedAt: initialSnapshot.fetchedAt + 1,
    };

    const bridgePromise = startResetCreditBridge(api);
    await vi.waitFor(() => {
      expect(api.invoke).toHaveBeenCalledWith('get_reset_credits');
    });
    handlers.get('reset-credits://updated')?.({ payload: updated });
    resolve(initialSnapshot);

    const bridge = await bridgePromise;
    expect(useResetCreditStore.getState().snapshot).toEqual(updated);
    await bridge.stop();
  });

  it('preserves an equal-timestamp stale event over a pending fresh retained state', async () => {
    const { promise, resolve } = createDeferred<unknown>();
    const { api, handlers } = createApi();
    vi.mocked(api.invoke).mockReturnValueOnce(promise);
    const stale = { ...initialSnapshot, stale: true };

    const bridgePromise = startResetCreditBridge(api);
    await vi.waitFor(() => {
      expect(api.invoke).toHaveBeenCalledWith('get_reset_credits');
    });
    handlers.get('reset-credits://updated')?.({ payload: stale });
    resolve(initialSnapshot);

    const bridge = await bridgePromise;
    expect(useResetCreditStore.getState().snapshot).toEqual(stale);
    await bridge.stop();
  });

  it.each([-1, 1.5, '3'])('rejects invalid retained and event counts: %s', async (availableCount) => {
    const { api, handlers } = createApi({ ...initialSnapshot, availableCount });
    const bridge = await startResetCreditBridge(api);

    expect(useResetCreditStore.getState().snapshot).toBeNull();

    handlers.get('reset-credits://updated')?.({
      payload: { ...initialSnapshot, availableCount },
    });
    expect(useResetCreditStore.getState().snapshot).toBeNull();

    await bridge.stop();
  });

  it('accepts null availability from retained state and update events', async () => {
    const retained = { ...initialSnapshot, availableCount: null };
    const updated = { ...retained, fetchedAt: retained.fetchedAt + 1 };
    const { api, handlers } = createApi(retained);
    const bridge = await startResetCreditBridge(api);

    expect(useResetCreditStore.getState().snapshot).toEqual(retained);

    handlers.get('reset-credits://updated')?.({ payload: updated });
    expect(useResetCreditStore.getState().snapshot).toEqual(updated);

    await bridge.stop();
  });

  it('rejects payloads without auth status and accepts auth-required updates', async () => {
    const { authRequired: _, ...missingAuth } = initialSnapshot;
    const { api, handlers } = createApi(missingAuth);
    const bridge = await startResetCreditBridge(api);
    expect(useResetCreditStore.getState().snapshot).toBeNull();

    const authRequired = {
      ...initialSnapshot,
      stale: true,
      authRequired: true,
    };
    handlers.get('reset-credits://updated')?.({ payload: authRequired });
    expect(useResetCreditStore.getState().snapshot).toEqual(authRequired);

    await bridge.stop();
  });

  it('clears retained state when listener registration fails', async () => {
    const listenerFailure = new Error('reset credit listener failed');
    const { api } = createApi();
    vi.mocked(api.listen).mockRejectedValueOnce(listenerFailure);

    await expect(startResetCreditBridge(api)).rejects.toBe(listenerFailure);

    expect(useResetCreditStore.getState().snapshot).toBeNull();
  });

  it('clears an old snapshot when the initial command fails', async () => {
    const invokeFailure = new Error('get reset credits failed');
    const oldSnapshot = { ...initialSnapshot, availableCount: 1 };
    const { api } = createApi();
    useResetCreditStore.getState().applySnapshot(oldSnapshot);
    vi.mocked(api.invoke).mockRejectedValueOnce(invokeFailure);

    await expect(startResetCreditBridge(api)).rejects.toBe(invokeFailure);

    expect(useResetCreditStore.getState().snapshot).toBeNull();
    expect(api.listen).toHaveBeenCalledWith(
      'reset-credits://updated',
      expect.any(Function),
    );
  });

  it('forwards refresh reasons and stops idempotently', async () => {
    const { api, handlers, unlisten } = createApi();
    const bridge = await startResetCreditBridge(api);

    await bridge.refresh('manual');
    await bridge.stop();
    await bridge.stop();

    handlers.get('reset-credits://updated')?.({
      payload: { ...initialSnapshot, availableCount: 0 },
    });

    expect(api.invoke).toHaveBeenLastCalledWith('refresh_reset_credits', {
      reason: 'manual',
    });
    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(useResetCreditStore.getState().snapshot).toEqual(initialSnapshot);
  });
});
