# Gpt Orbit Weekly Reset Credits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a separately installable `Gpt Orbit Weekly` Windows widget that shows only the Codex weekly quota and an always-visible red glass badge containing the real number of available OpenAI rate-limit reset credits.

**Architecture:** Keep the proven Codex `app-server` path for weekly quota data. Add an isolated Rust reset-credit client/service that reads the local Codex login only in the weekly build, calls the read-only ChatGPT reset-credit endpoint, caches a normalized count, and emits safe Tauri events. Build both products from one source tree using a Vite variant and a Tauri configuration overlay.

**Tech Stack:** Tauri 2, Rust 2021, Tokio, Reqwest with rustls, React 19, TypeScript 5.7, Zustand 5, Vite 6, Vitest 3, Testing Library, NSIS.

## Global Constraints

- Preserve the current `Gpt Orbit` behavior and installer identity exactly.
- The new product identity is `Gpt Orbit Weekly`, main binary `gpt-orbit-weekly`, identifier `com.codex-orbit.weekly`.
- The weekly build must not render `5H`, `5 小时`, or any five-hour quota value in visible or accessible UI.
- Weekly quota continues to come from the existing local `app-server` bridge; never substitute five-hour data when weekly data is missing.
- Reset credits are read-only. Do not implement or call `/consume`.
- Only the weekly build may read Codex auth or call `https://chatgpt.com/backend-api/wham/rate-limit-reset-credits`.
- Access tokens and account identifiers never enter the WebView, cache, test artifacts, screenshots, or logs.
- Reject redirects to another host, use a 10-second timeout, and cap response bodies at 64 KiB.
- Unknown reset count is `null`/`—`; only an explicit valid zero may render `0`.
- Refresh reset credits at startup, tray refresh, window show, resume, unlock, and every 300 seconds.
- Cached reset-credit state is always marked stale; a failed refresh may retain a previous value but may not fabricate one.
- Weekly visible geometry is 148×148 plus a 46×46 badge with 10px overlap when collapsed, and 245×112 plus the same badge when expanded. Native canvases are 208×172 and 305×136.
- Hover delay is 150ms, expansion is approximately 350ms, leave grace is 200ms, and reduced-motion transitions are approximately 140ms.
- The badge is display-only and remains visible in collapsed and expanded states.
- All tests use temporary Codex homes and local fake servers; no automated test may access real credentials or the live ChatGPT endpoint.

## File structure

- `src/appVariant.ts`: frontend build-variant contract.
- `src/features/reset-credits/`: safe frontend types, store, event bridge, and red badge.
- `src/features/orbit/WeeklyOrbitWidget.tsx`: weekly-only visual/interaction component.
- `src-tauri/src/backend/reset_credits.rs`: auth loader, response normalization, HTTPS client, and cache.
- `src-tauri/src/backend/reset_credit_service.rs`: refresh scheduling, stale fallback, events, and shutdown.
- `src-tauri/src/desktop/app_variant.rs`: Rust variant and geometry selection from Tauri identifier.
- `src-tauri/tauri.weekly.conf.json`: full weekly build overlay.
- Existing quota, tray, lifecycle, window-controller, acceptance, and package files are changed only where the weekly variant needs an explicit hook.

---

### Task 1: Freeze the standard/weekly build-flavor contract

**Files:**
- Create: `src/appVariant.ts`
- Create: `src/appVariant.test.ts`
- Create: `src/vite-env.d.ts`
- Create: `src-tauri/tauri.weekly.conf.json`
- Create: `src-tauri/src/desktop/app_variant.rs`
- Modify: `vite.config.ts`
- Modify: `src-tauri/src/desktop/mod.rs`
- Modify: `src-tauri/tests/gpt_orbit_integration.rs`
- Modify: `package.json`

**Interfaces:**
- Produces `type AppVariant = 'standard' | 'weekly'`.
- Produces `resolveAppVariant(value: unknown): AppVariant` for deterministic tests.
- Maps Vite `mode === 'weekly'` to the Weekly entry through the `#app-entry` alias; other modes use the Standard entry.
- Produces Rust `AppVariant::from_identifier(&str)` and `WidgetCanvas` constants.
- Produces scripts `build:weekly`, `dev:weekly`, and `tauri:build:weekly`.

- [ ] **Step 1: Write failing frontend and Rust contract tests**

```ts
import { describe, expect, it } from 'vitest';
import { resolveAppVariant } from './appVariant';

describe('resolveAppVariant', () => {
  it('selects weekly only for the frozen weekly token', () => {
    expect(resolveAppVariant('weekly')).toBe('weekly');
    expect(resolveAppVariant('standard')).toBe('standard');
    expect(resolveAppVariant(undefined)).toBe('standard');
  });
});
```

Add a Rust integration test that parses `tauri.conf.json` and `tauri.weekly.conf.json`, asserts distinct `productName`, `identifier`, and `mainBinaryName`, and asserts the weekly window repeats `transparent=true`, `decorations=false`, `resizable=false`, `shadow=false`, `alwaysOnTop=true`, `skipTaskbar=true`, and `visible=false` with width/minWidth `208`, height/maxHeight `172`, minHeight `136`, and maxWidth `305`.

- [ ] **Step 2: Run the focused tests and observe the missing-contract failures**

Run: `npm.cmd test -- --configLoader runner --run src/appVariant.test.ts`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test gpt_orbit_integration weekly_build_has_isolated_identity_and_full_window_security`

Expected: both fail because the variant module and weekly Tauri overlay do not exist.

- [ ] **Step 3: Implement the build flavor and full Tauri overlay**

```ts
export type AppVariant = 'standard' | 'weekly';

export function resolveAppVariant(value: unknown): AppVariant {
  return value === 'weekly' ? 'weekly' : 'standard';
}
```

In `vite.config.ts`, select the production entry from the Vite mode instead of an `.env.*` file:

```ts
resolve: {
  alias: {
    '#app-entry': fileURLToPath(new URL(
      mode === 'weekly' ? './src/app/WeeklyApp.tsx' : './src/app/StandardApp.tsx',
      import.meta.url,
    )),
  },
},
```

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppVariant { Standard, Weekly }

impl AppVariant {
    pub fn from_identifier(identifier: &str) -> Self {
        if identifier == "com.codex-orbit.weekly" { Self::Weekly } else { Self::Standard }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetCanvas { pub collapsed_width: f64, pub collapsed_height: f64, pub expanded_width: f64, pub expanded_height: f64 }

pub const STANDARD_CANVAS: WidgetCanvas = WidgetCanvas { collapsed_width: 172.0, collapsed_height: 172.0, expanded_width: 269.0, expanded_height: 136.0 };
pub const WEEKLY_CANVAS: WidgetCanvas = WidgetCanvas { collapsed_width: 208.0, collapsed_height: 172.0, expanded_width: 305.0, expanded_height: 136.0 };
```

The Vite mode alias is the only frontend build-flavor selector; do not create or commit `.env.weekly`. The overlay contains the frozen weekly identity, `beforeBuildCommand: "npm run build:weekly"`, and a complete `app.windows` array that preserves every standard safety flag while applying weekly geometry.

- [ ] **Step 4: Verify flavor builds and contracts**

Run: `npm.cmd test -- --configLoader runner --run src/appVariant.test.ts`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test gpt_orbit_integration weekly_build_has_isolated_identity_and_full_window_security`

Run: `npm.cmd run build:weekly`

Expected: focused tests pass and Vite produces a weekly-mode frontend bundle.

- [ ] **Step 5: Commit Task 1**

```powershell
git add package.json vite.config.ts src/appVariant.ts src/appVariant.test.ts src/vite-env.d.ts src-tauri/tauri.weekly.conf.json src-tauri/src/desktop/app_variant.rs src-tauri/src/desktop/mod.rs src-tauri/tests/gpt_orbit_integration.rs
git commit -m "feat: add weekly widget build flavor"
```

---

### Task 2: Implement the secure reset-credit domain, client, and cache

**Files:**
- Create: `src-tauri/src/backend/reset_credits.rs`
- Modify: `src-tauri/src/backend/mod.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

**Interfaces:**
- Produces `ResetCreditState { available_count: Option<u32>, fetched_at: i64, stale: bool }` serialized as camelCase.
- Produces `normalize_reset_credit_response(Value, now: i64) -> BackendResult<ResetCreditState>`.
- Produces `ResetCreditAuth::load(&Path) -> BackendResult<ResetCreditAuth>` with redacted `Debug`.
- Produces async trait `ResetCreditTransport::fetch() -> BackendResult<ResetCreditState>`.
- Produces `ResetCreditClient::production(auth_path: PathBuf) -> BackendResult<Self>`.
- Produces `ResetCreditClient::with_loopback_endpoint(endpoint: Url, auth_path: PathBuf) -> BackendResult<Self>` for integration tests; it accepts only `http://127.0.0.1:<port>` or `http://[::1]:<port>` and is never called by production setup.
- Produces `ResetCreditCache::{new, load, store, clear}`.

- [ ] **Step 1: Add failing normalization, auth-redaction, host-lock, body-limit, and cache tests**

Test these exact cases inside `reset_credits.rs`:

```rust
#[test]
fn explicit_zero_is_preserved_and_malformed_values_are_rejected() {
    let state = normalize_reset_credit_response(serde_json::json!({"available_count": 0}), 7).unwrap();
    assert_eq!(state.available_count, Some(0));
    assert_eq!(state.fetched_at, 7);
    for value in [serde_json::json!(-1), serde_json::json!(1.5), serde_json::json!("3")] {
        assert!(normalize_reset_credit_response(serde_json::json!({"available_count": value}), 7).is_err());
    }
}

#[test]
fn credits_array_fallback_counts_only_available_unexpired_entries() {
    let state = normalize_reset_credit_response(serde_json::json!({"credits": [
        {"status": "available", "expires_at": "2030-01-01T00:00:00Z"},
        {"status": "redeemed", "expires_at": "2030-01-01T00:00:00Z"},
        {"status": "available", "expires_at": "2020-01-01T00:00:00Z"}
    ]}), 1_800_000_000).unwrap();
    assert_eq!(state.available_count, Some(1));
}
```

Implement complete neighboring tests for redacted `Debug`, loopback 302 rejection, a 65,537-byte response rejection, and safe stale cache keys using the same public interfaces. Each assertion must compare behavior, not source text.

- [ ] **Step 2: Run the new Rust unit tests and observe the unresolved-module failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib reset_credits::tests`

Expected: fail because `backend::reset_credits` is not implemented.

- [ ] **Step 3: Implement the safe client and cache**

Add `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }`, `url = "2"`, `chrono = { version = "0.4", default-features = false, features = ["std"] }`, and `dirs = "6"`.

```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCreditState {
    pub available_count: Option<u32>,
    pub fetched_at: i64,
    pub stale: bool,
}

#[async_trait::async_trait]
pub trait ResetCreditTransport: Send + Sync {
    async fn fetch(&self) -> BackendResult<ResetCreditState>;
}
```

Build Reqwest with `redirect(Policy::none())`, `timeout(Duration::from_secs(10))`, and HTTPS-only production URL. Read at most 65,536 bytes by iterating `response.chunk()`. Send `Authorization: Bearer …`, `ChatGPT-Account-ID`, `OpenAI-Beta: codex-1`, and `Originator: Gpt Orbit Weekly`. Never interpolate header values or response bodies into errors. Cache via temporary-file rename and serialize only `ResetCreditState`.

- [ ] **Step 4: Verify the client contract and entire Rust library**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib reset_credits::tests -- --test-threads=1`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --lib -- -D warnings`

Expected: all reset-credit unit tests and clippy pass without live network access.

- [ ] **Step 5: Commit Task 2**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/backend/mod.rs src-tauri/src/backend/reset_credits.rs
git commit -m "feat: read and cache reset credit count"
```

---

### Task 3: Add the reset-credit service, Tauri commands, and lifecycle hooks

**Files:**
- Create: `src-tauri/src/backend/reset_credit_service.rs`
- Modify: `src-tauri/src/backend/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/desktop/tray.rs`
- Modify: `src-tauri/tests/desktop_lifecycle.rs`
- Modify: `src-tauri/tests/tray_actions.rs`

**Interfaces:**
- Produces `ResetCreditService::start(app, transport, cache)`, `current()`, `refresh_now(reason)`, and `shutdown()`.
- Produces Tauri commands `get_reset_credits` and `refresh_reset_credits`.
- Emits `reset-credits://updated` with only `ResetCreditState`.
- Starts this service only when `AppVariant::Weekly` is selected.

- [ ] **Step 1: Write failing service and lifecycle tests**

Use a fake `ResetCreditTransport` with queued results and call counters. Verify startup loads stale cache, successful refresh publishes a live count, failed refresh retains the count with `stale=true`, concurrent refreshes coalesce, periodic interval is 300 seconds under paused Tokio time, and shutdown ends the worker. Extend tray/lifecycle contract tests so tray refresh, window show, resume, and unlock request both services when weekly state exists.

- [ ] **Step 2: Run focused tests and confirm the service is absent**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib reset_credit_service::tests -- --test-threads=1`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test tray_actions --test desktop_lifecycle`

Expected: fail on missing service and missing command/event wiring.

- [ ] **Step 3: Implement one supervised reset-credit worker**

```rust
pub enum ResetCreditCommand { Refresh(RefreshReason), Shutdown }

#[derive(Clone)]
pub struct ResetCreditService {
    current: std::sync::Arc<std::sync::Mutex<Option<ResetCreditState>>>,
    commands: tokio::sync::mpsc::UnboundedSender<ResetCreditCommand>,
    task: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}
```

The worker loads cache, refreshes immediately, ticks every 300 seconds, serializes refreshes, emits only normalized state, and stores only successful results. In `lib.rs`, derive variant from `app.config().identifier`; start/manage the reset service only for weekly. Register both commands. Tray/lifecycle uses `try_state::<ResetCreditService>()`, so standard behavior remains unchanged.

- [ ] **Step 4: Verify service, command registration, lifecycle, and shutdown**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test tray_actions --test desktop_lifecycle -- --test-threads=1`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

Expected: service and existing backend tests pass; clippy reports no warnings.

- [ ] **Step 5: Commit Task 3**

```powershell
git add src-tauri/src/backend/mod.rs src-tauri/src/backend/reset_credit_service.rs src-tauri/src/lib.rs src-tauri/src/desktop/tray.rs src-tauri/tests/desktop_lifecycle.rs src-tauri/tests/tray_actions.rs
git commit -m "feat: supervise reset credit synchronization"
```

---

### Task 4: Add the weekly-only frontend reset-credit bridge and store

**Files:**
- Create: `src/features/reset-credits/reset-credits.types.ts`
- Create: `src/features/reset-credits/reset-credits.store.ts`
- Create: `src/features/reset-credits/reset-credits.store.test.ts`
- Create: `src/features/reset-credits/reset-credits.bridge.ts`
- Create: `src/features/reset-credits/reset-credits.bridge.test.ts`
- Modify: `src/app/AppShell.tsx`
- Modify: `src/app/StandardApp.tsx`
- Modify: `src/app/WeeklyApp.tsx`
- Modify: `src/App.integration.test.tsx`

**Interfaces:**
- Produces `ResetCreditState` matching Rust camelCase fields.
- Produces `useResetCreditStore` with `snapshot`, `applySnapshot`, and `clear`.
- Produces `startResetCreditBridge(api) -> { refresh(reason), stop }`.
- `AppShell` accepts a required `variant: AppVariant`; the Vite-selected `StandardApp` and `WeeklyApp` entries pass explicit literal variants.

- [ ] **Step 1: Write failing validators, store, bridge, and variant-gating tests**

```ts
const valid = { availableCount: 3, fetchedAt: 1_800_000_000, stale: false };
expect(useResetCreditStore.getState().snapshot).toBeNull();
useResetCreditStore.getState().applySnapshot(valid);
expect(useResetCreditStore.getState().snapshot).toEqual(valid);
```

Bridge tests assert `get_reset_credits`, `reset-credits://updated`, `refresh_reset_credits`, idempotent cleanup, rejection of negative/fractional/string counts, and acceptance of `availableCount: null`. App integration tests render `StandardApp` and `WeeklyApp` separately and assert the Standard entry never invokes either reset-credit command while the Weekly entry registers the bridge.

- [ ] **Step 2: Run focused frontend tests and observe missing modules**

Run: `npm.cmd test -- --configLoader runner --run src/features/reset-credits src/App.integration.test.tsx`

Expected: fail because the reset-credit frontend modules and variant-gated `AppShell` path do not exist.

- [ ] **Step 3: Implement safe state validation and weekly-only startup**

```ts
export interface ResetCreditState {
  availableCount: number | null;
  fetchedAt: number;
  stale: boolean;
}

export function isResetCreditState(value: unknown): value is ResetCreditState {
  if (typeof value !== 'object' || value === null) return false;
  const state = value as Record<string, unknown>;
  const count = state.availableCount;
  return (count === null || (Number.isInteger(count) && Number(count) >= 0)) &&
    typeof state.fetchedAt === 'number' && Number.isFinite(state.fetchedAt) &&
    typeof state.stale === 'boolean';
}
```

`AppShell({ variant })` starts `startResetCreditBridge` only when `variant === 'weekly' && isTauri()`. `WeeklyApp` passes `variant="weekly"`, `StandardApp` passes `variant="standard"`, and `main.tsx` receives the correct entry through `#app-entry`. A bridge failure leaves the store empty and does not stop the existing quota bridge or window restore path.

- [ ] **Step 4: Verify focused and full frontend suites**

Run: `npm.cmd test -- --configLoader runner --run src/features/reset-credits src/App.integration.test.tsx`

Run: `npm.cmd test -- --configLoader runner --run`

Run: `npm.cmd run typecheck`

Expected: reset-credit tests and all existing tests pass.

- [ ] **Step 5: Commit Task 4**

```powershell
git add src/features/reset-credits src/app/AppShell.tsx src/app/StandardApp.tsx src/app/WeeklyApp.tsx src/App.integration.test.tsx
git commit -m "feat: bridge weekly reset credit state"
```

---

### Task 5: Build the weekly-only orb and red glass badge

**Files:**
- Create: `src/features/reset-credits/ResetCreditBadge.tsx`
- Create: `src/features/reset-credits/ResetCreditBadge.test.tsx`
- Create: `src/features/orbit/WeeklyOrbitWidget.tsx`
- Create: `src/features/orbit/WeeklyOrbitWidget.test.tsx`
- Modify: `src/features/orbit/useOrbitInteraction.ts`
- Modify: `src/features/orbit/useOrbitInteraction.test.tsx`
- Modify: `src/features/orbit/countdown.ts`
- Modify: `src/features/orbit/countdown.test.ts`
- Modify: `src/features/orbit/orbit-widget.css`
- Modify: `src/app/AppShell.tsx`
- Modify: `src/app/WeeklyApp.tsx`
- Modify: `src/App.test.tsx`

**Interfaces:**
- Produces `formatResetCreditCount(number | null): string` with `—`, `0..99`, and `99+`.
- Produces `ResetCreditBadge({ state })` with fixed accessible labels.
- Produces `WeeklyOrbitWidget` with the same drag/expand callbacks as `OrbitWidget`, plus `resetCredits`.
- Extends `useOrbitInteraction` with `allowFlip?: boolean`, default `true`.
- Produces `formatWeeklyCompactCountdown(resetsAt)`.

- [ ] **Step 1: Write failing rendering and interaction tests**

```tsx
render(<ResetCreditBadge state={{ availableCount: 3, fetchedAt: 1, stale: false }} />);
expect(screen.getByLabelText('剩余 3 次额度重置')).toHaveTextContent('3');
```

Weekly widget tests assert: collapsed `WEEK LEFT`; weekly percentage and compact weekly countdown; no `5H`, `5 小时`, five-hour percentage, or back face in the DOM; sustained hover reveals `Codex · 本周`; click and pointer-up never change face; leave collapses after 200ms; badge stays mounted across expansion; stale/unknown/zero/99+ states render correctly; drag greater than 6px starts native drag and does not act as a click.

- [ ] **Step 2: Run focused tests and observe missing weekly UI**

Run: `npm.cmd test -- --configLoader runner --run src/features/reset-credits/ResetCreditBadge.test.tsx src/features/orbit/WeeklyOrbitWidget.test.tsx src/features/orbit/useOrbitInteraction.test.tsx src/features/orbit/countdown.test.ts`

Expected: fail because the badge, weekly widget, no-flip option, and weekly compact formatter are missing.

- [ ] **Step 3: Implement the visual component and exact state behavior**

```ts
export function formatResetCreditCount(count: number | null): string {
  if (count === null) return '—';
  return count > 99 ? '99+' : String(count);
}
```

```tsx
<aside
  className="reset-credit-badge"
  aria-label={count === null ? '额度重置次数暂不可用' : `剩余 ${count} 次额度重置`}
  data-stale={String(state?.stale ?? false)}
>
  <span className="reset-credit-badge__value">{formatResetCreditCount(count)}</span>
  <span className="reset-credit-badge__unit">次</span>
</aside>
```

Use a persistent wrapper containing the 148→245px indigo glass body and absolutely positioned 46px badge at `left: calc(100% - 10px)`. Transition the body width and wrapper width over 350ms so the badge moves continuously. Use violet weekly ring, `Codex · 本周`, weekly reset text, green live/amber stale status, and no flipper markup. Change `.app-shell` to fill the native canvas so 12px visual margins are real at both sizes.

- [ ] **Step 4: Verify weekly UI, standard regression, typecheck, and build modes**

Run: `npm.cmd test -- --configLoader runner --run`

Run: `npm.cmd run typecheck`

Run: `npm.cmd run build`

Run: `npm.cmd run build:weekly`

Expected: all frontend tests pass; both mode builds succeed; standard tests still cover the three-state flip.

- [ ] **Step 5: Commit Task 5**

```powershell
git add src/features/reset-credits/ResetCreditBadge.tsx src/features/reset-credits/ResetCreditBadge.test.tsx src/features/orbit/WeeklyOrbitWidget.tsx src/features/orbit/WeeklyOrbitWidget.test.tsx src/features/orbit/useOrbitInteraction.ts src/features/orbit/useOrbitInteraction.test.tsx src/features/orbit/countdown.ts src/features/orbit/countdown.test.ts src/features/orbit/orbit-widget.css src/app/AppShell.tsx src/app/WeeklyApp.tsx src/App.test.tsx
git commit -m "feat: render weekly orb with reset badge"
```

---

### Task 6: Make window geometry, restore, tray, and lifecycle variant-aware

**Files:**
- Modify: `src/features/window/window.geometry.ts`
- Modify: `src/features/window/window.geometry.test.ts`
- Modify: `src/features/window/window.controller.ts`
- Modify: `src/features/window/window.controller.test.ts`
- Modify: `src/app/AppShell.tsx`
- Modify: `src-tauri/src/desktop/app_variant.rs`
- Modify: `src-tauri/src/desktop/preferences.rs`
- Modify: `src-tauri/src/desktop/tray.rs`
- Modify: `src-tauri/tests/widget_placement.rs`
- Modify: `src-tauri/tests/tray_actions.rs`

**Interfaces:**
- Produces frontend `WidgetGeometry`, `STANDARD_GEOMETRY`, and `WEEKLY_GEOMETRY`.
- `createWindowController` and `createPlacementPersistence` accept a geometry parameter with standard default.
- Rust restore selects `WidgetCanvas` from `app.config().identifier`.
- Tray tooltip uses configured product name and refreshes reset credits when the service exists.

- [ ] **Step 1: Write failing variant geometry and restore tests**

Frontend tests assert weekly collapsed/expanded canvases are 208×172 and 305×136, the collapsed ring center is 86px from canvas left, expanded ring center remains 68px, right-edge expansion clamps, and collapse returns to the adjusted weekly anchor. Rust tests assert weekly restore uses 208×172 and standard restore remains 172×172. Tray tests assert tooltip/product naming is not hard-coded and refresh action reaches both managed services.

- [ ] **Step 2: Run focused geometry and desktop tests**

Run: `npm.cmd test -- --configLoader runner --run src/features/window`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test widget_placement --test tray_actions -- --test-threads=1`

Expected: fail because geometry and restore still use standard constants.

- [ ] **Step 3: Implement explicit geometry objects and runtime selection**

```ts
export interface WidgetGeometry {
  collapsedCanvas: { width: number; height: number };
  expandedCanvas: { width: number; height: number };
  collapsedRingCenterX: number;
  expandedRingCenterX: number;
}

export const WEEKLY_GEOMETRY: WidgetGeometry = {
  collapsedCanvas: { width: 208, height: 172 },
  expandedCanvas: { width: 305, height: 136 },
  collapsedRingCenterX: 86,
  expandedRingCenterX: 68,
};
```

Pass `variant === 'weekly' ? WEEKLY_GEOMETRY : STANDARD_GEOMETRY` into both Tauri window helpers. Refactor Rust restore math to consume `WidgetCanvas` selected from the identifier. Obtain tray tooltip from `app.config().product_name` with `Gpt Orbit` fallback.

- [ ] **Step 4: Verify full frontend and Rust suites**

Run: `npm.cmd test -- --configLoader runner --run`

Run: `cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`

Run: `npm.cmd run typecheck`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

Expected: all standard and weekly geometry, placement, tray, frontend, and backend tests pass.

- [ ] **Step 5: Commit Task 6**

```powershell
git add src/features/window src/app/AppShell.tsx src-tauri/src/desktop/app_variant.rs src-tauri/src/desktop/preferences.rs src-tauri/src/desktop/tray.rs src-tauri/tests/widget_placement.rs src-tauri/tests/tray_actions.rs
git commit -m "feat: isolate weekly widget desktop geometry"
```

---

### Task 7: Prove secure integration and produce both Windows installers

**Files:**
- Create: `src-tauri/tests/reset_credit_flow.rs`
- Create: `tests/fake-reset-credit-server/server.mjs`
- Modify: `scripts/acceptance/capture-widget.ps1`
- Modify: `scripts/acceptance/verify-window.ps1`
- Modify: `docs/acceptance/gpt-orbit-windows-matrix.md`
- Modify: `.superpowers/sdd/progress.md`

**Interfaces:**
- Produces a local fake endpoint with live, zero, malformed, unauthorized, oversized, redirect, and recovery scenarios.
- Produces acceptance commands for standard and weekly variants.
- Produces two side-by-side NSIS installers and desktop screenshots.

- [ ] **Step 1: Write failing process-level reset-credit acceptance tests**

The integration test creates a temporary `CODEX_HOME/auth.json` with synthetic token/account values, starts the local fake server, and proves: live count 3; explicit zero; stale cache after disconnect; malformed/oversized response never becomes zero; unauthorized becomes unavailable without leaking the synthetic secrets; redirect is rejected; recovery replaces stale state. Add a source scan asserting `/consume` is absent from production Rust and TypeScript.

- [ ] **Step 2: Run the acceptance test before adding the fixture**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test reset_credit_flow -- --test-threads=1`

Expected: fail because the fake server and process-level test contract do not exist.

- [ ] **Step 3: Implement the fake server and acceptance documentation**

The fake server binds only to `127.0.0.1`, records request count and received header names without recording values, and emits scenario-selected JSON. Update capture checklist filenames to `weekly-collapsed.png` and `weekly-expanded.png`, add weekly visible geometry and no-flip checks, and expand the matrix with data privacy, stale fallback, two installer identities, simultaneous process, and independent placement rows.

- [ ] **Step 4: Run the complete release verification matrix**

Run:

```powershell
npm.cmd test -- --configLoader runner --run
npm.cmd run typecheck
npm.cmd run build
npm.cmd run build:weekly
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm.cmd run tauri info
git diff --check
```

Expected: all commands pass; no live ChatGPT requests occur.

- [ ] **Step 5: Build separate NSIS bundles**

```powershell
$env:CARGO_INCREMENTAL='0'
$env:RUSTFLAGS='-C debuginfo=0'
$releaseRoot = Join-Path $env:TEMP 'gpt-orbit-release'
$env:CARGO_TARGET_DIR = Join-Path $releaseRoot 'standard'
npm.cmd run tauri build
$env:CARGO_TARGET_DIR = Join-Path $releaseRoot 'weekly'
npm.cmd run tauri:build:weekly
```

Expected artifacts:

- `$releaseRoot\standard\release\bundle\nsis\Gpt Orbit_0.1.0_x64-setup.exe`
- `$releaseRoot\weekly\release\bundle\nsis\Gpt Orbit Weekly_0.1.0_x64-setup.exe`

- [ ] **Step 6: Run Windows visual and coexistence acceptance**

Install both current-user NSIS packages, launch both processes, drag them to different positions, restart each, confirm independent restore, capture weekly collapsed and expanded states on a dark desktop, and verify the red badge remains on the right. With the user's existing Codex login, compare the badge to a one-shot local read of `available_count` that prints only the integer and never prints headers, tokens, account ID, credit IDs, or raw JSON. Inspect screenshots for readable Chinese, no five-hour text, no secret/account content, halo clipping, and badge clipping. Record exact evidence paths in the matrix.

- [ ] **Step 7: Commit Task 7**

```powershell
git add src-tauri/tests/reset_credit_flow.rs tests/fake-reset-credit-server scripts/acceptance docs/acceptance .superpowers/sdd/progress.md
git commit -m "test: verify weekly widget release flow"
```

## Final completion audit

Before declaring completion, compare every requirement in `docs/superpowers/specs/2026-07-13-gpt-orbit-weekly-reset-credits-design.md` with code, automated test output, installer metadata, running processes, persisted positions, and screenshots. A missing screenshot, unbuilt installer, live-network test, secret-bearing log, visible five-hour content in the weekly variant, or unverified coexistence keeps the goal incomplete.
