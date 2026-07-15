# Gpt Orbit Windows 11 Floating Widget Design

**Date:** 2026-07-12  
**Status:** Approved  
**Scope:** Production visual and interaction specification for the Codex quota desktop widget  
**Precedence:** This document supersedes earlier frontend visual and hover behavior where they conflict. Existing backend protocol, privacy, cache, and recovery requirements remain in force.

## 1. Product identity and goals

The visible product name is **Gpt Orbit** in the window metadata, tray, installer, and application display name. Quota labels remain **Codex · 5 小时** and **Codex · 本周** because the data comes from Codex app-server. Internal protocol identifiers may retain their compatibility names.

Gpt Orbit is a quiet Windows 11 desktop ornament that exposes Codex quota state without resembling a gaming HUD. It is a transparent, frameless, freely draggable widget with premium aurora purple-blue glass, high-quality Chinese typography, and three tightly defined visual states.

## 2. Window architecture

Use a Tauri transparent native window with no title bar, chrome, or taskbar button. The window is always-on-top by default without taking keyboard focus. A tray checkbox can disable always-on-top and return it to normal window stacking.

The visible orb is exactly **148 × 148 px**. The visible capsule is exactly **245 × 112 px**. The native canvas may include a small transparent margin for halo and shadow, but transparent margins must not become a broad click-blocking region.

The native window dynamically resizes between states. The original orb center is the expansion anchor and continuously becomes the left quota ring. The capsule primarily grows to the right. Near a work-area edge, the window is clamped and shifted only as much as necessary to remain fully visible. The safe adjusted position becomes the new collapse position.

Window position is stored per monitor and DPI. On restart, restore the prior location. If the monitor no longer exists, place the widget in the upper-right safe area of the primary display.

## 3. Interaction state machine

The state machine has stable states `collapsed`, `expandedFront`, and `expandedBack`, plus transient expansion, collapse, and flip phases.

### 3.1 Hover

- Pointer entry starts a 150ms confirmation timer.
- Leaving before 150ms keeps the widget collapsed.
- After 150ms, the orb expands to the front capsule over approximately 350ms.
- Hover expansion always shows the five-hour quota. Hover never reveals weekly quota.
- Re-entry during the leave grace period cancels collapse.

### 3.2 Click and flip

- A click on the expanded capsule flips between front and weekly faces.
- The flip lasts approximately 450ms around the horizontal axis with subtle perspective and inertia and no bounce.
- Faces switch close to 90 degrees so text is never mirrored.
- Capsule size and screen position remain fixed during the flip.

### 3.3 Pointer leave

- Pointer leave starts a 200ms grace timer.
- At expiry, the state returns to the front face before collapsing.
- Collapse uses the inverse size, corner-radius, layout, and opacity transition.

### 3.4 Dragging

- Pointer down records the origin.
- Movement greater than 6px begins native window dragging through Tauri.
- Movement at or below 6px remains eligible for click.
- Starting a drag cancels hover, leave, and click/flip timers.
- Releasing after a drag never flips the quota face.
- The final position is clamped to the current display work area and persisted.

### 3.5 Reduced motion

Honor `prefers-reduced-motion`. Replace the 3D flip with a short crossfade and use restrained size/opacity changes. Do not rotate, bounce, or use pronounced inertia.

## 4. Visual system

The material is deep translucent indigo glass with controlled blur, a thin highlight border, cyan refraction in the upper-left, violet glow in the lower-right, and a restrained floating shadow. CSS-rendered glass is the stable baseline; Windows-native backdrop effects are optional enhancement and must not be required for legibility.

Typography uses `Segoe UI Variable`, `Microsoft YaHei UI`, and system sans-serif fallbacks. Numeric fields use tabular figures to prevent countdown layout shift.

### 4.1 Default orbit sphere

- Visible body: 148 × 148px circular glass orb.
- Add a thin, subtle orbital halo.
- Show only `5H LEFT`, the remaining percentage such as `73%`, and a compact countdown such as `02:18`.
- Percentage is the visual center. The label and countdown are quieter.
- Do not show long labels, status controls, or weekly quota.

### 4.2 Hovered front capsule

- Visible body: 245 × 112px with approximately 56px corner radius.
- The original orb center becomes the left ice-blue quota ring.
- Ring center shows the live five-hour remaining percentage.
- Right title: `Codex · 5 小时`.
- Subtitle: localized reset text such as `2 小时 18 分后重置`.
- Bottom status: small green glow dot and `额度实时同步`.
- Text fades in with minimal displacement; it does not fly across the screen.

### 4.3 Weekly back face

- Keep the capsule dimensions and position unchanged.
- Use a violet glowing quota ring and the weekly remaining percentage.
- Title: `Codex · 本周`.
- Subtitle: localized absolute reset text such as `周一 09:30 重置`.
- Retain the live-sync indicator.

## 5. Data and status behavior

At mount, invoke the frozen Tauri command `get_rate_limits`, then subscribe to `rate-limits://updated` and status events. The initial query prevents loss of cache publication before frontend listeners are ready.

Display backend-provided `remainingPercent`; never display `usedPercent` as remaining. Derive countdown text from `resetsAt` and update it locally without restarting the interaction animation. Incoming quota updates must not change the current front/back face.

Status mapping:

- Live: green dot, `额度实时同步`.
- Cached or temporarily offline: amber dot, `显示上次同步额度` on expanded faces.
- Weekly unavailable: weekly face shows `--%` and `周额度暂不可用`.
- Login required: collapsed orb shows `--%`; expanded content shows a safe login message. The login URL is sent only through the controlled event path to the system browser and is never logged or cached.

Backend error messages shown to the UI are fixed, user-safe categories. Raw protocol data, tokens, email addresses, login URLs, and server error bodies must not be logged or persisted.

## 6. Components and boundaries

- `OrbitStateMachine`: all hover, leave, flip, drag-threshold, and reduced-motion transitions.
- `OrbitVisual`: orb, capsule, quota ring, glass layers, front face, and weekly face.
- `WindowController`: native resize, ring-center anchoring, edge avoidance, dragging, position persistence, monitor, and DPI handling.
- `QuotaBridge`: retained initial query, event subscription, countdown, and safe state mapping.

These units communicate through explicit state and command interfaces. Visual rendering does not call the backend directly, and backend events do not directly resize or move the native window.

## 7. Tray and desktop behavior

The tray menu contains `显示/隐藏`, `刷新额度`, `始终置顶`, and `退出`. Always-on-top defaults to enabled and is persisted. Showing the widget restores it to a visible safe position. Exiting waits for the supervised app-server process to shut down and be reaped.

## 8. Verification

Automated interaction tests must verify:

- Hover shorter than 150ms does not expand; sustained hover expands.
- Hover only exposes the five-hour face.
- Click toggles the weekly face and never mirrors text.
- Leave for 200ms restores front and collapses.
- A drag over 6px never triggers flip.
- Re-entry cancels pending collapse.
- Quota updates preserve the current face and animation state.
- Reduced motion replaces rotation with crossfade.
- Edge clamping keeps the capsule visible.
- Offline, cache, missing-weekly, and login states never fabricate quota values.

Windows verification covers 100%, 125%, 150%, and 200% DPI; multiple monitors; removed monitor recovery; dark and light wallpapers; always-on-top toggling; tray restore; restart position restore; frameless hit regions; and packaged installation.

End-to-end tests use a fake app-server to cover initialization, login, full quota reads, sparse updates, disconnect, cache restore, backoff, reconnect, and shutdown. Final manual acceptance demonstrates the three-state sequence near the upper-right of a realistic dark Windows desktop and confirms free dragging anywhere in the work area.
