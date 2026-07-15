import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RateLimitState } from '../quota/quota.types';
import { OrbitWidget } from './OrbitWidget';

const now = new Date(2026, 6, 12, 7, 12, 0).getTime();

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
    resetsAt: new Date(2026, 6, 13, 9, 30, 0).getTime() / 1000,
  },
  other: [],
  planType: 'plus',
  reachedType: null,
  fetchedAt: now / 1000,
  source: 'read',
  stale: false,
};

function renderWidget(
  props: Partial<React.ComponentProps<typeof OrbitWidget>> = {},
) {
  return render(
    <OrbitWidget
      snapshot={snapshot}
      status="live"
      onDragStart={vi.fn(async () => undefined)}
      onExpandedChange={vi.fn()}
      onResetExpired={vi.fn()}
      {...props}
    />,
  );
}

function widget() {
  return screen.getByRole('button', { name: 'Gpt Orbit' });
}

function expand() {
  fireEvent.pointerEnter(widget());
  act(() => {
    vi.advanceTimersByTime(150);
  });
}

describe('OrbitWidget', () => {
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

  it('renders the collapsed ornament with exactly the three compact text nodes', () => {
    renderWidget();

    const collapsed = screen.getByTestId('orbit-collapsed');
    const visibleTexts = within(collapsed)
      .getAllByTestId('orbit-collapsed-text')
      .map((node) => node.textContent);

    expect(visibleTexts).toEqual(['5H LEFT', '73%', '02:18']);
    expect(widget()).toHaveAttribute('data-expanded', 'false');
    expect(widget()).toHaveAttribute('data-face', 'front');
  });

  it('expands to the five-hour face on sustained hover without exposing weekly quota', () => {
    renderWidget();

    expand();

    expect(widget()).toHaveAttribute('data-expanded', 'true');
    expect(widget()).toHaveAttribute('data-face', 'front');
    expect(screen.getByText('Codex · 5 小时')).toBeInTheDocument();
    expect(screen.getByText('2 小时 18 分后重置')).toBeInTheDocument();
    expect(screen.getAllByText('额度实时同步').length).toBeGreaterThan(0);
    expect(screen.getByLabelText('Codex 本周额度')).toHaveAttribute('aria-hidden', 'true');
  });

  it('flips to the weekly face on click and flips back on the second click', () => {
    renderWidget();

    expand();
    expect(screen.getByLabelText('Codex 本周额度')).toHaveAttribute('aria-hidden', 'true');
    expect(screen.getByLabelText('Codex 本周额度')).not.toHaveAttribute('hidden');

    fireEvent.click(widget());

    expect(widget()).toHaveAttribute('data-face', 'back');
    expect(screen.getByLabelText('Codex 5 小时额度')).toHaveAttribute('aria-hidden', 'true');
    expect(screen.getByLabelText('Codex 5 小时额度')).not.toHaveAttribute('hidden');
    expect(screen.getByText('Codex · 本周')).toBeVisible();
    expect(screen.getByText('58%')).toBeInTheDocument();
    expect(screen.getByText('周一 09:30 重置')).toBeInTheDocument();

    fireEvent.click(widget());

    expect(widget()).toHaveAttribute('data-face', 'front');
    expect(screen.getByText('Codex · 5 小时')).toBeVisible();
  });

  it('shows a weekly unavailable fallback when weekly quota is missing', () => {
    renderWidget({ snapshot: { ...snapshot, weekly: null } });

    expand();
    fireEvent.click(widget());

    expect(screen.getByText('--%')).toBeInTheDocument();
    expect(screen.getByText('周额度暂不可用')).toBeInTheDocument();
  });

  it('shows stale sync copy and keeps data updates from changing the selected face', () => {
    const { rerender } = renderWidget({ status: 'stale' });

    expand();
    fireEvent.click(widget());
    rerender(
      <OrbitWidget
        snapshot={{
          ...snapshot,
          fiveHour: { ...snapshot.fiveHour!, remainingPercent: 64 },
        }}
        status="stale"
        onDragStart={vi.fn(async () => undefined)}
        onExpandedChange={vi.fn()}
        onResetExpired={vi.fn()}
      />,
    );

    expect(screen.getAllByText('显示上次同步额度').length).toBeGreaterThan(0);
    expect(widget()).toHaveAttribute('data-face', 'back');
  });

  it('marks reduced motion when the user prefers it', () => {
    vi.stubGlobal('matchMedia', (query: string) => ({
      matches: query === '(prefers-reduced-motion: reduce)',
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
      onchange: null,
    }));

    renderWidget();

    expect(widget()).toHaveAttribute('data-reduced-motion', 'true');
  });

  it('calls reset expiry when the five-hour window reaches its future reset time', () => {
    const onResetExpired = vi.fn();
    renderWidget({ onResetExpired });

    act(() => {
      vi.advanceTimersByTime(2 * 60 * 60 * 1000 + 18 * 60 * 1000 - 1);
    });

    expect(onResetExpired).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(1);
    });

    expect(onResetExpired).toHaveBeenCalledTimes(1);
  });
});
