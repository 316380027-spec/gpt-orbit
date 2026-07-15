# Codex Orbit Windows Desktop Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the Windows desktop shell, recovery behavior, integration tests, and unsigned NSIS acceptance gate for the Codex Orbit quota MVP.

**Architecture:** Keep all Windows/Tauri behavior behind focused Rust adapters: window placement, tray/autostart, and lifecycle. The React layer receives normalized desktop events only. Acceptance uses the real App Server supervisor with an injectable fake executable, followed by real-account and 24-hour Windows checks.

**Tech Stack:** Tauri 2, Rust, React, TypeScript, Vitest, Testing Library, PowerShell, NSIS, Node.js JSONL fixture server.

## Global Constraints

- Target platform is Windows 11; MVP does not add macOS support.
- Main window is a fixed 320 x 220 transparent, borderless, non-resizable widget.
- Always-on-top defaults to enabled; click-through defaults to disabled so hover switching works.
- Enabling click-through must clearly state that hover switching is paused; weekly quota remains available from the tray.
- Restore must work at 100%, 125%, and 150% DPI and after the saved monitor is disconnected.
- Window show, system resume, and session unlock trigger an immediate quota refresh, coalesced within two seconds.
- At most one widget-managed App Server child may exist at any time.
- The 24-hour gate requires no crash, idle CPU below 1%, and memory below 150 MB.
- Logs and acceptance artifacts must not contain tokens, email addresses, raw App Server responses, credentials, or conversation content.
- MVP produces an unsigned current-user NSIS installer; signing, updater, and public release are out of scope.

## Required Backend Interfaces

These interfaces must exist before this plan is executed. If their implementing task has not landed, stop at the first dependent step instead of inventing a second quota service.

```rust
pub enum RefreshReason {
    Startup,
    Poll,
    LoginCompleted,
    Manual,
    Tray,
    WindowShown,
    Resume,
    SessionUnlocked,
    ResetExpired,
}

pub trait RateLimitService: Send + Sync {
    fn refresh_now(&self, reason: RefreshReason) -> Result<(), RateLimitError>;
}

pub struct AppServerLaunch {
    pub program: std::path::PathBuf,
    pub args: Vec<std::ffi::OsString>,
}

pub async fn run_with_launch(
    launch: AppServerLaunch,
    events: impl RateLimitEventSink,
) -> Result<AppServerHandle, AppServerError>;

impl AppServerHandle {
    pub async fn shutdown(self) -> Result<(), AppServerError>;
}
```

Frontend quota store must already expose:

```ts
export type QuotaViewMode = 'fiveHour' | 'weekly';
export function setQuotaViewMode(mode: QuotaViewMode): void;
```

---

### Task 1: Transparent Window and DPI-Safe Placement

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/desktop/mod.rs`
- Create: `src-tauri/src/desktop/preferences.rs`
- Create: `src-tauri/src/desktop/placement.rs`
- Test: `src-tauri/tests/window_placement.rs`
- Create: `src/lib/desktop/types.ts`
- Create: `src/lib/desktop/api.ts`

**Interfaces:**
- Consumes: Tauri main window label `main`.
- Produces:

```rust
pub struct MonitorSnapshot {
    pub name: Option<String>,
    pub origin_px: (i32, i32),
    pub size_px: (u32, u32),
    pub scale_factor: f64,
}

pub struct SavedPlacement {
    pub monitor_name: Option<String>,
    pub offset_logical: (f64, f64),
    pub saved_scale_factor: f64,
}

pub fn restore_position(
    saved: &SavedPlacement,
    monitors: &[MonitorSnapshot],
    primary_index: usize,
    window_size_logical: (f64, f64),
) -> (i32, i32);

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPreferences {
    pub always_on_top: bool,
    pub click_through: bool,
    pub autostart: bool,
    pub placement: Option<SavedPlacement>,
}
```

- [ ] **Step 1: Write failing placement tests**

Add tests proving that a missing saved monitor falls back to the primary monitor, positions are clamped fully inside the monitor, and current scale factor replaces the saved factor. Use a 1920 x 1080 monitor at 125%, a 320 x 220 logical window, and assert the clamped physical result is `(1520, 805)`.

- [ ] **Step 2: Verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test window_placement`

Expected: FAIL because `restore_position` is not defined.

- [ ] **Step 3: Implement the window policy**

Configure `main` with `width: 320`, `height: 220`, matching min/max sizes, `transparent: true`, `decorations: false`, `resizable: false`, `shadow: false`, `alwaysOnTop: true`, `skipTaskbar: true`, and `visible: false`.

Persist monitor name plus monitor-relative logical offset after `WindowEvent::Moved`, debouncing disk writes by 300 ms. Restore using the current monitor scale and clamp before showing. Defaults are always-on-top on, click-through off, autostart off.

- [ ] **Step 4: Verify implementation**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test window_placement
npm run tauri info
```

Expected: placement tests PASS and Tauri accepts every window configuration field.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/tauri.conf.json src-tauri/src/lib.rs src-tauri/src/desktop src-tauri/tests/window_placement.rs src/lib/desktop
git commit -m "feat: add Windows widget window state"
```

### Task 2: Tray, Always-on-Top, Click-Through, and Autostart

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Create: `src-tauri/src/desktop/tray.rs`
- Create: `src-tauri/src/desktop/autostart.rs`
- Test: `src-tauri/tests/tray_actions.rs`

**Interfaces:**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ToggleVisibility,
    RefreshNow,
    ShowFiveHour,
    ShowWeekly,
    ToggleAlwaysOnTop,
    ToggleClickThrough,
    ToggleAutostart,
    Quit,
}

pub fn tray_action(id: &str) -> Option<TrayAction>;
```

Events emitted to React:

```text
desktop://show-view payload {"mode":"fiveHour"|"weekly"}
desktop://interaction-capabilities payload {"hoverSwitchAvailable":boolean,"message":string|null}
```

- [ ] **Step 1: Write the failing action map test**

Assert exact mappings for `show-hide`, `refresh`, `view-five-hour`, `view-weekly`, `always-on-top`, `click-through`, `autostart`, and `quit`; assert unknown IDs return `None`.

- [ ] **Step 2: Verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test tray_actions`

Expected: FAIL because `tray_action` is undefined.

- [ ] **Step 3: Build exactly one native tray**

Enable Tauri's tray feature and `tauri-plugin-autostart = "2"`. Construct the tray only with `TrayIconBuilder`; do not also declare a tray in `tauri.conf.json`.

Menu labels are `显示/隐藏`, `立即刷新`, `查看 5 小时`, `查看本周`, `始终置顶`, `鼠标穿透`, `开机启动`, and `退出`. Route refresh to `RateLimitService::refresh_now(RefreshReason::Tray)`. View commands show the window and emit `desktop://show-view`.

Before calling `set_ignore_cursor_events(true)`, emit `鼠标穿透已开启，悬停切换暂停；可从托盘查看周额度。`; then change the tray label to `鼠标穿透 ✓（悬停暂停）`. Close requests hide the widget. Only `退出` saves pending state, shuts down App Server, removes the tray, and exits.

- [ ] **Step 4: Verify tray behavior**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test tray_actions
npm run tauri dev
```

Expected: tests PASS; Windows displays exactly one tray icon; every toggle updates immediately; autostart changes without elevation; tray exit leaves no App Server child.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/src/lib.rs src-tauri/capabilities/default.json src-tauri/src/desktop src-tauri/tests/tray_actions.rs
git commit -m "feat: add tray and startup controls"
```

### Task 3: Resume and Visibility Refresh Policy

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/desktop/lifecycle.rs`
- Test: `src-tauri/tests/lifecycle_policy.rs`
- Create: `src/hooks/useDesktopLifecycle.ts`
- Test: `src/hooks/useDesktopLifecycle.test.ts`
- Modify: `src/App.tsx`

**Interfaces:**

```rust
pub enum LifecycleSignal {
    WindowShown,
    Suspended,
    Resumed,
    SessionLocked,
    SessionUnlocked,
}

pub struct LifecycleDecision {
    pub pause_animations: bool,
    pub refresh_now: bool,
}

pub fn lifecycle_decision(signal: LifecycleSignal) -> LifecycleDecision;
```

- [ ] **Step 1: Write failing lifecycle tests**

Assert suspend/lock pauses without refresh; resume/unlock/show unpauses and requests refresh. Add a frontend test proving paused state stops visual timers without deleting the last quota snapshot.

- [ ] **Step 2: Verify failures**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test lifecycle_policy
npm test -- --run src/hooks/useDesktopLifecycle.test.ts
```

Expected: FAIL because the policy and hook do not exist.

- [ ] **Step 3: Implement coalesced refresh**

Map Tauri resume, window show, and Windows session unlock into `RefreshReason`. Coalesce simultaneous lifecycle signals into one refresh within two seconds. React pauses CSS transitions and countdown repaint while hidden, locked, or suspended; on resume it renders cached data immediately and waits for the Rust snapshot rather than issuing RPC itself.

- [ ] **Step 4: Verify**

Run the two commands from Step 2.

Expected: PASS. A manual 60-second sleep/wake produces one sanitized resume refresh and no duplicate App Server child.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/desktop/lifecycle.rs src-tauri/tests/lifecycle_policy.rs src/hooks src/App.tsx
git commit -m "feat: refresh quotas after Windows resume"
```

### Task 4: Fake App Server Integration Suite

**Files:**
- Modify: `src-tauri/src/app_server/process.rs`
- Modify: `src-tauri/src/app_server/mod.rs`
- Create: `tests/fake-app-server/server.mjs`
- Create: `tests/fake-app-server/scenarios.mjs`
- Test: `src-tauri/tests/app_server_integration.rs`
- Modify: `package.json`

- [ ] **Step 1: Write failing process-level tests**

Spawn the fake executable through `AppServerLaunch`. Cover `complete`, `login-required-then-success`, `sparse-update`, `malformed-json-then-valid`, `disconnect-once`, and `timeout`. Each test has a ten-second Tokio timeout and a drop guard that terminates its child.

- [ ] **Step 2: Verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test app_server_integration -- --nocapture`

Expected: FAIL because the fixture server and injectable launch path do not exist.

- [ ] **Step 3: Implement deterministic JSONL fixtures**

`server.mjs` reads one JSON object per line, returns matching IDs, emits notifications without IDs, and writes diagnostics only to stderr. Synchronize on incoming methods instead of arbitrary sleeps. `disconnect-once` uses a test-provided marker file so only the first process exits.

- [ ] **Step 4: Run the complete automated gate**

```powershell
npm run typecheck
npm test -- --run
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: PASS and no fixture `node.exe` remains.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/app_server tests/fake-app-server src-tauri/tests/app_server_integration.rs package.json
git commit -m "test: add fake app server integration suite"
```

### Task 5: Real Account, 24-Hour, and NSIS Acceptance

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Create: `scripts/acceptance/compare-usage.ps1`
- Create: `scripts/acceptance/monitor-24h.ps1`
- Create: `scripts/acceptance/check-install.ps1`
- Create: `docs/acceptance/windows-matrix.md`
- Create: `docs/acceptance/real-account-template.md`
- Create: `docs/acceptance/24h-template.csv`

- [ ] **Step 1: Configure the unsigned installer**

Set `bundle.targets` to `["nsis"]` and `bundle.windows.nsis.installMode` to `currentUser`. Do not add signing or updater configuration.

- [ ] **Step 2: Build the release candidate**

```powershell
npm ci
npm run typecheck
npm test -- --run
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build -- --target x86_64-pc-windows-msvc
```

Expected: one installer at `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*_x64-setup.exe`.

- [ ] **Step 3: Validate installation lifecycle**

Run `scripts/acceptance/check-install.ps1` against the installer. Verify current-user install without elevation, one tray icon, one managed child maximum, restored visible placement, autostart off/on/off, upgrade using a higher patch version while Orbit is exited, and complete uninstall without touching Codex credentials.

- [ ] **Step 4: Compare a real account**

Record three samples at least five minutes apart and one after an update. Remaining percentage must match the official Usage page after identical whole-number rounding; reset time must differ by no more than 60 seconds; an update must reach UI within one second. Artifacts contain only observed times, remaining values, and reset times.

- [ ] **Step 5: Run the soak**

```powershell
powershell -ExecutionPolicy Bypass -File scripts/acceptance/monitor-24h.ps1 -DurationHours 24 -SampleSeconds 60 -Output artifacts/acceptance/24h.csv
```

During the run perform one sleep/wake, network disconnect/reconnect, hide/show, and forced App Server termination. Expected: no crash, child count never above one, idle CPU median below 1%, memory below 150 MB with no sustained final-six-hour growth, and no child after tray exit.

- [ ] **Step 6: Complete the Windows matrix**

Record PASS/FAIL with timestamps for single and dual monitors; 100%, 125%, and 150% DPI; mixed-DPI monitor movement; disconnected saved monitor; always-on-top; click-through warning; sleep/wake; lock/unlock; tray actions; autostart; and NSIS install/upgrade/uninstall.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/tauri.conf.json scripts/acceptance docs/acceptance
git commit -m "chore: add Windows release acceptance gate"
```

## Completion Gate

This plan is complete only when all five task test commands pass and the following evidence exists:

- `docs/acceptance/windows-matrix.md` has no unexplained FAIL.
- Real-account evidence proves both windows when supplied, reset error at most 60 seconds, and update latency at most one second.
- The 24-hour CSV proves no crash, no duplicate child, idle CPU below 1%, and memory below 150 MB.
- A clean-machine current-user NSIS install, higher-version upgrade, and uninstall all pass.
- Logs and artifacts pass a manual sensitive-data scan.

## Risks and Mitigations

- **Click-through disables WebView hover:** keep it off by default, warn before enabling, and preserve tray weekly view.
- **Raw coordinates fail across DPI/topology changes:** save monitor-relative logical offsets, restore with current scale, and clamp before showing.
- **Duplicate tray icons:** construct the tray in Rust only, never in both config and code.
- **Close-to-hide can strand background work:** tray quit must flush preferences and await App Server shutdown before exiting.
- **Resume/show/unlock event bursts:** coalesce refreshes for two seconds.
- **NSIS same-version replacement is unreliable:** test an incremented patch version with Orbit exited.
- **A single final memory reading hides leaks:** retain one-minute samples and reject sustained growth during the final six hours.
- **Native unlock events vary by runtime version:** isolate Windows capture in `desktop/lifecycle.rs`; record lock/unlock separately in the matrix.
