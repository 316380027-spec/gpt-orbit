import { useEffect, useMemo } from 'react';
import type { QuotaConnectionStatus, RateLimitState } from '../quota/quota.types';
import { formatCompactCountdown, formatFiveHourReset, formatWeeklyReset } from './countdown';
import { OrbitRing } from './OrbitRing';
import { useOrbitInteraction } from './useOrbitInteraction';

export interface OrbitWidgetProps {
  snapshot: RateLimitState | null;
  status: QuotaConnectionStatus;
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

export function OrbitWidget({
  snapshot,
  status,
  onExpandedChange,
  onDragStart,
  onResetExpired,
}: OrbitWidgetProps) {
  const interaction = useOrbitInteraction({
    startDragging: onDragStart,
    onExpandedChange,
  });
  const fiveHour = snapshot?.fiveHour ?? null;
  const weekly = snapshot?.weekly ?? null;
  const fiveHourPercent = fiveHour?.remainingPercent ?? null;
  const weeklyPercent = weekly?.remainingPercent ?? null;
  const isBack = interaction.state.face === 'back';
  const reducedMotion = useMemo(() => prefersReducedMotion(), []);
  const syncText = status === 'stale' ? '显示上次同步额度' : '额度实时同步';

  useEffect(() => {
    if (fiveHour?.resetsAt !== null && fiveHour?.resetsAt !== undefined) {
      const delay = fiveHour.resetsAt * 1000 - Date.now();
      if (delay <= 0) {
        onResetExpired();
        return undefined;
      }
      const resetTimer = setTimeout(onResetExpired, delay);
      return () => {
        clearTimeout(resetTimer);
      };
    }
    return undefined;
  }, [fiveHour?.resetsAt, onResetExpired]);

  return (
    <div
      aria-label="Gpt Orbit"
      className="orbit-widget"
      data-expanded={String(interaction.state.expanded)}
      data-face={interaction.state.face}
      data-reduced-motion={String(reducedMotion)}
      onClick={interaction.handlers.onClick}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          interaction.handlers.onClick();
        }
      }}
      onPointerDown={interaction.handlers.onPointerDown}
      onPointerEnter={interaction.handlers.onPointerEnter}
      onPointerLeave={interaction.handlers.onPointerLeave}
      onPointerMove={interaction.handlers.onPointerMove}
      onPointerUp={interaction.handlers.onPointerUp}
      role="button"
      tabIndex={0}
    >
      <div className="orbit-orb-layer" data-testid="orbit-collapsed">
        <span data-testid="orbit-collapsed-text">5H LEFT</span>
        <span data-testid="orbit-collapsed-text">
          {fiveHourPercent === null ? '--%' : `${Math.round(fiveHourPercent)}%`}
        </span>
        <span data-testid="orbit-collapsed-text">
          {formatCompactCountdown(fiveHour?.resetsAt ?? null)}
        </span>
      </div>

      <div className="orbit-capsule-layer">
        <OrbitRing percent={isBack ? weeklyPercent : fiveHourPercent} tone={isBack ? 'violet' : 'ice'} />
        <div className="orbit-flipper">
          <section
            aria-hidden={isBack}
            aria-label="Codex 5 小时额度"
            className="orbit-face orbit-face--front"
          >
            <div className="orbit-face__title">Codex · 5 小时</div>
            <div className="orbit-face__subtitle">
              {formatFiveHourReset(fiveHour?.resetsAt ?? null)}
            </div>
            <div className="orbit-face__sync">
              <span className="orbit-live-dot" />
              <span>{syncText}</span>
            </div>
          </section>
          <section
            aria-hidden={!isBack}
            aria-label="Codex 本周额度"
            className="orbit-face orbit-face--back"
          >
            <div className="orbit-face__title">Codex · 本周</div>
            <div className="orbit-face__subtitle">{formatWeeklyReset(weekly?.resetsAt ?? null)}</div>
            <div className="orbit-face__sync">
              <span className="orbit-live-dot" />
              <span>{syncText}</span>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
