# Codex Orbit 前端额度组件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建 React/TypeScript 额度卡片：默认显示 5 小时额度，稳定悬停切换周额度，并提供倒计时、键盘操作、减少动态效果与完整自动化测试。

**Architecture:** Rust 只向前端发送规范化 `RateLimitState`；Zustand 保存额度快照、连接状态和显示模式。React 将同一个 `QuotaWindow` 原子地绑定到标签、百分比、圆环和重置时间，悬停逻辑封装为独立 hook，倒计时只在本地每秒更新。

**Tech Stack:** React, TypeScript, Zustand, Vitest, Testing Library, CSS, Tauri 2 invoke API

## Global Constraints

- `resetsAt` 与 `fetchedAt` 均为 Unix 秒；前端不得猜测或转换为毫秒后再存入 store。
- 初始显示模式固定为 `fiveHour`。
- `pointerenter` 持续 150ms 后才可切到 `weekly`；`pointerleave` 持续 200ms 后恢复 `fiveHour`。
- 每次指针事件必须取消反方向旧定时器；定时器触发时再次检查指针位置和周额度是否存在。
- 周额度变为 `null` 时立即回退 `fiveHour`；缺失时显示“当前账户未返回周额度”。
- 键盘焦点内按 `W` 查看周额度，按 `Esc` 恢复 5 小时额度。
- 默认切换动画约 200ms；`prefers-reduced-motion: reduce` 时禁用过渡。
- 倒计时到期显示“等待刷新”，不得显示负时间，并且同一个 `resetsAt` 只触发一次刷新请求。
- React 不接收 App Server 原始响应、不处理凭证、不执行网络轮询。
- 所有行为按红—绿—重构顺序开发；每个任务独立测试并提交。

## 前置依赖

- 根目录已有可运行的 Tauri 2 + React + TypeScript 项目。
- 已安装 `zustand`、`vitest`、`jsdom`、`@testing-library/react`、`@testing-library/user-event`、`@testing-library/jest-dom`。
- `vite.config.ts` 的 Vitest 配置包含 `environment: 'jsdom'` 与 `setupFiles: ['./src/test/setup.ts']`。
- Rust 集成任务最终提供无参数 Tauri command `refresh_rate_limits`，成功返回 `void`。

---

### Task F1: 额度契约与 Zustand store

**Files:**
- Create: `src/features/quota/quota.types.ts`
- Create: `src/features/quota/quota.store.ts`
- Test: `src/features/quota/quota.store.test.ts`

**Interfaces:**
- Consumes: Rust 发布的规范化 `RateLimitState`。
- Produces: `QuotaWindow`, `RateLimitState`, `ConnectionStatus`, `DisplayMode`, `QuotaStoreState`, `useQuotaStore`, `selectVisibleWindow(state)`。

- [ ] **Step 1: 写失败测试**

创建 `src/features/quota/quota.store.test.ts`：

```ts
import { beforeEach, describe, expect, it } from 'vitest';
import { initialQuotaStoreState, selectVisibleWindow, useQuotaStore } from './quota.store';
import type { RateLimitState } from './quota.types';

const snapshot: RateLimitState = {
  fiveHour: { kind: 'fiveHour', usedPercent: 25, remainingPercent: 75, windowDurationMins: 300, resetsAt: 1_800_000_000 },
  weekly: { kind: 'weekly', usedPercent: 40, remainingPercent: 60, windowDurationMins: 10_080, resetsAt: 1_800_500_000 },
  other: [], planType: 'plus', reachedType: null,
  fetchedAt: 1_799_999_000, source: 'read', stale: false,
};

describe('quota store', () => {
  beforeEach(() => useQuotaStore.setState(initialQuotaStoreState, true));

  it('defaults to fiveHour', () => {
    expect(useQuotaStore.getState().displayMode).toBe('fiveHour');
  });

  it('keeps weekly mode across a fresh snapshot', () => {
    useQuotaStore.getState().applySnapshot(snapshot);
    useQuotaStore.getState().setDisplayMode('weekly');
    useQuotaStore.getState().applySnapshot({ ...snapshot, weekly: { ...snapshot.weekly!, remainingPercent: 42 } });
    expect(selectVisibleWindow(useQuotaStore.getState())?.remainingPercent).toBe(42);
  });

  it('falls back when weekly disappears', () => {
    useQuotaStore.getState().applySnapshot(snapshot);
    useQuotaStore.getState().setDisplayMode('weekly');
    useQuotaStore.getState().applySnapshot({ ...snapshot, weekly: null });
    expect(useQuotaStore.getState().displayMode).toBe('fiveHour');
  });
});
```

- [ ] **Step 2: 验证测试失败**

Run: `npm test -- --run src/features/quota/quota.store.test.ts`

Expected: FAIL，无法解析 `quota.store` 或 `quota.types`。

- [ ] **Step 3: 实现契约**

创建 `src/features/quota/quota.types.ts`：

```ts
export type QuotaWindowKind = 'fiveHour' | 'weekly' | 'other';
export interface QuotaWindow {
  kind: QuotaWindowKind;
  usedPercent: number;
  remainingPercent: number;
  windowDurationMins: number;
  resetsAt: number | null;
}
export type RateLimitSource = 'read' | 'updated' | 'cache';
export interface RateLimitState {
  fiveHour: QuotaWindow | null;
  weekly: QuotaWindow | null;
  other: QuotaWindow[];
  planType: string | null;
  reachedType: string | null;
  fetchedAt: number;
  source: RateLimitSource;
  stale: boolean;
}
export type ConnectionStatus = 'starting' | 'loginRequired' | 'refreshing' | 'live' | 'offline';
export type DisplayMode = 'fiveHour' | 'weekly';
```

- [ ] **Step 4: 实现 store**

创建 `src/features/quota/quota.store.ts`：

```ts
import { create } from 'zustand';
import type { ConnectionStatus, DisplayMode, QuotaWindow, RateLimitState } from './quota.types';

export interface QuotaStoreState extends RateLimitState {
  connectionStatus: ConnectionStatus;
  displayMode: DisplayMode;
  applySnapshot(snapshot: RateLimitState): void;
  setConnectionStatus(status: ConnectionStatus): void;
  setDisplayMode(mode: DisplayMode): void;
}

export const initialQuotaStoreState: QuotaStoreState = {
  fiveHour: null, weekly: null, other: [], planType: null, reachedType: null,
  fetchedAt: 0, source: 'cache', stale: true,
  connectionStatus: 'starting', displayMode: 'fiveHour',
  applySnapshot: () => undefined,
  setConnectionStatus: () => undefined,
  setDisplayMode: () => undefined,
};

export const useQuotaStore = create<QuotaStoreState>((set) => ({
  ...initialQuotaStoreState,
  applySnapshot: (snapshot) => set((state) => ({
    ...snapshot,
    displayMode: state.displayMode === 'weekly' && snapshot.weekly === null ? 'fiveHour' : state.displayMode,
  })),
  setConnectionStatus: (connectionStatus) => set({ connectionStatus }),
  setDisplayMode: (displayMode) => set((state) => ({
    displayMode: displayMode === 'weekly' && state.weekly === null ? 'fiveHour' : displayMode,
  })),
}));

export function selectVisibleWindow(state: QuotaStoreState): QuotaWindow | null {
  return state.displayMode === 'weekly' && state.weekly ? state.weekly : state.fiveHour;
}
```

- [ ] **Step 5: 验证并提交**

Run: `npm test -- --run src/features/quota/quota.store.test.ts`

Expected: 3 tests PASS。

```powershell
git add src/features/quota/quota.types.ts src/features/quota/quota.store.ts src/features/quota/quota.store.test.ts
git commit -m "feat: add frontend quota state contract"
```

---

### Task F2: 本地重置倒计时

**Files:**
- Create: `src/features/quota/countdown.ts`
- Create: `src/features/quota/ResetCountdown.tsx`
- Test: `src/features/quota/countdown.test.ts`
- Test: `src/features/quota/ResetCountdown.test.tsx`

**Interfaces:**
- Produces: `formatResetCountdown(resetsAt, nowSeconds): string`。
- Produces: `ResetCountdown({ resetsAt, onExpired })`；同一个 `resetsAt` 最多调用一次 `onExpired`。

- [ ] **Step 1: 写纯函数失败测试**

```ts
import { describe, expect, it } from 'vitest';
import { formatResetCountdown } from './countdown';

describe('formatResetCountdown', () => {
  it('formats positive time', () => expect(formatResetCountdown(200_000, 100_000)).toBe('1 天 3 小时 47 分钟后重置'));
  it('rounds partial minutes up', () => expect(formatResetCountdown(1_061, 1_000)).toBe('2 分钟后重置'));
  it('never shows negative time', () => expect(formatResetCountdown(999, 1_000)).toBe('等待刷新'));
  it('handles null', () => expect(formatResetCountdown(null, 1_000)).toBe('重置时间未知'));
});
```

Run: `npm test -- --run src/features/quota/countdown.test.ts`

Expected: FAIL，`formatResetCountdown` 不存在。

- [ ] **Step 2: 实现格式化函数**

```ts
export function formatResetCountdown(resetsAt: number | null, nowSeconds: number): string {
  if (resetsAt === null) return '重置时间未知';
  const seconds = resetsAt - nowSeconds;
  if (seconds <= 0) return '等待刷新';
  const totalMinutes = Math.ceil(seconds / 60);
  const days = Math.floor(totalMinutes / 1440);
  const hours = Math.floor((totalMinutes % 1440) / 60);
  const minutes = totalMinutes % 60;
  const parts: string[] = [];
  if (days) parts.push(`${days} 天`);
  if (hours) parts.push(`${hours} 小时`);
  if (minutes || !parts.length) parts.push(`${minutes} 分钟`);
  return `${parts.join(' ')}后重置`;
}
```

- [ ] **Step 3: 写组件失败测试**

```tsx
import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ResetCountdown } from './ResetCountdown';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2026-07-12T00:00:00Z')); });
afterEach(() => vi.useRealTimers());

it('requests one refresh after expiry', () => {
  const onExpired = vi.fn();
  const now = Math.floor(Date.now() / 1000);
  render(<ResetCountdown resetsAt={now + 1} onExpired={onExpired} />);
  act(() => vi.advanceTimersByTime(3000));
  expect(screen.getByText('等待刷新')).toBeInTheDocument();
  expect(onExpired).toHaveBeenCalledTimes(1);
});
```

Run: `npm test -- --run src/features/quota/ResetCountdown.test.tsx`

Expected: FAIL，`ResetCountdown` 不存在。

- [ ] **Step 4: 实现组件**

```tsx
import { useEffect, useRef, useState } from 'react';
import { formatResetCountdown } from './countdown';

export function ResetCountdown({ resetsAt, onExpired }: { resetsAt: number | null; onExpired(): void }) {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  const notified = useRef<number | null>(null);

  useEffect(() => {
    setNow(Math.floor(Date.now() / 1000));
    if (notified.current !== resetsAt) notified.current = null;
    const id = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(id);
  }, [resetsAt]);

  useEffect(() => {
    if (resetsAt !== null && resetsAt <= now && notified.current !== resetsAt) {
      notified.current = resetsAt;
      onExpired();
    }
  }, [now, onExpired, resetsAt]);

  return <time dateTime={resetsAt ? new Date(resetsAt * 1000).toISOString() : undefined}>{formatResetCountdown(resetsAt, now)}</time>;
}
```

- [ ] **Step 5: 验证并提交**

Run: `npm test -- --run src/features/quota/countdown.test.ts src/features/quota/ResetCountdown.test.tsx`

Expected: 5 tests PASS，无遗留定时器警告。

```powershell
git add src/features/quota/countdown.ts src/features/quota/countdown.test.ts src/features/quota/ResetCountdown.tsx src/features/quota/ResetCountdown.test.tsx
git commit -m "feat: add quota reset countdown"
```

---

### Task F3: 悬停与键盘状态机

**Files:**
- Create: `src/features/quota/useQuotaDisplayMode.ts`
- Test: `src/features/quota/useQuotaDisplayMode.test.tsx`

**Interfaces:**
- Consumes: `hasWeekly: boolean`, `useQuotaStore().setDisplayMode`。
- Produces: `onPointerEnter`, `onPointerLeave`, `onKeyDown`，可直接展开到卡片根元素。

- [ ] **Step 1: 写状态机失败测试**

```tsx
import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { initialQuotaStoreState, useQuotaStore } from './quota.store';
import { useQuotaDisplayMode } from './useQuotaDisplayMode';

describe('useQuotaDisplayMode', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useQuotaStore.setState({ ...initialQuotaStoreState, weekly: { kind: 'weekly', usedPercent: 40, remainingPercent: 60, windowDurationMins: 10080, resetsAt: 1_800_000_000 } }, true);
  });
  afterEach(() => vi.useRealTimers());

  it('switches at 150ms', () => {
    const { result } = renderHook(() => useQuotaDisplayMode(true));
    act(() => result.current.onPointerEnter());
    act(() => vi.advanceTimersByTime(149));
    expect(useQuotaStore.getState().displayMode).toBe('fiveHour');
    act(() => vi.advanceTimersByTime(1));
    expect(useQuotaStore.getState().displayMode).toBe('weekly');
  });

  it('restores at 200ms and cancels stale enter', () => {
    const { result } = renderHook(() => useQuotaDisplayMode(true));
    act(() => { result.current.onPointerEnter(); vi.advanceTimersByTime(100); result.current.onPointerLeave(); vi.advanceTimersByTime(500); });
    expect(useQuotaStore.getState().displayMode).toBe('fiveHour');
  });

  it('supports W and Escape', () => {
    const { result } = renderHook(() => useQuotaDisplayMode(true));
    act(() => result.current.onKeyDown({ key: 'w', preventDefault: vi.fn() } as never));
    expect(useQuotaStore.getState().displayMode).toBe('weekly');
    act(() => result.current.onKeyDown({ key: 'Escape', preventDefault: vi.fn() } as never));
    expect(useQuotaStore.getState().displayMode).toBe('fiveHour');
  });
});
```

Run: `npm test -- --run src/features/quota/useQuotaDisplayMode.test.tsx`

Expected: FAIL，hook 不存在。

- [ ] **Step 2: 实现状态机**

```ts
import { useEffect, useRef } from 'react';
import type { KeyboardEvent } from 'react';
import { useQuotaStore } from './quota.store';

export function useQuotaDisplayMode(hasWeekly: boolean) {
  const setMode = useQuotaStore((state) => state.setDisplayMode);
  const inside = useRef(false);
  const weeklyRef = useRef(hasWeekly);
  const enter = useRef<number | null>(null);
  const leave = useRef<number | null>(null);
  weeklyRef.current = hasWeekly;
  const clearEnter = () => { if (enter.current !== null) window.clearTimeout(enter.current); enter.current = null; };
  const clearLeave = () => { if (leave.current !== null) window.clearTimeout(leave.current); leave.current = null; };

  useEffect(() => { if (!hasWeekly) setMode('fiveHour'); }, [hasWeekly, setMode]);
  useEffect(() => () => { clearEnter(); clearLeave(); }, []);

  return {
    onPointerEnter: () => {
      inside.current = true; clearLeave(); clearEnter();
      enter.current = window.setTimeout(() => {
        enter.current = null;
        if (inside.current && weeklyRef.current) setMode('weekly');
      }, 150);
    },
    onPointerLeave: () => {
      inside.current = false; clearEnter(); clearLeave();
      leave.current = window.setTimeout(() => {
        leave.current = null;
        if (!inside.current) setMode('fiveHour');
      }, 200);
    },
    onKeyDown: (event: KeyboardEvent<HTMLElement>) => {
      if (event.key.toLowerCase() === 'w' && weeklyRef.current) { event.preventDefault(); clearEnter(); clearLeave(); setMode('weekly'); }
      if (event.key === 'Escape') { event.preventDefault(); clearEnter(); clearLeave(); setMode('fiveHour'); }
    },
  };
}
```

- [ ] **Step 3: 验证并提交**

Run: `npm test -- --run src/features/quota/useQuotaDisplayMode.test.tsx`

Expected: 3 tests PASS。

```powershell
git add src/features/quota/useQuotaDisplayMode.ts src/features/quota/useQuotaDisplayMode.test.tsx
git commit -m "feat: add quota hover state machine"
```

---

### Task F4: 毛玻璃卡片与可访问额度环

**Files:**
- Create: `src/features/quota/QuotaRing.tsx`
- Create: `src/features/quota/QuotaWidget.tsx`
- Create: `src/features/quota/quota-widget.css`
- Test: `src/features/quota/QuotaWidget.test.tsx`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `selectVisibleWindow`, `useQuotaDisplayMode`, `ResetCountdown`。
- Produces: `QuotaWidget({ onRefreshRequested })`。

- [ ] **Step 1: 写组件失败测试**

```tsx
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { initialQuotaStoreState, useQuotaStore } from './quota.store';
import { QuotaWidget } from './QuotaWidget';

beforeEach(() => {
  vi.useFakeTimers();
  useQuotaStore.setState({
    ...initialQuotaStoreState,
    fiveHour: { kind: 'fiveHour', usedPercent: 25, remainingPercent: 75, windowDurationMins: 300, resetsAt: 1_800_000_000 },
    weekly: { kind: 'weekly', usedPercent: 40, remainingPercent: 60, windowDurationMins: 10080, resetsAt: 1_800_500_000 },
    connectionStatus: 'live', source: 'read', stale: false,
  }, true);
});
afterEach(() => vi.useRealTimers());

it('shows fiveHour then switches all weekly fields together', () => {
  render(<QuotaWidget onRefreshRequested={() => undefined} />);
  expect(screen.getByText('5 小时')).toBeInTheDocument();
  expect(screen.getByText('75%')).toBeInTheDocument();
  fireEvent.pointerEnter(screen.getByRole('group'));
  act(() => vi.advanceTimersByTime(150));
  expect(screen.getByText('本周')).toBeInTheDocument();
  expect(screen.getByText('60%')).toBeInTheDocument();
  expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '60');
});

it('explains missing weekly data', () => {
  useQuotaStore.setState({ weekly: null });
  render(<QuotaWidget onRefreshRequested={() => undefined} />);
  expect(screen.getByText('当前账户未返回周额度')).toBeInTheDocument();
});
```

Run: `npm test -- --run src/features/quota/QuotaWidget.test.tsx`

Expected: FAIL，组件不存在。

- [ ] **Step 2: 实现额度环**

```tsx
export function QuotaRing({ remainingPercent, exhausted }: { remainingPercent: number; exhausted: boolean }) {
  const radius = 52;
  const length = 2 * Math.PI * radius;
  return (
    <svg className={exhausted ? 'quota-ring quota-ring--exhausted' : 'quota-ring'} viewBox="0 0 120 120" role="progressbar" aria-label="剩余额度" aria-valuemin={0} aria-valuemax={100} aria-valuenow={remainingPercent}>
      <circle className="quota-ring__track" cx="60" cy="60" r={radius} />
      <circle className="quota-ring__value" cx="60" cy="60" r={radius} strokeDasharray={length} strokeDashoffset={length * (1 - remainingPercent / 100)} />
    </svg>
  );
}
```

- [ ] **Step 3: 实现卡片**

`QuotaWidget.tsx` 必须：从 store 只调用一次 `selectVisibleWindow(store)`；同一 `visibleWindow` 同时生成标签、百分比、环和 `ResetCountdown`；根元素使用 `role="group"`、`tabIndex={0}` 并展开 `useQuotaDisplayMode(store.weekly !== null)`；`aria-label` 格式为 `5 小时，剩余 75%，实时数据`；缓存或 stale 显示黄色“历史数据”，实时显示绿色，刷新显示蓝色，无数据离线显示灰色；到期调用 `onRefreshRequested`。

`src/App.tsx` 使用：

```tsx
import { invoke } from '@tauri-apps/api/core';
import { QuotaWidget } from './features/quota/QuotaWidget';

export default function App() {
  return <QuotaWidget onRefreshRequested={() => { void invoke('refresh_rate_limits'); }} />;
}
```

- [ ] **Step 4: 添加最小 CSS**

`quota-widget.css` 至少包含固定 240px 毛玻璃卡片、圆环、状态点、约 200ms 的 opacity/transform/stroke 过渡，并加入：

```css
@media (prefers-reduced-motion: reduce) {
  .quota-widget__content,
  .quota-ring__value { transition: none; }
}
```

- [ ] **Step 5: 验证并提交**

Run:

```powershell
npm test -- --run src/features/quota/QuotaWidget.test.tsx
npm run build
```

Expected: 2 tests PASS；TypeScript 与 Vite production build 成功。

```powershell
git add src/App.tsx src/features/quota/QuotaRing.tsx src/features/quota/QuotaWidget.tsx src/features/quota/QuotaWidget.test.tsx src/features/quota/quota-widget.css
git commit -m "feat: build interactive quota widget"
```

---

## 完成门

- [ ] `npm test -- --run` 全部 PASS，无 `act(...)` 或遗留计时器警告。
- [ ] `npm run build` 成功，无 TypeScript 错误。
- [ ] 默认显示 5 小时；150ms 移入切周额度；200ms 移出恢复。
- [ ] 快速 enter/leave 连续 20 次无闪烁或旧定时器反向覆盖。
- [ ] 悬停期间收到新周快照，保持周模式并立即展示新值。
- [ ] 周额度变为 `null` 后立即回退 5 小时并显示缺失说明。
- [ ] `W` 与 `Esc` 可操作；Narrator 能读出窗口、剩余比例和数据状态。
- [ ] Windows 减少动态效果开启时无过渡。
- [ ] 倒计时到期显示“等待刷新”，单个 reset 时间仅请求一次刷新。
- [ ] 100%、125%、150% DPI 下标签、圆环、百分比和状态点无裁切。
