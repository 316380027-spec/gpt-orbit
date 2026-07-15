import { describe, expect, it, vi } from 'vitest';
import {
  createPlacementPersistence,
  createWindowController,
  type PlacementPersistenceAdapter,
  type WindowAdapter,
} from './window.controller';
import { WEEKLY_GEOMETRY } from './window.geometry';

function createAdapter(position = { x: 100, y: 100 }): WindowAdapter & { calls: string[] } {
  const calls: string[] = [];
  let currentPosition = position;
  return {
    calls,
    async outerPosition() {
      calls.push('outerPosition');
      return currentPosition;
    },
    async currentMonitor() {
      calls.push('currentMonitor');
      return { name: 'Primary', workArea: { x: 0, y: 0, width: 1920, height: 1080 }, scaleFactor: 1 };
    },
    async setPosition(next) {
      calls.push(`setPosition:${next.x},${next.y}`);
      currentPosition = next;
    },
    async setSize(next) {
      calls.push(`setSize:${next.width},${next.height}`);
    },
    async startDragging() {
      calls.push('startDragging');
    },
    async setAlwaysOnTop(enabled) {
      calls.push(`setAlwaysOnTop:${enabled}`);
    },
  };
}

describe('window controller', () => {
  it('expands by resizing before moving when growth goes right/down', async () => {
    const adapter = createAdapter({ x: 100, y: 100 });
    const controller = createWindowController(adapter);

    await controller.setExpanded(true);

    expect(adapter.calls).toContain('setSize:269,136');
    expect(adapter.calls.indexOf('setSize:269,136')).toBeLessThan(
      adapter.calls.findIndex((call) => call.startsWith('setPosition:')),
    );
  });

  it('moves before resizing when clamping left/up avoids one-frame clipping', async () => {
    const adapter = createAdapter({ x: 1800, y: 2 });
    const controller = createWindowController(adapter);

    await controller.setExpanded(true);

    expect(adapter.calls.findIndex((call) => call.startsWith('setPosition:'))).toBeLessThan(
      adapter.calls.indexOf('setSize:269,136'),
    );
  });

  it('collapses back to the adjusted safe collapsed canvas', async () => {
    const adapter = createAdapter({ x: 1800, y: 2 });
    const controller = createWindowController(adapter);

    await controller.setExpanded(true);
    await controller.setExpanded(false);

    expect(adapter.calls).toContain('setSize:172,172');
    expect(adapter.calls).toContain('setPosition:1633,2');
  });

  it('uses weekly canvases while preserving the adjusted right-edge ring anchor', async () => {
    const adapter = createAdapter({ x: 1800, y: 2 });
    const controller = createWindowController(adapter, WEEKLY_GEOMETRY);

    await controller.setExpanded(true);
    await controller.setExpanded(false);

    expect(adapter.calls).toContain('setSize:153,68');
    expect(adapter.calls).toContain('setPosition:1767,11');
    expect(adapter.calls).toContain('setSize:104,86');
    expect(adapter.calls).toContain('setPosition:1760,2');
  });

  it('keeps the weekly native canvas expanded until the CSS collapse finishes', async () => {
    vi.useFakeTimers();
    try {
      const adapter = createAdapter({ x: 100, y: 100 });
      const controller = createWindowController(adapter, WEEKLY_GEOMETRY, 280);
      await controller.setExpanded(true);
      adapter.calls.length = 0;

      const collapse = controller.setExpanded(false);
      await vi.advanceTimersByTimeAsync(279);
      expect(adapter.calls).not.toContain('setSize:104,86');

      await vi.advanceTimersByTimeAsync(1);
      await collapse;
      expect(adapter.calls).toContain('setSize:104,86');
    } finally {
      vi.useRealTimers();
    }
  });

  it('cancels a pending weekly collapse when hover returns', async () => {
    vi.useFakeTimers();
    try {
      const adapter = createAdapter({ x: 100, y: 100 });
      const controller = createWindowController(adapter, WEEKLY_GEOMETRY, 280);
      await controller.setExpanded(true);
      adapter.calls.length = 0;

      const collapse = controller.setExpanded(false);
      await vi.advanceTimersByTimeAsync(100);
      const expandAgain = controller.setExpanded(true);
      await Promise.all([collapse, expandAgain]);
      await vi.advanceTimersByTimeAsync(300);

      expect(adapter.calls).not.toContain('setSize:104,86');
    } finally {
      vi.useRealTimers();
    }
  });

  it('does not drift when asked to collapse while already collapsed', async () => {
    const adapter = createAdapter({ x: 1651, y: 32 });
    const controller = createWindowController(adapter);

    await controller.setExpanded(false);

    expect(adapter.calls).toEqual(['outerPosition', 'currentMonitor']);
  });

  it('serializes rapid expansion requests and delegates dragging/topmost operations', async () => {
    const adapter = createAdapter();
    const controller = createWindowController(adapter);
    const setExpanded = vi.spyOn(adapter, 'setSize');

    await Promise.all([
      controller.setExpanded(true),
      controller.setExpanded(false),
      controller.startDragging(),
      controller.setAlwaysOnTop(false),
    ]);

    expect(setExpanded).toHaveBeenCalledTimes(2);
    expect(adapter.calls).toContain('startDragging');
    expect(adapter.calls).toContain('setAlwaysOnTop:false');
  });
});

function createPlacementAdapter(): PlacementPersistenceAdapter & {
  calls: string[];
  moved?: () => void;
} {
  const calls: string[] = [];
  return {
    calls,
    async loadDesktopPreferences() {
      calls.push('loadDesktopPreferences');
      return {
        alwaysOnTop: false,
        monitorName: 'Side',
        offsetLogical: [1200, 850],
        scaleFactor: 1.25,
      };
    },
    async saveWidgetPlacement() {
      calls.push('saveWidgetPlacement');
    },
    async availableMonitors() {
      calls.push('availableMonitors');
      return [
        { name: 'Primary', workArea: { x: 0, y: 0, width: 1920, height: 1080 }, scaleFactor: 1 },
        { name: 'Side', workArea: { x: 1920, y: 0, width: 1280, height: 900 }, scaleFactor: 1.25 },
      ];
    },
    async primaryMonitor() {
      calls.push('primaryMonitor');
      return { name: 'Primary', workArea: { x: 0, y: 0, width: 1920, height: 1080 }, scaleFactor: 1 };
    },
    async setPosition(position) {
      calls.push(`setPosition:${position.x},${position.y}`);
    },
    async setSize(size) {
      calls.push(`setSize:${size.width},${size.height}`);
    },
    async setAlwaysOnTop(enabled) {
      calls.push(`setAlwaysOnTop:${enabled}`);
    },
    async show() {
      calls.push('show');
    },
    async onMoved(handler) {
      calls.push('onMoved');
      this.moved = handler;
      return () => {
        calls.push('unlistenMoved');
      };
    },
  };
}

describe('placement persistence', () => {
  it('restores saved placement, applies always-on-top, and shows the collapsed window', async () => {
    const adapter = createPlacementAdapter();
    const placement = createPlacementPersistence(adapter);

    await placement.restoreAndShow();

    expect(adapter.calls).toContain('setAlwaysOnTop:false');
    expect(adapter.calls).toContain('setSize:172,172');
    expect(adapter.calls).toContain('setPosition:3028,728');
    expect(adapter.calls.at(-1)).toBe('show');
  });

  it('restores and saves weekly placement with its independent collapsed canvas', async () => {
    vi.useFakeTimers();
    const adapter = createPlacementAdapter();
    const placement = createPlacementPersistence(adapter, WEEKLY_GEOMETRY);

    await placement.restoreAndShow();
    const stop = await placement.startMoveSaveDebounce();
    adapter.moved?.();
    await vi.advanceTimersByTimeAsync(300);

    expect(adapter.calls).toContain('setSize:104,86');
    expect(adapter.calls).toContain('setPosition:3096,814');
    expect(adapter.calls).toContain('saveWidgetPlacement');
    await stop();
    vi.useRealTimers();
  });

  it('debounces move saves by 300ms and delegates position-only persistence to native setup', async () => {
    vi.useFakeTimers();
    const adapter = createPlacementAdapter();
    const saveWidgetPlacement = vi.spyOn(adapter, 'saveWidgetPlacement');
    const placement = createPlacementPersistence(adapter);

    const stop = await placement.startMoveSaveDebounce();
    adapter.moved?.();
    adapter.moved?.();
    await vi.advanceTimersByTimeAsync(299);
    expect(adapter.calls.some((call) => call.startsWith('saveWidgetPlacement'))).toBe(false);

    await vi.advanceTimersByTimeAsync(1);

    expect(saveWidgetPlacement).toHaveBeenCalledWith();
    await stop();
    expect(adapter.calls).toContain('unlistenMoved');
    vi.useRealTimers();
  });
});
