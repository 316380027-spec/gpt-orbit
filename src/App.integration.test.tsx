import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RateLimitState } from './features/quota/quota.types';
import { STANDARD_GEOMETRY, WEEKLY_GEOMETRY } from './features/window/window.geometry';

const now = new Date(2026, 6, 12, 7, 12, 0).getTime();
const WEEKLY_COLLAPSE_RESIZE_DELAY_MS = 280;

const snapshot: RateLimitState = {
  fiveHour: {
    kind: 'fiveHour',
    usedPercent: 27,
    remainingPercent: 73,
    windowDurationMins: 300,
    resetsAt: now / 1000 + 2 * 60 * 60 + 18 * 60,
  },
  weekly: {
    kind: 'weekly',
    usedPercent: 42,
    remainingPercent: 58,
    windowDurationMins: 10_080,
    resetsAt: new Date(2026, 6, 13, 9, 30, 0).getTime() / 1000,
  },
  other: [],
  planType: 'plus',
  reachedType: null,
  fetchedAt: now / 1000,
  source: 'read',
  stale: false,
};

const eventHandlers = new Map<string, (event: { payload: unknown }) => void>();
const resetCreditSnapshot = {
  availableCount: 3,
  fetchedAt: now / 1000,
  stale: false,
  authRequired: false,
};
async function defaultInvoke(command: string) {
  if (command === 'get_quota_bridge_state') {
    return { snapshot, status: 'live' };
  }
  if (command === 'get_reset_credits') return resetCreditSnapshot;
  return null;
}
const invoke = vi.fn(defaultInvoke);
async function defaultListen(
  event: string,
  handler: (event: { payload: unknown }) => void,
): Promise<() => void> {
  eventHandlers.set(event, handler);
  return vi.fn();
}
const listen = vi.fn(defaultListen);
const restoreAndShow = vi.fn(async () => undefined);
const createTauriWindowController = vi.fn(() => ({
  setExpanded: vi.fn(async () => undefined),
  startDragging: vi.fn(async () => undefined),
  setAlwaysOnTop: vi.fn(async () => undefined),
}));
const createTauriPlacementPersistence = vi.fn(() => ({
  restoreAndShow,
  startMoveSaveDebounce: vi.fn(async () => vi.fn(async () => undefined)),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke,
  isTauri: () => true,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen,
}));

vi.mock('./features/window/window.controller', () => ({
  createTauriWindowController,
  createTauriPlacementPersistence,
  WEEKLY_COLLAPSE_RESIZE_DELAY_MS,
}));

function widget() {
  return screen.getByRole('button', { name: 'Gpt Orbit' });
}

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

describe('App desktop integration', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(now);
    eventHandlers.clear();
    invoke.mockReset();
    invoke.mockImplementation(defaultInvoke);
    listen.mockReset();
    listen.mockImplementation(defaultListen);
    restoreAndShow.mockClear();
    createTauriWindowController.mockClear();
    createTauriPlacementPersistence.mockClear();
  });

  afterEach(() => {
    cleanup();
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
    vi.resetModules();
  });

  it('keeps the standard variant isolated from reset credit commands and events', async () => {
    const { default: App } = await import('./App');

    render(<App variant="standard" />);

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(invoke).toHaveBeenCalledWith('get_quota_bridge_state');
    expect(eventHandlers.has('rate-limits://updated')).toBe(true);
    expect(eventHandlers.has('desktop://always-on-top')).toBe(true);
    expect(eventHandlers.has('desktop://visibility')).toBe(true);
    const invokedCommands = invoke.mock.calls.map(([command]) => command);
    expect(invokedCommands).not.toContain('get_reset_credits');
    expect(invokedCommands).not.toContain('refresh_reset_credits');
    const listenedEvents = listen.mock.calls.map(([event]) => event);
    expect(listenedEvents).not.toContain('reset-credits://updated');

    fireEvent.pointerEnter(widget());
    act(() => {
      vi.advanceTimersByTime(150);
    });
    fireEvent.click(widget());
    expect(widget()).toHaveAttribute('data-face', 'back');

    act(() => {
      eventHandlers.get('desktop://always-on-top')?.({ payload: { enabled: false } });
      eventHandlers.get('desktop://visibility')?.({ payload: { visible: true } });
    });

    expect(widget()).toHaveAttribute('data-face', 'back');
  });

  it('starts the reset credit bridge only for the weekly variant', async () => {
    const { default: App } = await import('./App');

    render(<App variant="weekly" />);

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(invoke).toHaveBeenCalledWith('get_reset_credits');
    expect(listen).toHaveBeenCalledWith(
      'reset-credits://updated',
      expect.any(Function),
    );
  });

  it.each([
    ['standard', STANDARD_GEOMETRY],
    ['weekly', WEEKLY_GEOMETRY],
  ] as const)('passes the %s geometry to both native window factories', async (variant, geometry) => {
    const { default: App } = await import('./App');

    render(<App variant={variant} />);

    expect(createTauriWindowController).toHaveBeenCalledWith(
      geometry,
      variant === 'weekly' ? WEEKLY_COLLAPSE_RESIZE_DELAY_MS : 0,
    );
    expect(createTauriPlacementPersistence).toHaveBeenCalledWith(geometry);
  });

  it('unlistens once when reset credit listener registration finishes after cleanup', async () => {
    const deferredUnlisten = createDeferred<() => void>();
    const lateUnlisten = vi.fn();
    listen.mockImplementation(async (event, handler) => {
      eventHandlers.set(event, handler);
      return event === 'reset-credits://updated'
        ? deferredUnlisten.promise
        : vi.fn();
    });
    const { default: App } = await import('./App');

    render(<App variant="weekly" />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(listen.mock.calls.map(([event]) => event)).toContain(
      'reset-credits://updated',
    );

    cleanup();
    cleanup();
    deferredUnlisten.resolve(lateUnlisten);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(lateUnlisten).toHaveBeenCalledTimes(1);
    cleanup();
    expect(lateUnlisten).toHaveBeenCalledTimes(1);
  });

  it('leaves startup window restoration to native setup', async () => {
    const { default: App } = await import('./App');

    render(<App variant="weekly" />);

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(restoreAndShow).not.toHaveBeenCalled();
  });

  it('keeps quota running when the weekly bridge fails', async () => {
    const resetFailure = new Error('reset credit unavailable');
    invoke.mockImplementation(async (command: string) => {
      if (command === 'get_quota_bridge_state') {
        return { snapshot, status: 'live' };
      }
      if (command === 'get_reset_credits') throw resetFailure;
      return null;
    });
    const { default: App } = await import('./App');

    render(<App variant="weekly" />);

    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(invoke).toHaveBeenCalledWith('get_quota_bridge_state');
    expect(eventHandlers.has('rate-limits://updated')).toBe(true);
  });
});
