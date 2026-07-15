import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { formatResetCreditCount, ResetCreditBadge } from './ResetCreditBadge';

describe('ResetCreditBadge', () => {
  afterEach(cleanup);

  it('formats unknown, exact, and capped reset credit counts', () => {
    expect(formatResetCreditCount(null)).toBe('—');
    expect(formatResetCreditCount(0)).toBe('0');
    expect(formatResetCreditCount(99)).toBe('99');
    expect(formatResetCreditCount(100)).toBe('99+');
  });

  it('renders the available count with its fixed accessible label', () => {
    render(
      <ResetCreditBadge
        state={{
          availableCount: 3,
          fetchedAt: 1,
          stale: false,
          authRequired: false,
        }}
      />,
    );

    expect(screen.getByLabelText('剩余 3 次额度重置')).toHaveTextContent('3次');
  });

  it('renders unknown, zero, stale, and 99+ states accurately', () => {
    const { rerender } = render(<ResetCreditBadge state={null} />);
    const unknown = screen.getByLabelText('额度重置次数暂不可用');
    expect(unknown).toHaveTextContent('—次');
    expect(unknown).toHaveAttribute('data-empty', 'false');
    expect(unknown).toHaveAttribute('data-stale', 'false');

    rerender(
      <ResetCreditBadge
        state={{
          availableCount: 0,
          fetchedAt: 1,
          stale: false,
          authRequired: false,
        }}
      />,
    );
    expect(screen.getByLabelText('剩余 0 次额度重置')).toHaveTextContent('0次');
    expect(screen.getByLabelText('剩余 0 次额度重置')).toHaveAttribute('data-empty', 'true');

    rerender(
      <ResetCreditBadge
        state={{
          availableCount: 120,
          fetchedAt: 1,
          stale: true,
          authRequired: false,
        }}
      />,
    );
    const capped = screen.getByLabelText('剩余 120 次额度重置');
    expect(capped).toHaveTextContent('99+次');
    expect(capped).toHaveAttribute('data-stale', 'true');
  });
});
