import { useEffect, useMemo } from 'react';
import type { ResetCreditState } from '../reset-credits/reset-credits.types';
import { ResetCreditBadge } from '../reset-credits/ResetCreditBadge';
import type { QuotaConnectionStatus, RateLimitState } from '../quota/quota.types';
import { formatWeeklyCompactCountdown, formatWeeklyReset } from './countdown';
import { OrbitRing } from './OrbitRing';
import { useOrbitInteraction } from './useOrbitInteraction';

export interface WeeklyOrbitWidgetProps {
  snapshot: RateLimitState | null;
  status: QuotaConnectionStatus;
  resetCredits: ResetCreditState | null;
  onExpandedChange: (expanded: boolean) => void;
  onDragStart: () => Promise<void>;
  onResetExpired: () => void;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

export function formatWeeklySyncStatus(
  status: QuotaConnectionStatus,
  hasWeeklyQuota: boolean,
  resetCredits: ResetCreditState | null,
): string {
  if (status === 'loginRequired' || resetCredits?.authRequired === true) {
    return '请登录 Codex';
  }
  if (status === 'starting') {
    return '正在连接额度服务';
  }
  if (status === 'offline') {
    return '额度服务已离线';
  }
  if (status === 'stale' || resetCredits?.stale === true) {
    return '显示上次同步数据';
  }
  if (!hasWeeklyQuota) {
    return '周额度暂不可用';
  }
  if (resetCredits === null || resetCredits.availableCount === null) {
    return '额度重置次数暂不可用';
  }
  return '额度实时同步';
}

export function WeeklyOrbitWidget({
  snapshot,
  status,
  resetCredits,
  onExpandedChange,
  onDragStart,
  onResetExpired,
}: WeeklyOrbitWidgetProps) {
  const interaction = useOrbitInteraction({
    startDragging: onDragStart,
    onExpandedChange,
    allowFlip: false,
    dragThresholdPx: 3,
  });
  const weekly = snapshot?.weekly ?? null;
  const weeklyPercent = weekly?.remainingPercent ?? null;
  const reducedMotion = useMemo(() => prefersReducedMotion(), []);
  const syncText = formatWeeklySyncStatus(status, weekly !== null, resetCredits);
  const isLive = syncText === '额度实时同步';

  useEffect(() => {
    if (weekly?.resetsAt !== null && weekly?.resetsAt !== undefined) {
      const delay = weekly.resetsAt * 1000 - Date.now();
      if (delay <= 0) {
        onResetExpired();
        return undefined;
      }
      const resetTimer = setTimeout(onResetExpired, delay);
      return () => clearTimeout(resetTimer);
    }
    return undefined;
  }, [onResetExpired, weekly?.resetsAt]);

  return (
    <div
      aria-label="Gpt Orbit Weekly"
      className="weekly-orbit-widget"
      data-expanded={String(interaction.state.expanded)}
      data-face={interaction.state.face}
      data-reduced-motion={String(reducedMotion)}
      onPointerDown={interaction.handlers.onPointerDown}
      onPointerEnter={interaction.handlers.onPointerEnter}
      onPointerLeave={interaction.handlers.onPointerLeave}
      onPointerMove={interaction.handlers.onPointerMove}
      onPointerUp={interaction.handlers.onPointerUp}
      role="group"
    >
      <div className="weekly-orbit-widget__body">
        <div
          aria-hidden={interaction.state.expanded}
          className="orbit-orb-layer"
          data-testid="weekly-orbit-collapsed"
        >
          <span data-testid="weekly-orbit-collapsed-text">本周</span>
          <span data-testid="weekly-orbit-collapsed-text">
            {weeklyPercent === null ? '--%' : `${Math.round(weeklyPercent)}%`}
          </span>
          <span data-testid="weekly-orbit-collapsed-text">
            {formatWeeklyCompactCountdown(weekly?.resetsAt ?? null)}
          </span>
        </div>

        <div
          aria-hidden={!interaction.state.expanded}
          className="orbit-capsule-layer"
          data-testid="weekly-orbit-expanded"
        >
          <OrbitRing percent={weeklyPercent} tone="violet" />
          <section aria-label="Codex 本周额度" className="weekly-orbit-details">
            <div className="orbit-face__title">Codex · 本周</div>
            <div className="orbit-face__subtitle">
              {formatWeeklyReset(weekly?.resetsAt ?? null)}
            </div>
            <div
              className="orbit-face__sync"
              data-stale={String(!isLive)}
              role="status"
            >
              <span className="orbit-live-dot" />
              <span>{syncText}</span>
            </div>
          </section>
        </div>
      </div>
      <ResetCreditBadge state={resetCredits} />
    </div>
  );
}
