import { LogicalPosition, LogicalSize } from '@tauri-apps/api/dpi';
import {
  availableMonitors,
  currentMonitor,
  getCurrentWindow,
  primaryMonitor,
} from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import {
  STANDARD_GEOMETRY,
  collapsedRectFromExpanded,
  expandedRect,
  restoredCollapsedRect,
  type DesktopPreferences,
  type Point,
  type Rect,
  type MonitorWorkArea,
  type WidgetGeometry,
} from './window.geometry';

export interface WindowAdapter {
  outerPosition(): Promise<Point>;
  currentMonitor(): Promise<MonitorWorkArea | null>;
  setPosition(position: Point): Promise<void>;
  setSize(size: { width: number; height: number }): Promise<void>;
  startDragging(): Promise<void>;
  setAlwaysOnTop(enabled: boolean): Promise<void>;
}

export interface WindowController {
  setExpanded(expanded: boolean): Promise<void>;
  startDragging(): Promise<void>;
  setAlwaysOnTop(enabled: boolean): Promise<void>;
}

export interface PlacementPersistenceAdapter {
  loadDesktopPreferences(): Promise<DesktopPreferences>;
  saveWidgetPlacement(): Promise<void>;
  availableMonitors(): Promise<MonitorWorkArea[]>;
  primaryMonitor(): Promise<MonitorWorkArea | null>;
  setPosition(position: Point): Promise<void>;
  setSize(size: { width: number; height: number }): Promise<void>;
  setAlwaysOnTop(enabled: boolean): Promise<void>;
  show(): Promise<void>;
  onMoved(handler: () => void): Promise<() => void>;
}

export interface PlacementPersistence {
  restoreAndShow(): Promise<void>;
  startMoveSaveDebounce(): Promise<() => Promise<void>>;
}

const FALLBACK_WORK_AREA = { x: 0, y: 0, width: 1920, height: 1080 };
const DEFAULT_MOVE_SAVE_DEBOUNCE_MS = 300;
export const WEEKLY_COLLAPSE_RESIZE_DELAY_MS = 280;

function currentRingCenter(
  rect: Rect,
  expanded: boolean,
  geometry: WidgetGeometry,
): Point {
  return expanded
    ? {
        x: rect.x + geometry.expandedRingCenterX,
        y: rect.y + geometry.expandedCanvas.height / 2,
      }
    : {
        x: rect.x + geometry.collapsedRingCenterX,
        y: rect.y + geometry.collapsedCanvas.height / 2,
      };
}

async function applyRect(adapter: WindowAdapter, current: Rect, target: Rect): Promise<void> {
  const movesLeftOrUp = target.x < current.x || target.y < current.y;
  if (movesLeftOrUp) {
    await adapter.setPosition({ x: target.x, y: target.y });
    await adapter.setSize({ width: target.width, height: target.height });
    return;
  }
  await adapter.setSize({ width: target.width, height: target.height });
  await adapter.setPosition({ x: target.x, y: target.y });
}

export function createWindowController(
  adapter: WindowAdapter,
  geometry: WidgetGeometry = STANDARD_GEOMETRY,
  collapseResizeDelayMs = 0,
): WindowController {
  let queue = Promise.resolve();
  let expanded = false;
  let requestedExpanded = false;
  let collapseTimer: ReturnType<typeof setTimeout> | null = null;
  let resolvePendingCollapse: (() => void) | null = null;

  function enqueue(work: () => Promise<void>): Promise<void> {
    queue = queue.then(work, work);
    return queue;
  }

  function cancelPendingCollapse() {
    if (collapseTimer !== null) {
      clearTimeout(collapseTimer);
      collapseTimer = null;
    }
    if (resolvePendingCollapse !== null) {
      resolvePendingCollapse();
      resolvePendingCollapse = null;
    }
  }

  async function applyExpanded(nextExpanded: boolean, cancelIfStale: boolean) {
    const position = await adapter.outerPosition();
    const monitor = await adapter.currentMonitor();
    if (cancelIfStale && requestedExpanded !== nextExpanded) {
      return;
    }
    const workArea = monitor?.workArea ?? FALLBACK_WORK_AREA;
    const current: Rect = {
      x: position.x,
      y: position.y,
      ...(expanded ? geometry.expandedCanvas : geometry.collapsedCanvas),
    };
    if (nextExpanded === expanded) {
      return;
    }
    const target = nextExpanded
      ? expandedRect({
          ringCenter: currentRingCenter(current, expanded, geometry),
          workArea,
          geometry,
        })
      : collapsedRectFromExpanded({ expanded: current, workArea, geometry });
    await applyRect(adapter, current, target);
    expanded = nextExpanded;
  }

  return {
    setExpanded(nextExpanded) {
      requestedExpanded = nextExpanded;
      cancelPendingCollapse();

      if (collapseResizeDelayMs <= 0) {
        return enqueue(() => applyExpanded(nextExpanded, false));
      }

      if (nextExpanded || !expanded) {
        return enqueue(() => applyExpanded(nextExpanded, true));
      }

      return new Promise<void>((resolve, reject) => {
        resolvePendingCollapse = resolve;
        collapseTimer = setTimeout(() => {
          collapseTimer = null;
          resolvePendingCollapse = null;
          void enqueue(() => applyExpanded(false, true)).then(resolve, reject);
        }, collapseResizeDelayMs);
      });
    },
    startDragging() {
      return enqueue(() => adapter.startDragging());
    },
    setAlwaysOnTop(enabled) {
      return enqueue(() => adapter.setAlwaysOnTop(enabled));
    },
  };
}

export function createPlacementPersistence(
  adapter: PlacementPersistenceAdapter,
  geometry: WidgetGeometry = STANDARD_GEOMETRY,
  debounceMs = DEFAULT_MOVE_SAVE_DEBOUNCE_MS,
): PlacementPersistence {
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  async function saveCurrentPlacement() {
    await adapter.saveWidgetPlacement();
  }

  return {
    async restoreAndShow() {
      const [preferences, monitors] = await Promise.all([
        adapter.loadDesktopPreferences(),
        adapter.availableMonitors(),
      ]);
      const primary = await adapter.primaryMonitor();
      const target = restoredCollapsedRect({
        preferences,
        monitors,
        primaryMonitor: primary,
        geometry,
      });
      await adapter.setAlwaysOnTop(preferences.alwaysOnTop);
      await adapter.setSize(geometry.collapsedCanvas);
      await adapter.setPosition({ x: target.x, y: target.y });
      await adapter.show();
    },
    async startMoveSaveDebounce() {
      const unlisten = await adapter.onMoved(() => {
        if (saveTimer !== null) {
          clearTimeout(saveTimer);
        }
        saveTimer = setTimeout(() => {
          saveTimer = null;
          void saveCurrentPlacement();
        }, debounceMs);
      });
      return async () => {
        if (saveTimer !== null) {
          clearTimeout(saveTimer);
          saveTimer = null;
        }
        unlisten();
      };
    },
  };
}

function monitorToLogical(monitor: Awaited<ReturnType<typeof currentMonitor>>): MonitorWorkArea | null {
  if (monitor === null) {
    return null;
  }
  const logicalPosition = monitor.workArea.position.toLogical(monitor.scaleFactor);
  const logicalSize = monitor.workArea.size.toLogical(monitor.scaleFactor);
  return {
    name: monitor.name,
    scaleFactor: monitor.scaleFactor,
    workArea: {
      x: logicalPosition.x,
      y: logicalPosition.y,
      width: logicalSize.width,
      height: logicalSize.height,
    },
  };
}

export function createTauriWindowController(
  geometry: WidgetGeometry = STANDARD_GEOMETRY,
  collapseResizeDelayMs = 0,
): WindowController {
  const appWindow = getCurrentWindow();
  return createWindowController({
    async outerPosition() {
      const position = await appWindow.outerPosition();
      const scaleFactor = await appWindow.scaleFactor();
      const logicalPosition = position.toLogical(scaleFactor);
      return { x: logicalPosition.x, y: logicalPosition.y };
    },
    async currentMonitor() {
      const monitor = await currentMonitor();
      return monitorToLogical(monitor);
    },
    async setPosition(position) {
      await appWindow.setPosition(new LogicalPosition(position.x, position.y));
    },
    async setSize(size) {
      await appWindow.setSize(new LogicalSize(size.width, size.height));
    },
    async startDragging() {
      await appWindow.startDragging();
    },
    async setAlwaysOnTop(enabled) {
      await appWindow.setAlwaysOnTop(enabled);
    },
  }, geometry, collapseResizeDelayMs);
}

export function createTauriPlacementPersistence(
  geometry: WidgetGeometry = STANDARD_GEOMETRY,
): PlacementPersistence {
  const appWindow = getCurrentWindow();
  return createPlacementPersistence({
    async loadDesktopPreferences() {
      return invoke<DesktopPreferences>('load_desktop_preferences');
    },
    async saveWidgetPlacement() {
      await invoke('save_widget_placement');
    },
    async availableMonitors() {
      const monitors = await availableMonitors();
      return monitors.flatMap((monitor) => {
        const logical = monitorToLogical(monitor);
        return logical === null ? [] : [logical];
      });
    },
    async primaryMonitor() {
      return monitorToLogical(await primaryMonitor());
    },
    async setPosition(position) {
      await appWindow.setPosition(new LogicalPosition(position.x, position.y));
    },
    async setSize(size) {
      await appWindow.setSize(new LogicalSize(size.width, size.height));
    },
    async setAlwaysOnTop(enabled) {
      await appWindow.setAlwaysOnTop(enabled);
    },
    async show() {
      await appWindow.show();
    },
    async onMoved(handler) {
      const unlisten = await appWindow.onMoved(() => {
        handler();
      });
      return unlisten;
    },
  }, geometry);
}
