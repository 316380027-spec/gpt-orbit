import { useEffect, useMemo, type ReactNode } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { startQuotaBridge } from '../features/quota/quota.bridge';
import { useQuotaStore } from '../features/quota/quota.store';
import type {
  QuotaConnectionStatus,
  RateLimitState,
} from '../features/quota/quota.types';
import { startResetCreditBridge } from '../features/reset-credits/reset-credits.bridge';
import { useResetCreditStore } from '../features/reset-credits/reset-credits.store';
import type { ResetCreditState } from '../features/reset-credits/reset-credits.types';
import {
  createTauriPlacementPersistence,
  createTauriWindowController,
  WEEKLY_COLLAPSE_RESIZE_DELAY_MS,
} from '../features/window/window.controller';
import {
  STANDARD_GEOMETRY,
  WEEKLY_GEOMETRY,
} from '../features/window/window.geometry';
import type { AppVariant } from '../appVariant';

export interface AppWidgetContext {
  snapshot: RateLimitState | null;
  status: QuotaConnectionStatus;
  resetCredits: ResetCreditState | null;
  onDragStart: () => Promise<void>;
  onExpandedChange: (expanded: boolean) => void;
  onResetExpired: () => void;
}

interface AppShellProps {
  renderWidget: (context: AppWidgetContext) => ReactNode;
  variant: AppVariant;
}

export function AppShell({ renderWidget, variant }: AppShellProps) {
  const snapshot = useQuotaStore((state) => state.snapshot);
  const status = useQuotaStore((state) => state.status);
  const resetCredits = useResetCreditStore((state) => state.snapshot);
  const geometry = variant === 'weekly' ? WEEKLY_GEOMETRY : STANDARD_GEOMETRY;
  const windowController = useMemo(
    () =>
      isTauri()
        ? createTauriWindowController(
            geometry,
            variant === 'weekly' ? WEEKLY_COLLAPSE_RESIZE_DELAY_MS : 0,
          )
        : {
            async setExpanded(_expanded: boolean) {
              await Promise.resolve();
            },
            async startDragging() {
              await Promise.resolve();
            },
            async setAlwaysOnTop(_enabled: boolean) {
              await Promise.resolve();
            },
          },
    [geometry, variant],
  );
  const placementPersistence = useMemo(
    () =>
      isTauri()
        ? createTauriPlacementPersistence(geometry)
        : {
            async restoreAndShow() {
              await Promise.resolve();
            },
            async startMoveSaveDebounce() {
              return async () => {
                await Promise.resolve();
              };
            },
          },
    [geometry],
  );

  useEffect(() => {
    if (!isTauri()) {
      return undefined;
    }

    let stopped = false;
    let stopBridge: (() => Promise<void>) | null = null;

    void startQuotaBridge({
      invoke,
      listen,
      open(url) {
        window.open(url, '_blank', 'noopener,noreferrer');
      },
    }).then((bridge) => {
      if (stopped) {
        void bridge.stop();
        return;
      }
      stopBridge = bridge.stop;
    });

    return () => {
      stopped = true;
      if (stopBridge !== null) {
        void stopBridge();
      }
    };
  }, []);

  useEffect(() => {
    if (variant !== 'weekly' || !isTauri()) {
      return undefined;
    }

    let stopped = false;
    let stopBridge: (() => Promise<void>) | null = null;

    void startResetCreditBridge({ invoke, listen })
      .then((bridge) => {
        if (stopped) {
          void bridge.stop();
          return;
        }
        stopBridge = bridge.stop;
      })
      .catch(() => undefined);

    return () => {
      stopped = true;
      if (stopBridge !== null) {
        void stopBridge();
      }
    };
  }, [variant]);

  useEffect(() => {
    if (!isTauri()) {
      return undefined;
    }

    let stopped = false;
    let stopMoveSave: (() => Promise<void>) | null = null;

    void placementPersistence.startMoveSaveDebounce().then((stop) => {
      if (stopped) {
        void stop();
        return;
      }
      stopMoveSave = stop;
    });

    return () => {
      stopped = true;
      if (stopMoveSave !== null) {
        void stopMoveSave();
      }
    };
  }, [placementPersistence]);

  useEffect(() => {
    if (!isTauri()) {
      return undefined;
    }

    let stopped = false;
    const unlisteners: Array<() => void> = [];
    void Promise.all([
      listen('desktop://always-on-top', () => undefined),
      listen('desktop://visibility', () => undefined),
    ]).then((registered) => {
      if (stopped) {
        registered.forEach((unlisten) => {
          unlisten();
        });
        return;
      }
      unlisteners.push(...registered);
    });

    return () => {
      stopped = true;
      unlisteners.forEach((unlisten) => {
        unlisten();
      });
    };
  }, []);

  return (
    <main aria-label="Gpt Orbit" className="app-shell">
      {renderWidget({
        snapshot,
        status,
        resetCredits,
        onDragStart: windowController.startDragging,
        onExpandedChange(expanded) {
          void windowController.setExpanded(expanded);
        },
        onResetExpired() {
          void useQuotaStore.getState().setStatus('stale');
        },
      })}
    </main>
  );
}
