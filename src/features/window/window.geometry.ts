export interface Point {
  x: number;
  y: number;
}

export interface Rect extends Point {
  width: number;
  height: number;
}

export interface WorkArea extends Rect {}

export interface MonitorWorkArea {
  name: string | null;
  workArea: WorkArea;
  scaleFactor?: number;
}

export interface DesktopPreferences {
  alwaysOnTop: boolean;
  monitorName: string | null;
  offsetLogical: [number, number] | null;
  scaleFactor: number | null;
}

export interface WidgetGeometry {
  collapsedCanvas: { width: number; height: number };
  expandedCanvas: { width: number; height: number };
  collapsedRingCenterX: number;
  expandedRingCenterX: number;
}

export const VISUAL_MARGIN = 12;
export const COLLAPSED_VISIBLE = { width: 148, height: 148 };
export const EXPANDED_VISIBLE = { width: 245, height: 112 };
export const COLLAPSED_CANVAS = {
  width: COLLAPSED_VISIBLE.width + VISUAL_MARGIN * 2,
  height: COLLAPSED_VISIBLE.height + VISUAL_MARGIN * 2,
};
export const EXPANDED_CANVAS = {
  width: EXPANDED_VISIBLE.width + VISUAL_MARGIN * 2,
  height: EXPANDED_VISIBLE.height + VISUAL_MARGIN * 2,
};
export const CAPSULE_RING_CENTER_FROM_BODY_LEFT = 56;
export const STANDARD_GEOMETRY: WidgetGeometry = {
  collapsedCanvas: COLLAPSED_CANVAS,
  expandedCanvas: EXPANDED_CANVAS,
  collapsedRingCenterX: COLLAPSED_CANVAS.width / 2,
  expandedRingCenterX: VISUAL_MARGIN + CAPSULE_RING_CENTER_FROM_BODY_LEFT,
};
export const WEEKLY_GEOMETRY: WidgetGeometry = {
  collapsedCanvas: { width: 104, height: 86 },
  expandedCanvas: { width: 153, height: 68 },
  collapsedRingCenterX: 43,
  expandedRingCenterX: 36,
};
export const EXPANDED_RING_CENTER_FROM_CANVAS_LEFT = STANDARD_GEOMETRY.expandedRingCenterX;
export const COLLAPSED_RING_CENTER_FROM_CANVAS_LEFT = STANDARD_GEOMETRY.collapsedRingCenterX;

export function clampRect(rect: Rect, workArea: WorkArea): Rect {
  return {
    ...rect,
    x: Math.min(Math.max(rect.x, workArea.x), workArea.x + workArea.width - rect.width),
    y: Math.min(Math.max(rect.y, workArea.y), workArea.y + workArea.height - rect.height),
  };
}

export function expandedRect({
  ringCenter,
  workArea,
  geometry = STANDARD_GEOMETRY,
}: {
  ringCenter: Point;
  workArea: WorkArea;
  geometry?: WidgetGeometry;
}): Rect {
  return clampRect(
    {
      x: ringCenter.x - geometry.expandedRingCenterX,
      y: ringCenter.y - geometry.expandedCanvas.height / 2,
      ...geometry.expandedCanvas,
    },
    workArea,
  );
}

export function collapsedRectFromExpanded({
  expanded,
  workArea,
  geometry = STANDARD_GEOMETRY,
}: {
  expanded: Rect;
  workArea: WorkArea;
  geometry?: WidgetGeometry;
}): Rect {
  const adjustedRingCenter = {
    x: expanded.x + geometry.expandedRingCenterX,
    y: expanded.y + geometry.expandedCanvas.height / 2,
  };
  return clampRect(
    {
      x: adjustedRingCenter.x - geometry.collapsedRingCenterX,
      y: adjustedRingCenter.y - geometry.collapsedCanvas.height / 2,
      ...geometry.collapsedCanvas,
    },
    workArea,
  );
}

export function toPhysicalRect(rect: Rect, scaleFactor: number): Rect {
  return {
    x: Math.round(rect.x * scaleFactor),
    y: Math.round(rect.y * scaleFactor),
    width: Math.round(rect.width * scaleFactor),
    height: Math.round(rect.height * scaleFactor),
  };
}

export function selectWorkArea(
  monitors: MonitorWorkArea[],
  preferredName: string | null,
): WorkArea {
  return (
    monitors.find((monitor) => monitor.name === preferredName)?.workArea ??
    monitors[0]?.workArea ?? {
      x: 0,
      y: 0,
      width: 1920,
      height: 1080,
    }
  );
}

export function restoredCollapsedRect({
  preferences,
  monitors,
  primaryMonitor,
  geometry = STANDARD_GEOMETRY,
}: {
  preferences: DesktopPreferences;
  monitors: MonitorWorkArea[];
  primaryMonitor?: MonitorWorkArea | null;
  geometry?: WidgetGeometry;
}): Rect {
  const workArea =
    monitors.find((monitor) => monitor.name === preferences.monitorName)?.workArea ??
    primaryMonitor?.workArea ??
    selectWorkArea(monitors, preferences.monitorName);
  const offset = preferences.offsetLogical;
  const unclamped = offset
    ? {
        x: workArea.x + offset[0],
        y: workArea.y + offset[1],
        ...geometry.collapsedCanvas,
      }
    : {
        x: workArea.x + workArea.width - geometry.collapsedCanvas.width - 24,
        y: workArea.y + 24,
        ...geometry.collapsedCanvas,
      };

  return clampRect(unclamped, workArea);
}

export function preferencesFromPlacement({
  rect,
  monitor,
  alwaysOnTop,
}: {
  rect: Rect;
  monitor: MonitorWorkArea;
  alwaysOnTop: boolean;
}): DesktopPreferences {
  return {
    alwaysOnTop,
    monitorName: monitor.name,
    offsetLogical: [rect.x - monitor.workArea.x, rect.y - monitor.workArea.y],
    scaleFactor: monitor.scaleFactor ?? null,
  };
}
