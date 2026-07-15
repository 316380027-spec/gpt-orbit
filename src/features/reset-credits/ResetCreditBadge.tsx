import type { ResetCreditState } from './reset-credits.types';

interface ResetCreditBadgeProps {
  state: ResetCreditState | null;
}

export function formatResetCreditCount(count: number | null): string {
  if (count === null) return '—';
  return count > 99 ? '99+' : String(count);
}

export function ResetCreditBadge({ state }: ResetCreditBadgeProps) {
  const count = state?.availableCount ?? null;

  return (
    <aside
      aria-label={
        count === null ? '额度重置次数暂不可用' : `剩余 ${count} 次额度重置`
      }
      className="reset-credit-badge"
      data-empty={String(count === 0)}
      data-stale={String(state?.stale ?? false)}
    >
      <span className="reset-credit-badge__value">{formatResetCreditCount(count)}</span>
      <span className="reset-credit-badge__unit">次</span>
    </aside>
  );
}
