# Codex Orbit Quota MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** Build a Windows 11 Codex quota widget that reads real five-hour and weekly limits, shows five-hour quota by default, switches to weekly quota after 150ms hover, and restores five-hour quota 200ms after pointer leave.

**Architecture:** This is the orchestration plan for three bounded implementation streams. The Rust backend owns one Codex App Server process and emits normalized state; the React frontend owns display state and accessibility; the Windows stream owns window, tray, lifecycle, installer, and end-to-end acceptance. Shared contracts are frozen below and may only change through a coordinated edit to all affected plans.

**Tech Stack:** Tauri 2, Rust 2021, Tokio, Serde, React 19, TypeScript 5, Zustand 5, Vite, Vitest, Testing Library, npm, Windows 11, NSIS.

## Global Constraints

- Target Windows 11; macOS is outside this MVP.
- Use stable Codex App Server methods without experimental API capabilities.
- Default view is fiveHour; hover enter delay is 150ms and leave delay is 200ms.
- Display remainingPercent = clamp(100 - usedPercent, 0, 100).
- Poll every 300 seconds and refresh on login completion, show, resume, unlock, manual request, and expired countdown.
- Keep cursor pass-through disabled by default; when enabled, hover switching is unavailable and tray viewing remains available.
- Never read conversation content, prompts, browser cookies, or unrelated workspace files.
- Never persist or log access tokens, email addresses, auth URLs, or complete App Server payloads.
- Preserve the last valid snapshot on failure and mark it stale.
- Idle acceptance targets are median CPU below 1% and working set below 150MB.
- Every implementation task follows red-green-refactor and ends with one focused commit.
- The repository is currently uninitialized; Task 1 creates main and commits the approved design and plans before code work.

---

## Plan Set

- Backend: docs/superpowers/plans/2026-07-12-codex-orbit-backend.md
- Frontend: docs/superpowers/plans/2026-07-12-codex-orbit-frontend.md
- Windows and release: docs/superpowers/plans/2026-07-12-codex-orbit-windows.md
- Approved design: docs/superpowers/specs/2026-07-12-codex-orbit-quota-mvp-design.md

## Frozen Shared Contracts

Rust serializes and TypeScript consumes this exact camelCase shape:

~~~ts
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

export type ConnectionStatus =
  | 'starting'
  | 'loginRequired'
  | 'refreshing'
  | 'live'
  | 'offline';
~~~

Tauri commands:

~~~text
get_rate_limits() -> RateLimitState | null
refresh_rate_limits() -> void
start_login() -> void
set_always_on_top(enabled: boolean) -> void
set_click_through(enabled: boolean) -> InteractionCapabilities
get_desktop_preferences() -> DesktopPreferences
~~~

Tauri events:

~~~text
rate-limits://updated        payload RateLimitState
rate-limits://status         payload {"status": ConnectionStatus, "message": string | null}
account://login-url          payload {"loginId": string, "authUrl": string}
desktop://show-view          payload {"mode": "fiveHour" | "weekly"}
desktop://interaction-capabilities payload InteractionCapabilities
desktop://lifecycle          payload {"paused": boolean, "refreshRequested": boolean}
~~~

Backend refresh interface:

~~~rust
pub enum RefreshReason {
    Startup,
    Poll,
    LoginCompleted,
    Manual,
    WindowShown,
    Resume,
    SessionUnlocked,
    ResetExpired,
    Tray,
}

pub fn refresh_now(&self, reason: RefreshReason) -> Result<(), RateLimitError>;
~~~

## Ownership Rules

- Backend stream exclusively edits src-tauri/src/app_server, src-tauri/src/rate_limits, src-tauri/src/cache.rs, and the quota Tauri commands.
- Frontend stream exclusively edits src/features/quota and src/test.
- Windows stream exclusively edits src-tauri/src/desktop, acceptance scripts, icons, and installer settings.
- src-tauri/src/lib.rs, src/App.tsx, package.json, Cargo.toml, capabilities, and Tauri config are integration files. Only the active integration task may edit them.
- A subagent must not change a frozen shared contract by itself. It reports the required change to the coordinator, who updates all consumers in one integration commit.
- Every subagent receives only its task text, frozen contracts, current test command, and directly relevant files.
- After each implementation task, dispatch a specification reviewer first and a code-quality reviewer second. Do not begin the next task until both reviews pass.

---

### Task 1: Initialize Repository and Buildable Tauri Skeleton

**Files:**
- Create: .gitignore
- Create: package.json
- Create: package-lock.json
- Create: tsconfig.json
- Create: tsconfig.app.json
- Create: tsconfig.node.json
- Create: vite.config.ts
- Create: index.html
- Create: src/main.tsx
- Create: src/App.tsx
- Create: src/test/setup.ts
- Create: src-tauri/Cargo.toml
- Create: src-tauri/build.rs
- Create: src-tauri/tauri.conf.json
- Create: src-tauri/capabilities/default.json
- Create: src-tauri/src/main.rs
- Create: src-tauri/src/lib.rs
- Test: src/App.test.tsx

**Interfaces:**
- Produces npm scripts typecheck, test, build, tauri, tauri:dev, tauri:build, and rust:test.
- Produces Tauri main window label main at fixed logical size 320 by 220.

- [ ] **Step 1: Initialize Git and commit approved documents**

Run:

~~~powershell
git init
git branch -M main
git add docs
git commit -m "docs: add Codex Orbit design and plans"
~~~

Expected: one documentation-only commit on main.

- [ ] **Step 2: Create the failing smoke test**

Create src/App.test.tsx:

~~~tsx
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import App from './App';

describe('App', () => {
  it('renders the product name while the backend starts', () => {
    render(<App />);
    expect(screen.getByText('Codex Orbit')).toBeInTheDocument();
  });
});
~~~

- [ ] **Step 3: Create exact npm scripts and dependencies**

package.json must contain Node >=22, React 19, Zustand 5, Tauri 2, Vitest, Testing Library, jsdom, and these scripts:

~~~json
{
  "scripts": {
    "dev": "vite --port 1420",
    "build": "tsc -b && vite build",
    "typecheck": "tsc -b --pretty false",
    "test": "vitest",
    "tauri": "tauri",
    "tauri:dev": "tauri dev",
    "tauri:build": "tauri build",
    "rust:test": "cargo test --manifest-path src-tauri/Cargo.toml"
  }
}
~~~

Cargo.toml must include Tauri 2 with tray-icon, Tokio process/io/sync/time/macros, Serde, serde_json, thiserror, tracing, tauri-plugin-autostart, and tauri-plugin-opener.

- [ ] **Step 4: Configure the transparent window**

Use this window object in src-tauri/tauri.conf.json:

~~~json
{
  "label": "main",
  "title": "Codex Orbit",
  "width": 320,
  "height": 220,
  "minWidth": 320,
  "minHeight": 220,
  "maxWidth": 320,
  "maxHeight": 220,
  "transparent": true,
  "decorations": false,
  "resizable": false,
  "shadow": false,
  "alwaysOnTop": true,
  "skipTaskbar": true,
  "visible": false
}
~~~

- [ ] **Step 5: Install and verify the skeleton**

Run:

~~~powershell
npm install
npm test -- --run src/App.test.tsx
npm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri info
~~~

Expected: smoke test passes, TypeScript and Cargo exit 0, and Tauri reports no unknown configuration fields.

- [ ] **Step 6: Commit**

~~~powershell
git add .gitignore package.json package-lock.json tsconfig*.json vite.config.ts index.html src src-tauri
git commit -m "build: initialize Codex Orbit workspace"
~~~

---

### Task 2: Execute Backend Plan

**Files:** Use the exact file map in the backend subplan.

**Interfaces:** Produces all frozen quota commands and events, plus RateLimitService::refresh_now.

- [ ] **Step 1: Dispatch a fresh backend implementer**

Give the implementer the backend subplan, frozen contracts, and ownership rules. Require completion of one backend task at a time with the specified failing test evidence and commit.

- [ ] **Step 2: Run backend quality gate after each backend task**

Run:

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run rust:test
~~~

Expected: all commands exit 0.

- [ ] **Step 3: Run backend integration gate**

Run the fake App Server scenarios complete, login-required-then-success, sparse-update, malformed-json-then-valid, disconnect-once, and timeout.

Expected: initialization precedes all account calls, one child exists at a time, sparse updates preserve weekly data, and failures keep stale cache visible.

---

### Task 3: Execute Frontend Plan

**Files:** Use the exact file map in the frontend subplan.

**Interfaces:** Consumes RateLimitState and frozen Tauri commands/events. Produces accessible QuotaWidget.

- [ ] **Step 1: Dispatch a fresh frontend implementer**

Give the implementer the frontend subplan and frozen contracts. Require failing test output for store, countdown, hover state machine, quota widget, and interaction regression tasks.

- [ ] **Step 2: Run frontend quality gate after each frontend task**

Run:

~~~powershell
npm test -- --run
npm run typecheck
npm run build
~~~

Expected: all tests pass, no timer or React act warnings appear, and dist is generated.

- [ ] **Step 3: Verify the core interaction**

Expected automated evidence:
- initial label is 5 小时;
- 149ms hover remains five-hour;
- 150ms hover changes label, percentage, ring, and reset time to weekly;
- 199ms after leave remains weekly;
- 200ms after leave restores five-hour;
- rapid passes leave no stale timer;
- missing weekly data never switches;
- W and Escape work while focused;
- reduced-motion removes transitions.

---

### Task 4: Integrate Backend and Frontend

**Files:**
- Modify: src-tauri/src/lib.rs
- Modify: src/App.tsx
- Modify: src-tauri/capabilities/default.json
- Test: src/integration/appBridge.test.tsx
- Test: src-tauri/tests/command_contract.rs

**Interfaces:** Uses only frozen commands and events.

- [ ] **Step 1: Write failing bridge contract tests**

Rust test serializes a canonical RateLimitState and asserts camelCase keys fiveHour, weekly, remainingPercent, resetsAt, fetchedAt, and stale. Frontend test mocks get_rate_limits and rate-limits://updated, then asserts the store moves from cache/stale to updated/live without changing weekly display mode.

- [ ] **Step 2: Run tests and observe failure**

~~~powershell
cargo test --manifest-path src-tauri/Cargo.toml --test command_contract
npm test -- --run src/integration/appBridge.test.tsx
~~~

Expected: tests fail because final command/event registration is absent.

- [ ] **Step 3: Register one backend state and bridge**

lib.rs must manage one RateLimitService, register the six frozen commands, emit cached state before starting App Server, start one supervisor in setup, and stop it during tray quit. App.tsx must subscribe once, apply the initial command result, apply update and status events, open the official login URL only in response to account://login-url, and clean every listener on unmount.

- [ ] **Step 4: Run combined tests**

~~~powershell
npm test -- --run
npm run typecheck
npm run rust:test
npm run tauri:dev
~~~

Expected: the card starts with cache or loading state and moves to live real or fake quota data without a reload.

- [ ] **Step 5: Commit**

~~~powershell
git add src/App.tsx src/integration src-tauri/src/lib.rs src-tauri/capabilities src-tauri/tests/command_contract.rs
git commit -m "feat: connect quota backend to widget"
~~~

---

### Task 5: Execute Windows Desktop and Release Plan

**Files:** Use the exact file map in the Windows subplan.

**Interfaces:** Consumes RateLimitService::refresh_now and frozen desktop events.

- [ ] **Step 1: Dispatch one desktop implementer per Windows task**

Use separate implementers for placement/preferences, tray/autostart, lifecycle refresh, fake-process integration, and release acceptance. Do not run two implementers that edit the same integration file concurrently.

- [ ] **Step 2: Run desktop quality gate after each task**

~~~powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
npm run rust:test
npm test -- --run
npm run tauri info
~~~

Expected: all commands exit 0 and exactly one tray icon is built from Rust, not duplicated in config.

- [ ] **Step 3: Run the Windows matrix**

Verify single and mixed-DPI monitors, disconnected saved monitor, always-on-top, pass-through warning, sleep/resume, lock/unlock, tray actions, autostart, install, patch upgrade, and uninstall.

---

### Task 6: Final Release Gate

**Files:**
- Create: docs/acceptance/windows-matrix.md
- Create: docs/acceptance/real-account-template.md
- Create: docs/acceptance/24h-template.csv
- Create: scripts/acceptance/monitor-24h.ps1
- Create: scripts/acceptance/check-install.ps1
- Modify: README.md

- [ ] **Step 1: Run every automated check**

~~~powershell
npm ci
npm run typecheck
npm test -- --run
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri:build -- --target x86_64-pc-windows-msvc
~~~

Expected: every command exits 0 and one current-user NSIS installer is produced.

- [ ] **Step 2: Compare real quota data**

Record three samples at least five minutes apart and one after an update. Pass when whole-number percentages match at comparable capture times, reset timestamps differ by no more than 60 seconds, and update delivery reaches UI within one second.

- [ ] **Step 3: Run the 24-hour soak**

Sample once per minute. During the run, perform sleep/wake, network disconnect/reconnect, hide/show, and forced App Server termination. Pass when child count never exceeds one, idle CPU median is below 1%, working set remains below 150MB without sustained final-six-hour growth, and tray exit leaves no child.

- [ ] **Step 4: Build, install, upgrade, and uninstall**

Expected: current-user installation needs no elevation; relaunch restores an on-screen position; autostart toggles persist; a higher patch installer replaces the old version while Orbit is closed; uninstall removes Orbit and its startup entry without touching Codex credentials.

- [ ] **Step 5: Commit release evidence**

~~~powershell
git add README.md docs/acceptance scripts/acceptance
git commit -m "docs: verify Codex Orbit Windows MVP"
git status --short
~~~

Expected: the final working tree is clean.

## Completion Definition

- Backend, frontend, and Windows subplans are fully checked off.
- Every task has failing-test evidence, passing-test evidence, one implementation commit, one specification review, and one code-quality review.
- Five-hour is the default and weekly hover timing matches 150ms/200ms.
- Cache and logs contain no secrets.
- Real-account comparison, 24-hour soak, and NSIS lifecycle all pass.
