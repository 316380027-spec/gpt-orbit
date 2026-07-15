# Gpt Orbit Floating Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the production Windows 11 Gpt Orbit widget with a draggable 148px orb, a hover-expanded 245 × 112px five-hour capsule, and a click-flipped weekly quota face.

**Architecture:** React owns a deterministic interaction state machine, quota presentation, and CSS/SVG glass rendering. A focused window controller owns Tauri resize, ring-center anchoring, work-area clamping, native dragging, and persisted placement. Rust remains the source of normalized quota state and owns tray, lifecycle, process supervision, and safe retained-state commands.

**Tech Stack:** React 19, TypeScript 5.7, Zustand 5, CSS/SVG, Vitest 3, Testing Library, Tauri 2, Rust, Tokio, Windows 11 WebView2.

## Global Constraints

- Visible product name is `Gpt Orbit`; quota titles remain `Codex · 5 小时` and `Codex · 本周`.
- Visible orb body is exactly 148 × 148px; visible capsule body is exactly 245 × 112px.
- Hover confirmation is 150ms, expansion/collapse is approximately 350ms, flip is approximately 450ms, and pointer-leave grace is 200ms.
- Hover only expands the five-hour face. Weekly quota is reachable only by clicking the expanded capsule.
- Pointer movement greater than 6px starts native dragging and suppresses click/flip.
- Expansion anchors the original orb center to the left quota ring; edge clamping keeps the full capsule inside the current monitor work area.
- The frameless transparent window is always-on-top by default, skips the taskbar, does not request keyboard focus, and exposes a persisted tray toggle for always-on-top.
- Reduced motion replaces the 3D flip with a short crossfade and restrained size/opacity transitions.
- Frontend initial state uses the exact Tauri command `get_rate_limits`, then listens for safe backend events.
- Frontend displays backend `remainingPercent`, never raw `usedPercent` as remaining.
- Tokens, email, raw app-server responses, login URLs, and raw backend errors are never logged or persisted.
- The existing backend Task 4 lifecycle review must be approved before Task 1 is merged. Preserve its in-progress files and do not create a second quota supervisor.
- The older `2026-07-12-codex-orbit-frontend.md` hover-to-weekly behavior and fixed 320 × 220 Windows size are superseded by this plan.

## File Structure

- `src/features/quota/quota.types.ts`: serialized Rust quota contract and safe connection status.
- `src/features/quota/quota.store.ts`: retained quota snapshot and connection state; no interaction animation state.
- `src/features/quota/quota.bridge.ts`: initial command, event subscriptions, refresh command, and cleanup.
- `src/features/orbit/orbit.machine.ts`: pure interaction reducer and constants.
- `src/features/orbit/useOrbitInteraction.ts`: timer and pointer adapter around the reducer.
- `src/features/orbit/OrbitRing.tsx`: accessible SVG ring.
- `src/features/orbit/OrbitWidget.tsx`: three-state visual composition.
- `src/features/orbit/orbit-widget.css`: glass, morph, flip, crossfade, and typography.
- `src/features/window/window.geometry.ts`: pure logical-pixel anchor and work-area clamping.
- `src/features/window/window.controller.ts`: Tauri window adapter, drag, resize, and persisted position calls.
- `src-tauri/src/desktop/preferences.rs`: monitor/DPI-aware placement and always-on-top preference.
- `src-tauri/src/desktop/tray.rs`: one tray and exact menu actions.
- `src-tauri/src/desktop/mod.rs`: desktop commands and lifecycle wiring.
- `src/App.tsx`: bridge, widget, and window-controller composition only.

---

### Task 1: Product Rename, Quota Contract, and Retained Data Bridge

**Files:**
- Modify: `package.json`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Create: `src/features/quota/quota.types.ts`
- Create: `src/features/quota/quota.store.ts`
- Create: `src/features/quota/quota.bridge.ts`
- Test: `src/features/quota/quota.store.test.ts`
- Test: `src/features/quota/quota.bridge.test.ts`
- Modify: `src/App.test.tsx`

**Interfaces:**
- Consumes: Tauri command `get_rate_limits`, `refresh_rate_limits`; events `rate-limits://updated`, `rate-limits://status`, and `account://login-url`.
- Produces: `RateLimitState`, `QuotaConnectionStatus`, `useQuotaStore`, `startQuotaBridge(api)`, and `stop(): void`.

- [ ] **Step 1: Write failing contract and bridge tests**

Create tests that assert the store retains the current front/back-independent snapshot, rejects no fields, maps cached data to stale display, invokes `get_rate_limits` once before registering listeners, applies `rate-limits://updated`, and removes every listener during cleanup.

```ts
const snapshot: RateLimitState = {
  fiveHour: { kind: 'fiveHour', usedPercent: 27, remainingPercent: 73, windowDurationMins: 300, resetsAt: 1_800_000_000 },
  weekly: { kind: 'weekly', usedPercent: 42, remainingPercent: 58, windowDurationMins: 10080, resetsAt: 1_800_500_000 },
  other: [], planType: 'plus', reachedType: null,
  fetchedAt: 1_799_999_000, source: 'read', stale: false,
};

expect(useQuotaStore.getState().snapshot).toEqual(snapshot);
expect(api.invoke).toHaveBeenNthCalledWith(1, 'get_rate_limits');
expect(api.listen).toHaveBeenCalledWith('rate-limits://updated', expect.any(Function));
```

- [ ] **Step 2: Run tests and verify RED**

Run: `npm test -- --run src/features/quota/quota.store.test.ts src/features/quota/quota.bridge.test.ts src/App.test.tsx`

Expected: FAIL because quota modules do not exist and the app still renders `Codex Orbit`.

- [ ] **Step 3: Add the exact serialized contract**

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
export type QuotaConnectionStatus = 'starting' | 'live' | 'stale' | 'offline' | 'loginRequired';
```

- [ ] **Step 4: Implement the store and injected bridge**

```ts
import { create } from 'zustand';
import type { QuotaConnectionStatus, RateLimitState } from './quota.types';

interface QuotaStore {
  snapshot: RateLimitState | null;
  status: QuotaConnectionStatus;
  applySnapshot(snapshot: RateLimitState): void;
  setStatus(status: QuotaConnectionStatus): void;
}
export const useQuotaStore = create<QuotaStore>((set) => ({
  snapshot: null,
  status: 'starting',
  applySnapshot: (snapshot) => set({ snapshot, status: snapshot.stale ? 'stale' : 'live' }),
  setStatus: (status) => set({ status }),
}));
```

`startQuotaBridge` accepts `{ invoke, listen, open }`, awaits `invoke<RateLimitState | null>('get_rate_limits')`, applies a returned snapshot, subscribes to the three whitelisted events, opens only the controlled login URL payload, and returns an async cleanup that invokes all unlisten functions. It exposes `refresh(reason)` as `invoke('refresh_rate_limits', { reason })`.

- [ ] **Step 5: Rename visible product metadata**

Set `package.json.name` to `gpt-orbit`; set Tauri `productName` and main window `title` to `Gpt Orbit`; update the capability description without changing the stable bundle identifier in this migration. Update `App.test.tsx` to assert the accessible name `Gpt Orbit`.

- [ ] **Step 6: Verify and commit**

Run:

```powershell
npm test -- --run src/features/quota/quota.store.test.ts src/features/quota/quota.bridge.test.ts src/App.test.tsx
npm run typecheck
```

Expected: all focused tests pass and TypeScript reports no errors.

```powershell
git add package.json src-tauri/tauri.conf.json src-tauri/capabilities/default.json src/App.test.tsx src/features/quota
git commit -m "feat: add Gpt Orbit quota bridge"
```

---

### Task 2: Deterministic Orbit Interaction and Drag Threshold

**Files:**
- Create: `src/features/orbit/orbit.machine.ts`
- Create: `src/features/orbit/useOrbitInteraction.ts`
- Test: `src/features/orbit/orbit.machine.test.ts`
- Test: `src/features/orbit/useOrbitInteraction.test.tsx`

**Interfaces:**
- Produces: `OrbitState`, `OrbitEvent`, `reduceOrbit(state, event)`, `useOrbitInteraction({ startDragging, onExpandedChange })`.
- Consumes: a `startDragging(): Promise<void>` callback; it does not import Tauri directly.

- [ ] **Step 1: Write failing reducer and timer tests**

Cover: enter then 149ms stays collapsed; 150ms expands front; hover never selects weekly; expanded click selects back; second click selects front; leave then re-enter before 200ms cancels collapse; 200ms leave restores front and collapses; movement 6px remains click-eligible; movement greater than 6px calls drag once and prevents flip; quota updates do not change face; unmount clears timers.

```ts
expect(reduceOrbit(collapsedState, { type: 'hoverConfirmed' })).toEqual({ expanded: true, face: 'front' });
expect(reduceOrbit({ expanded: true, face: 'front' }, { type: 'click' })).toEqual({ expanded: true, face: 'back' });
expect(reduceOrbit({ expanded: true, face: 'back' }, { type: 'leaveExpired' })).toEqual({ expanded: false, face: 'front' });
```

- [ ] **Step 2: Run tests and verify RED**

Run: `npm test -- --run src/features/orbit/orbit.machine.test.ts src/features/orbit/useOrbitInteraction.test.tsx`

Expected: FAIL because reducer and hook do not exist.

- [ ] **Step 3: Implement the pure reducer**

```ts
export interface OrbitState { expanded: boolean; face: 'front' | 'back' }
export type OrbitEvent =
  | { type: 'hoverConfirmed' }
  | { type: 'click' }
  | { type: 'leaveExpired' };
export const collapsedState: OrbitState = { expanded: false, face: 'front' };
export function reduceOrbit(state: OrbitState, event: OrbitEvent): OrbitState {
  if (event.type === 'hoverConfirmed') return { expanded: true, face: 'front' };
  if (event.type === 'leaveExpired') return collapsedState;
  if (event.type === 'click' && state.expanded) {
    return { expanded: true, face: state.face === 'front' ? 'back' : 'front' };
  }
  return state;
}
```

- [ ] **Step 4: Implement timers and pointer arbitration**

The hook uses constants `HOVER_MS = 150`, `LEAVE_MS = 200`, and `DRAG_THRESHOLD_PX = 6`. On pointer down it stores client coordinates and resets `dragged=false`. Pointer move computes `Math.hypot(dx, dy)`; only values strictly greater than 6 call `startDragging()` and set `dragged=true`. Pointer up dispatches click only when expanded and not dragged. Starting drag clears both timers. Pointer leave schedules one `leaveExpired`; pointer enter cancels it and schedules one `hoverConfirmed` only while collapsed.

- [ ] **Step 5: Verify and commit**

Run: `npm test -- --run src/features/orbit/orbit.machine.test.ts src/features/orbit/useOrbitInteraction.test.tsx`

Expected: all reducer and fake-timer tests pass without pending timer warnings.

```powershell
git add src/features/orbit/orbit.machine.ts src/features/orbit/orbit.machine.test.ts src/features/orbit/useOrbitInteraction.ts src/features/orbit/useOrbitInteraction.test.tsx
git commit -m "feat: add Orbit interaction state machine"
```

---

### Task 3: Aurora Glass Orb, Morphing Capsule, and Weekly Flip

**Files:**
- Create: `src/features/orbit/OrbitRing.tsx`
- Create: `src/features/orbit/OrbitWidget.tsx`
- Create: `src/features/orbit/orbit-widget.css`
- Create: `src/features/orbit/countdown.ts`
- Test: `src/features/orbit/OrbitWidget.test.tsx`
- Test: `src/features/orbit/countdown.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/main.tsx`

**Interfaces:**
- Consumes: `RateLimitState`, `QuotaConnectionStatus`, `useOrbitInteraction`, and `WindowController` callbacks.
- Produces: `OrbitWidget({ snapshot, status, onExpandedChange, onDragStart, onResetExpired })`.

- [ ] **Step 1: Write failing visual behavior tests**

Render a 73% five-hour and 58% weekly snapshot. Assert collapsed output contains only `5H LEFT`, `73%`, and `02:18`; sustained hover exposes `Codex · 5 小时` but not `Codex · 本周`; click exposes `Codex · 本周` and `58%`; second click returns front; weekly missing displays `--%` and `周额度暂不可用`; stale status displays `显示上次同步额度`; reduced motion adds `data-reduced-motion="true"`.

- [ ] **Step 2: Run tests and verify RED**

Run: `npm test -- --run src/features/orbit/OrbitWidget.test.tsx src/features/orbit/countdown.test.ts`

Expected: FAIL because visual modules do not exist.

- [ ] **Step 3: Implement the ring and countdown formatters**

```tsx
export function OrbitRing({ percent, tone }: { percent: number | null; tone: 'ice' | 'violet' }) {
  const value = percent === null ? 0 : Math.max(0, Math.min(100, percent));
  return (
    <svg className={`orbit-ring orbit-ring--${tone}`} viewBox="0 0 72 72" role="progressbar"
      aria-label="剩余额度" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent ?? undefined}>
      <circle className="orbit-ring__track" cx="36" cy="36" r="30" />
      <circle className="orbit-ring__value" cx="36" cy="36" r="30" pathLength="100"
        strokeDasharray="100" strokeDashoffset={100 - value} />
    </svg>
  );
}
```

`formatCompactCountdown` returns `HH:MM` for the collapsed orb and never returns a negative value. `formatFiveHourReset` returns localized relative hours/minutes. `formatWeeklyReset` returns localized weekday and 24-hour time using the current Windows locale.

- [ ] **Step 4: Implement the three-face component**

Use one root button-like group with `data-expanded`, `data-face`, and `data-reduced-motion`. The collapsed layer contains exactly three text nodes. The capsule contains a persistent left ring anchor and a `.orbit-flipper` with front/back faces. Both faces use separate accessible labels; the inactive face uses `aria-hidden=true`. Data updates replace values without changing `data-face`.

- [ ] **Step 5: Implement production CSS**

Define body dimensions through CSS custom properties `--orb-size: 148px`, `--capsule-width: 245px`, and `--capsule-height: 112px`. Use a deep indigo translucent base, `backdrop-filter: blur(22px) saturate(135%)`, a one-pixel white-violet border, upper-left cyan radial gradient, lower-right violet radial gradient, and restrained shadow. Animate width, height, border-radius, layout opacity, and ring placement over 350ms. Set flipper perspective to 900px and transition `transform 450ms cubic-bezier(.22,.72,.2,1)`. Backface visibility is hidden and the back face is pre-rotated 180 degrees around X. Under `prefers-reduced-motion: reduce`, disable rotation and crossfade faces in 140ms.

- [ ] **Step 6: Compose App and verify**

`App.tsx` starts the bridge once, reads the Zustand snapshot/status, creates one window controller, and renders one `OrbitWidget`. `main.tsx` imports the global reset and orbit CSS. No React component reads raw app-server payloads.

Run:

```powershell
npm test -- --run src/features/orbit/OrbitWidget.test.tsx src/features/orbit/countdown.test.ts
npm run build
```

Expected: tests pass and Vite production build succeeds.

```powershell
git add src/App.tsx src/main.tsx src/features/orbit
git commit -m "feat: build the Gpt Orbit three-state visual"
```

---

### Task 4: Dynamic Native Window, Ring Anchor, Edge Avoidance, and Persistence

**Files:**
- Create: `src/features/window/window.geometry.ts`
- Create: `src/features/window/window.controller.ts`
- Test: `src/features/window/window.geometry.test.ts`
- Test: `src/features/window/window.controller.test.ts`
- Create: `src-tauri/src/desktop/mod.rs`
- Create: `src-tauri/src/desktop/preferences.rs`
- Test: `src-tauri/tests/widget_placement.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

**Interfaces:**
- Produces TypeScript `WindowController.setExpanded(expanded)`, `startDragging()`, and `setAlwaysOnTop(enabled)`.
- Produces Rust commands `load_desktop_preferences`, `save_widget_placement`, and `set_always_on_top`.
- Consumes Tauri `getCurrentWindow().setSize`, `setPosition`, `outerPosition`, `startDragging`, and `currentMonitor` in logical pixels.

- [ ] **Step 1: Write failing geometry tests**

Test normal expansion with an unchanged ring center, right-edge clamping, left/top/bottom clamping, 100/125/150/200% scale conversions, removed-monitor fallback, and collapse from the adjusted safe position.

```ts
expect(expandedRect({ ringCenter: { x: 1800, y: 100 }, workArea, margin: 12 })).toEqual({ x: 1651, y: 32, width: 269, height: 136 });
expect(clampRect({ x: 1900, y: 1050, width: 269, height: 136 }, workArea)).toEqual({ x: 1651, y: 944, width: 269, height: 136 });
```

- [ ] **Step 2: Run geometry tests and verify RED**

Run: `npm test -- --run src/features/window/window.geometry.test.ts src/features/window/window.controller.test.ts`

Expected: FAIL because geometry and controller modules do not exist.

- [ ] **Step 3: Implement pure geometry and Tauri adapter**

Use visible sizes 148 × 148 and 245 × 112 plus 12px transparent visual margin on every side, producing native canvases of 172 × 172 and 269 × 136. Compute ring center at 56px from the capsule body's left edge. Read current outer position and monitor work area, calculate the anchored expanded rect, clamp it, call `setPosition` before `setSize` when moving left/up and `setSize` before `setPosition` when moving right/down to avoid one-frame clipping. Serialize controller calls through one promise chain so rapid enter/leave cannot apply stale geometry. `startDragging` delegates to Tauri only after the interaction hook crosses 6px.

- [ ] **Step 4: Implement monitor/DPI preferences in Rust**

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPreferences {
    pub always_on_top: bool,
    pub monitor_name: Option<String>,
    pub offset_logical: Option<(f64, f64)>,
    pub scale_factor: Option<f64>,
}
```

Persist only desktop preferences under app data. Debounce move saves by 300ms. On restore, select the named monitor or primary monitor, apply the current scale factor, clamp the collapsed canvas inside the work area, then show the window. Do not persist the transient capsule size or weekly face.

- [ ] **Step 5: Configure the native window and least privilege**

Set visible startup size to the collapsed canvas, `transparent: true`, `decorations: false`, `resizable: false`, `shadow: false`, `alwaysOnTop: true`, `skipTaskbar: true`, and `visible: false`. Grant only `core:window:allow-start-dragging`, `core:window:allow-set-size`, `core:window:allow-set-position`, `core:window:allow-outer-position`, `core:window:allow-current-monitor`, and `core:window:allow-set-always-on-top` in addition to required existing permissions. Verify exact identifiers against generated desktop schema.

- [ ] **Step 6: Verify and commit**

Run:

```powershell
npm test -- --run src/features/window
cargo test --manifest-path src-tauri/Cargo.toml --test widget_placement
npm run tauri info
```

Expected: TypeScript and Rust placement tests pass and Tauri accepts the window/capability configuration.

```powershell
git add src/features/window src-tauri/src/desktop src-tauri/src/lib.rs src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/tests/widget_placement.rs
git commit -m "feat: add draggable DPI-safe Orbit window"
```

---

### Task 5: Tray, Always-on-Top, Lifecycle, and Integrated UI Contract

**Files:**
- Create: `src-tauri/src/desktop/tray.rs`
- Create: `src-tauri/src/desktop/lifecycle.rs`
- Test: `src-tauri/tests/tray_actions.rs`
- Test: `src-tauri/tests/desktop_lifecycle.rs`
- Modify: `src-tauri/src/desktop/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: `src/App.integration.test.tsx`

**Interfaces:**
- Tray IDs: `show-hide`, `refresh`, `always-on-top`, and `quit`.
- Events: `desktop://always-on-top` with `{ enabled: boolean }` and `desktop://visibility` with `{ visible: boolean }`.
- Refresh reasons: `Tray`, `WindowShown`, `Resume`, and `SessionUnlocked` use the existing supervised backend service.

- [ ] **Step 1: Write failing tray and lifecycle tests**

Assert exact menu mapping and labels `显示/隐藏`, `刷新额度`, `始终置顶`, and `退出`. Assert only one tray is constructed. Assert close hides rather than exits; show, resume, and unlock request a coalesced refresh; quit awaits backend shutdown before process exit. Frontend integration asserts initial query precedes event application and always-on-top changes do not reset the current Orbit face.

- [ ] **Step 2: Run tests and verify RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test tray_actions --test desktop_lifecycle
npm test -- --run src/App.integration.test.tsx
```

Expected: FAIL because desktop tray/lifecycle modules and integrated app test do not exist.

- [ ] **Step 3: Implement one native tray and lifecycle routing**

Construct the tray in Rust only. `show-hide` restores and clamps the window; `refresh` calls `refresh_now(RefreshReason::Tray)`; `always-on-top` persists the preference, calls the native window setter, updates the checked menu item, and emits the safe event; `quit` saves placement, shuts down the backend service, removes the tray, and exits. Window close hides the widget. Resume, session unlock, and show events request refresh without creating another app-server process.

- [ ] **Step 4: Verify and commit**

Run the commands from Step 2, then run `npm run build` and `cargo test --manifest-path src-tauri/Cargo.toml`.

Expected: all desktop, frontend, and backend tests pass; exactly one tray exists; shutdown leaves no managed child.

```powershell
git add src-tauri/src/desktop src-tauri/src/lib.rs src-tauri/tests/tray_actions.rs src-tauri/tests/desktop_lifecycle.rs src/App.integration.test.tsx
git commit -m "feat: integrate Gpt Orbit desktop lifecycle"
```

---

### Task 6: Visual, DPI, Fake-Server, and Installer Acceptance

**Files:**
- Create: `tests/fake-app-server/server.mjs`
- Create: `src-tauri/tests/gpt_orbit_integration.rs`
- Create: `scripts/acceptance/capture-widget.ps1`
- Create: `scripts/acceptance/verify-window.ps1`
- Create: `docs/acceptance/gpt-orbit-windows-matrix.md`
- Modify: `src-tauri/tauri.conf.json`

**Interfaces:**
- Fake scenarios: `live`, `sparse-weekly`, `weekly-missing`, `login-required`, `disconnect-once`, and `malformed-then-valid`.
- Installer target: unsigned current-user NSIS.

- [ ] **Step 1: Write failing process-level integration tests**

Start the real supervised backend with the fake executable. Assert retained command state, event sequence, sparse merge, missing weekly display contract, disconnect/reconnect, login URL safety, shutdown/reap, and a maximum of one managed child. Each test has a ten-second timeout and cleanup guard.

- [ ] **Step 2: Implement deterministic JSONL scenarios**

The fixture reads one JSON object per line, synchronizes on methods instead of sleeps, writes protocol only to stdout, writes fixed diagnostics only to stderr, and uses a marker file for one-time disconnect. It never reads real credentials or the user's Codex home.

- [ ] **Step 3: Run the automated release gate**

```powershell
npm run typecheck
npm test -- --run
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: every command exits zero with no timer, act, clippy, or leaked-process warning.

- [ ] **Step 4: Configure and build NSIS**

Set bundle targets to `nsis` and install mode to `currentUser`; keep signing and updater disabled. Build with `npm run tauri build -- --target x86_64-pc-windows-msvc` and verify one installer is produced under the NSIS bundle directory.

- [ ] **Step 5: Complete the Windows 11 matrix**

Record PASS/FAIL for the three-state sequence, exact body sizes, 150/350/450/200ms timing tolerances, 6px drag threshold, right-edge avoidance, free dragging, 100/125/150/200% DPI, mixed-DPI displays, removed-monitor recovery, always-on-top toggle, tray restore, reduced motion, dark/light wallpaper readability, login, cache/offline, reconnect, install, upgrade, and uninstall. Capture screenshots near the upper-right of a realistic dark desktop for collapsed, front capsule, and weekly back face.

- [ ] **Step 6: Commit acceptance assets**

```powershell
git add tests/fake-app-server src-tauri/tests/gpt_orbit_integration.rs scripts/acceptance docs/acceptance/gpt-orbit-windows-matrix.md src-tauri/tauri.conf.json
git commit -m "test: complete Gpt Orbit Windows acceptance"
```

## Completion Gate

- [ ] Backend Task 4 lifecycle review is Approved with a clean worktree.
- [ ] All six task review gates are Approved.
- [ ] `npm test -- --run`, `npm run build`, Rust fmt, clippy, and all-target tests pass.
- [ ] Hover never exposes weekly quota; click alone flips the expanded capsule.
- [ ] Dragging over 6px never flips and edge expansion remains visible.
- [ ] Reduced motion uses crossfade without 3D rotation.
- [ ] Initial cached state is available through `get_rate_limits` even if its startup event was missed.
- [ ] No sensitive value appears in logs, cache, screenshots, fixture diagnostics, or acceptance artifacts.
- [ ] Windows 11 DPI, multi-monitor, tray, placement restore, and NSIS acceptance matrix has no unexplained failure.
