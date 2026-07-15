import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RateLimitState } from '../quota/quota.types';
import type { ResetCreditState } from '../reset-credits/reset-credits.types';
import { formatWeeklySyncStatus, WeeklyOrbitWidget } from './WeeklyOrbitWidget';

const orbitWidgetCss = readFileSync(
  join(process.cwd(), 'src/features/orbit/orbit-widget.css'),
  'utf8',
);

const now = new Date(2026, 6, 12, 7, 12, 0).getTime();
const resetCredits: ResetCreditState = {
  availableCount: 3,
  fetchedAt: now / 1000,
  stale: false,
  authRequired: false,
};
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
    resetsAt: now / 1000 + 26 * 60 * 60 + 18 * 60,
  },
  other: [],
  planType: 'plus',
  reachedType: null,
  fetchedAt: now / 1000,
  source: 'read',
  stale: false,
};

function renderWidget(props: Partial<React.ComponentProps<typeof WeeklyOrbitWidget>> = {}) {
  return render(
    <WeeklyOrbitWidget
      snapshot={snapshot}
      status="live"
      resetCredits={resetCredits}
      onDragStart={vi.fn(async () => undefined)}
      onExpandedChange={vi.fn()}
      onResetExpired={vi.fn()}
      {...props}
    />,
  );
}

function widget() {
  return screen.getByRole('group', { name: 'Gpt Orbit Weekly' });
}

function expand() {
  fireEvent.pointerEnter(widget());
  act(() => {
    vi.advanceTimersByTime(150);
  });
}

function pointer(
  element: HTMLElement,
  type: 'pointerdown' | 'pointermove' | 'pointerup',
  init: MouseEventInit = {},
) {
  fireEvent(element, new MouseEvent(type, { bubbles: true, ...init }));
}

describe('WeeklyOrbitWidget', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(now);
  });

  afterEach(() => {
    cleanup();
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('uses a native-size Quiet Prism layout without transform scaling', () => {
    const widgetRule = orbitWidgetCss.match(/\.weekly-orbit-widget\s*\{[^}]+\}/)?.[0] ?? '';
    const bodyRule = orbitWidgetCss.match(/\.weekly-orbit-widget__body\s*\{[^}]+\}/)?.[0] ?? '';

    expect(widgetRule).toContain('width: 92px;');
    expect(widgetRule).toContain('height: 74px;');
    expect(widgetRule).toContain('left: 6px;');
    expect(widgetRule).toContain('top: 6px;');
    expect(widgetRule).not.toMatch(/transform:\s*scale/);
    expect(bodyRule).toContain('width: 74px;');
    expect(bodyRule).toContain('height: 74px;');
    expect(orbitWidgetCss).toContain('width: 141px;');
    expect(orbitWidgetCss).toContain('width: 123px;');
    expect(orbitWidgetCss).toContain('padding: 6px 7px 6px 8px;');
  });

  it('renders only weekly quota and its persistent reset badge while collapsed', () => {
    renderWidget();

    const collapsed = screen.getByTestId('weekly-orbit-collapsed');
    expect(within(collapsed).getAllByTestId('weekly-orbit-collapsed-text').map((node) => node.textContent))
      .toEqual(['本周', '58%', '1D 03H']);
    expect(screen.getByLabelText('剩余 3 次额度重置')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Gpt Orbit Weekly' })).not.toBeInTheDocument();
    expect(widget()).not.toHaveAttribute('tabindex');
    expect(widget()).toHaveAttribute('data-expanded', 'false');
    expect(widget()).toHaveAttribute('data-face', 'front');
    expect(collapsed).toHaveAttribute('aria-hidden', 'false');
    expect(screen.getByTestId('weekly-orbit-expanded')).toHaveAttribute('aria-hidden', 'true');
    expect(screen.queryByRole('progressbar', { name: '剩余额度' })).not.toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    expect(screen.queryByText(/5H/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/5 小时/)).not.toBeInTheDocument();
    expect(screen.queryByText('73%')).not.toBeInTheDocument();
    expect(document.querySelector('.orbit-face--back')).not.toBeInTheDocument();
    expect(document.querySelector('.orbit-flipper')).not.toBeInTheDocument();
  });

  it('expands after 150ms, keeps the badge mounted, and collapses after 200ms', () => {
    renderWidget();
    const badge = screen.getByLabelText('剩余 3 次额度重置');

    fireEvent.pointerEnter(widget());
    act(() => vi.advanceTimersByTime(149));
    expect(widget()).toHaveAttribute('data-expanded', 'false');
    act(() => vi.advanceTimersByTime(1));

    expect(widget()).toHaveAttribute('data-expanded', 'true');
    expect(screen.getByTestId('weekly-orbit-collapsed')).toHaveAttribute('aria-hidden', 'true');
    expect(screen.getByTestId('weekly-orbit-expanded')).toHaveAttribute('aria-hidden', 'false');
    expect(screen.getByText('Codex · 本周')).toBeInTheDocument();
    expect(screen.getByRole('progressbar', { name: '剩余额度' })).toHaveAttribute(
      'aria-valuenow',
      '58',
    );
    expect(screen.getByRole('status')).toHaveTextContent('额度实时同步');
    expect(screen.getByLabelText('剩余 3 次额度重置')).toBe(badge);

    fireEvent.pointerLeave(widget());
    act(() => vi.advanceTimersByTime(199));
    expect(widget()).toHaveAttribute('data-expanded', 'true');
    act(() => vi.advanceTimersByTime(1));
    expect(widget()).toHaveAttribute('data-expanded', 'false');
    expect(screen.getByLabelText('剩余 3 次额度重置')).toBe(badge);
  });

  it('never flips on click or pointer-up', () => {
    renderWidget();
    expand();

    fireEvent.click(widget());
    pointer(widget(), 'pointerdown', { clientX: 0, clientY: 0 });
    pointer(widget(), 'pointerup');

    expect(widget()).toHaveAttribute('data-face', 'front');
    expect(screen.queryByText(/5 小时/)).not.toBeInTheDocument();
  });

  it('does not expose or consume meaningless keyboard button actions', () => {
    const onExpandedChange = vi.fn();
    renderWidget({ onExpandedChange });
    const enter = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      key: 'Enter',
    });
    const space = new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      key: ' ',
    });

    widget().dispatchEvent(enter);
    widget().dispatchEvent(space);

    expect(enter.defaultPrevented).toBe(false);
    expect(space.defaultPrevented).toBe(false);
    expect(onExpandedChange).not.toHaveBeenCalled();
    expect(widget()).toHaveAttribute('data-expanded', 'false');
  });

  it('starts native dragging above the 3px threshold', () => {
    const onDragStart = vi.fn(async () => undefined);
    renderWidget({ onDragStart });
    expand();

    pointer(widget(), 'pointerdown', { clientX: 0, clientY: 0 });
    pointer(widget(), 'pointermove', { clientX: 3.1, clientY: 0 });
    pointer(widget(), 'pointermove', { clientX: 4, clientY: 0 });
    pointer(widget(), 'pointerup');
    fireEvent.click(widget());

    expect(onDragStart).toHaveBeenCalledTimes(1);
    expect(widget()).toHaveAttribute('data-face', 'front');
  });

  it('renders missing weekly quota and stale reset credits without five-hour fallback', () => {
    renderWidget({
      snapshot: { ...snapshot, weekly: null },
      resetCredits: {
        availableCount: 0,
        fetchedAt: 1,
        stale: true,
        authRequired: false,
      },
    });

    expect(within(screen.getByTestId('weekly-orbit-collapsed')).getByText('--%')).toBeInTheDocument();
    expect(screen.getByText('--D --H')).toBeInTheDocument();
    expect(screen.getByLabelText('剩余 0 次额度重置')).toHaveAttribute('data-stale', 'true');
    expand();
    expect(screen.getByText('周额度暂不可用')).toBeInTheDocument();
    expect(screen.getByText('显示上次同步数据')).toBeInTheDocument();
    expect(screen.queryByText('73%')).not.toBeInTheDocument();
  });

  it('renders unknown and 99+ reset credit states in the weekly surface', () => {
    const { rerender } = renderWidget({ resetCredits: null });
    expect(screen.getByLabelText('额度重置次数暂不可用')).toHaveTextContent('—次');

    rerender(
      <WeeklyOrbitWidget
        snapshot={snapshot}
        status="live"
        resetCredits={{
          availableCount: 120,
          fetchedAt: 1,
          stale: false,
          authRequired: false,
        }}
        onDragStart={vi.fn(async () => undefined)}
        onExpandedChange={vi.fn()}
        onResetExpired={vi.fn()}
      />,
    );
    expect(screen.getByLabelText('剩余 120 次额度重置')).toHaveTextContent('99+次');
  });

  it.each([
    ['starting', true, resetCredits, '正在连接额度服务'],
    ['offline', true, resetCredits, '额度服务已离线'],
    ['loginRequired', true, resetCredits, '请登录 Codex'],
    ['live', true, { ...resetCredits, stale: true, authRequired: true }, '请登录 Codex'],
    ['stale', true, resetCredits, '显示上次同步数据'],
    ['live', true, null, '额度重置次数暂不可用'],
    ['live', true, { ...resetCredits, availableCount: null }, '额度重置次数暂不可用'],
    ['live', true, { ...resetCredits, stale: true }, '显示上次同步数据'],
    ['live', false, resetCredits, '周额度暂不可用'],
    ['live', true, resetCredits, '额度实时同步'],
  ] as const)(
    'maps %s weekly=%s reset-credit state to an accurate sync status',
    (status, hasWeeklyQuota, credits, expected) => {
      expect(formatWeeklySyncStatus(status, hasWeeklyQuota, credits)).toBe(expected);
      if (expected !== '额度实时同步') {
        expect(formatWeeklySyncStatus(status, hasWeeklyQuota, credits)).not.toBe('额度实时同步');
      }
    },
  );
});
