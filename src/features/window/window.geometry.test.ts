import { describe, expect, it } from 'vitest';
import {
  COLLAPSED_CANVAS,
  EXPANDED_CANVAS,
  STANDARD_GEOMETRY,
  WEEKLY_GEOMETRY,
  clampRect,
  collapsedRectFromExpanded,
  expandedRect,
  selectWorkArea,
  restoredCollapsedRect,
  toPhysicalRect,
} from './window.geometry';

const workArea = { x: 0, y: 0, width: 1920, height: 1080 };

describe('window geometry', () => {
  it('defines native canvases with a 12px transparent margin around the visible widget', () => {
    expect(COLLAPSED_CANVAS).toEqual({ width: 172, height: 172 });
    expect(EXPANDED_CANVAS).toEqual({ width: 269, height: 136 });
  });

  it('defines explicit standard and weekly canvases and ring anchors', () => {
    expect(STANDARD_GEOMETRY).toEqual({
      collapsedCanvas: { width: 172, height: 172 },
      expandedCanvas: { width: 269, height: 136 },
      collapsedRingCenterX: 86,
      expandedRingCenterX: 68,
    });
    expect(WEEKLY_GEOMETRY).toEqual({
      collapsedCanvas: { width: 104, height: 86 },
      expandedCanvas: { width: 153, height: 68 },
      collapsedRingCenterX: 43,
      expandedRingCenterX: 36,
    });
  });

  it('keeps the weekly ring anchored at the right edge and collapses to the adjusted anchor', () => {
    const expanded = expandedRect({
      ringCenter: { x: 1886, y: 100 },
      workArea,
      geometry: WEEKLY_GEOMETRY,
    });

    expect(expanded).toEqual({ x: 1767, y: 66, width: 153, height: 68 });
    expect(
      collapsedRectFromExpanded({ expanded, workArea, geometry: WEEKLY_GEOMETRY }),
    ).toEqual({ x: 1760, y: 57, width: 104, height: 86 });
  });

  it('keeps the quota ring anchored while expanding and clamps at the right edge', () => {
    expect(expandedRect({ ringCenter: { x: 1800, y: 100 }, workArea })).toEqual({
      x: 1651,
      y: 32,
      width: 269,
      height: 136,
    });
  });

  it('clamps left, top, right, and bottom edges inside the work area', () => {
    expect(clampRect({ x: -20, y: -15, width: 269, height: 136 }, workArea)).toEqual({
      x: 0,
      y: 0,
      width: 269,
      height: 136,
    });
    expect(clampRect({ x: 1900, y: 1050, width: 269, height: 136 }, workArea)).toEqual({
      x: 1651,
      y: 944,
      width: 269,
      height: 136,
    });
  });

  it('converts logical rectangles to physical pixels for common Windows scale factors', () => {
    const rect = { x: 10, y: 20, width: 172, height: 136 };

    expect(toPhysicalRect(rect, 1)).toEqual(rect);
    expect(toPhysicalRect(rect, 1.25)).toEqual({ x: 13, y: 25, width: 215, height: 170 });
    expect(toPhysicalRect(rect, 1.5)).toEqual({ x: 15, y: 30, width: 258, height: 204 });
    expect(toPhysicalRect(rect, 2)).toEqual({ x: 20, y: 40, width: 344, height: 272 });
  });

  it('collapses from an adjusted safe expanded position without persisting transient capsule size', () => {
    expect(
      collapsedRectFromExpanded({
        expanded: { x: 1651, y: 32, width: 269, height: 136 },
        workArea,
      }),
    ).toEqual({ x: 1633, y: 14, width: 172, height: 172 });
  });

  it('falls back to the primary monitor work area when the saved monitor was removed', () => {
    const monitors = [
      { name: 'Primary', workArea },
      { name: 'Side', workArea: { x: 1920, y: 0, width: 1280, height: 900 } },
    ];

    expect(selectWorkArea(monitors, 'Missing')).toEqual(workArea);
    expect(selectWorkArea(monitors, 'Side')).toEqual({ x: 1920, y: 0, width: 1280, height: 900 });
  });

  it('restores collapsed placement from saved monitor offset and clamps it inside the selected work area', () => {
    const monitors = [
      { name: 'Primary', workArea },
      { name: 'Side', workArea: { x: 1920, y: 0, width: 1280, height: 900 } },
    ];

    expect(
      restoredCollapsedRect({
        preferences: {
          alwaysOnTop: true,
          monitorName: 'Side',
          offsetLogical: [1200, 850],
          scaleFactor: 1.25,
        },
        monitors,
        primaryMonitor: monitors[0],
      }),
    ).toEqual({ x: 3028, y: 728, width: 172, height: 172 });
  });

  it('restores weekly placement independently with the weekly collapsed canvas', () => {
    const monitors = [
      { name: 'Primary', workArea },
      { name: 'Side', workArea: { x: 1920, y: 0, width: 1280, height: 900 } },
    ];

    expect(
      restoredCollapsedRect({
        preferences: {
          alwaysOnTop: true,
          monitorName: 'Side',
          offsetLogical: [1200, 850],
          scaleFactor: 1.25,
        },
        monitors,
        primaryMonitor: monitors[0],
        geometry: WEEKLY_GEOMETRY,
      }),
    ).toEqual({ x: 3096, y: 814, width: 104, height: 86 });
  });

  it('restores to the primary monitor when the saved monitor is no longer available', () => {
    const side = { name: 'Side', workArea: { x: 1920, y: 0, width: 1280, height: 900 } };
    const primary = { name: 'Primary', workArea };

    expect(
      restoredCollapsedRect({
        preferences: {
          alwaysOnTop: true,
          monitorName: 'Missing',
          offsetLogical: [1200, 850],
          scaleFactor: 1.25,
        },
        monitors: [side, primary],
        primaryMonitor: primary,
      }),
    ).toEqual({ x: 1200, y: 850, width: 172, height: 172 });
  });
});
