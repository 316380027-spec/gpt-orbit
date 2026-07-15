# Gpt Orbit Windows Acceptance Matrix

Status legend: PASS, FAIL, BLOCKED, NOT RUN.

| Area | Check | Status | Evidence |
| --- | --- | --- | --- |
| Interaction | Standard default collapsed orb shows only `5H LEFT`, percent, and countdown | PASS | Automated frontend tests |
| Interaction | Standard hover waits 150ms and expands to five-hour capsule | PASS | Automated frontend tests |
| Interaction | Standard click flips expanded capsule to weekly face | PASS | Automated frontend tests |
| Interaction | Weekly has one face and click does not flip | PASS | Automated frontend tests; `verify-window.ps1 -Variant Weekly` |
| Interaction | Weekly visible and accessible UI contains no five-hour quota | PASS | Automated frontend tests; weekly capture checklist |
| Interaction | Leave waits 200ms and collapses | PASS | Automated frontend tests |
| Interaction | Movement greater than 6px (Standard) or 3px (Weekly) starts drag and suppresses click action | PASS | Automated frontend tests |
| Window | Standard visible geometry is 148 x 148 collapsed and 245 x 112 expanded | PASS | CSS constants and window tests |
| Window | Weekly native Quiet Prism geometry uses a 74 x 74 orb and 123 x 56 capsule with violet satellite badges | PASS | CSS contract tests and weekly capture checklist |
| Window | Weekly native canvas is 104 x 86 collapsed and 153 x 68 expanded | PASS | Rust and frontend geometry tests |
| Window | Weekly native title stays empty so transparent pixels cannot reveal title text | PASS | Config integration test, runtime restore code, installed capture, and three cold-start probes |
| Window | Right-edge expansion stays within monitor work area | PASS | Geometry tests |
| Window | Restores removed monitor placement to primary monitor | PASS | Geometry tests |
| Window | DPI conversion covered at 100/125/150/200% | PASS | Geometry tests |
| Desktop | Always-on-top defaults enabled and can be toggled | PASS | Rust tray/lifecycle tests |
| Desktop | Tray show restores/clamps widget before showing | PASS | Rust implementation and review |
| Desktop | Tray quit saves placement and waits for backend shutdown | PASS | Rust implementation and review |
| Backend | Live quota, sparse update, malformed recovery, reconnect | PASS | Rust app-server flow tests |
| Backend | Login URL remains safe and controlled | PASS | Bridge and app-server tests |
| Backend | Live count, explicit zero, malformed/oversized rejection, redirect rejection, and recovery | PASS | `reset_credit_flow` loopback HTTP subprocess + production service integration |
| Stale fallback | Disconnect preserves count `3` as stale across production service current state and events; recovery publishes and caches `2` | PASS | Real `ResetCreditService` + `ResetCreditClient` + `ResetCreditCache` against one Node loopback sequence |
| Data privacy | Automated reset-credit tests use temp `CODEX_HOME`, loopback only, header names only, and synthetic credentials | PASS | `reset_credit_flow` source and loopback integration assertions |
| Accessibility | Reduced motion replaces standard 3D flip with crossfade | PASS | Frontend tests |
| Visual | Weekly collapsed state is readable, unclipped, and has no native title bleed-through | PASS | Installed runtime passive screen-bounds capture: `docs/acceptance/screenshots/weekly-collapsed.png` (104 x 86, 18,068 bytes) |
| Visual | Weekly expanded state has readable Chinese, no five-hour content, and no clipping | NOT RUN | Installed expanded-window capture was not completed; no public expanded screenshot is retained |
| Visual | Weekly violet badge remains fully visible on the right in the collapsed state | PASS | Installed collapsed capture retains the complete right badge |
| Visual | Light wallpaper readability | NOT RUN | Manual capture |
| Monitor | Mixed-DPI physical display test | NOT RUN | Manual Windows matrix |
| Two installer identities | Current-user standard and weekly NSIS packages have distinct product, binary, and application identifiers | PASS | Config integration tests and final installed paths: `Gpt Orbit/codex-orbit.exe` and `Gpt Orbit Weekly/gpt-orbit-weekly.exe` |
| Simultaneous process | Installed `codex-orbit.exe` and `gpt-orbit-weekly.exe` run concurrently | PASS | Final HEAD installed processes remained responsive concurrently for more than 12 seconds |
| Independent placement | Standard and weekly widgets use independent saved placement | PASS | Separate identifiers and preference tests; Weekly restored at 104 x 86 |
| Installer | Standard current-user NSIS installer produced | PASS | Release asset `Gpt Orbit_0.1.0_x64-setup.exe`; 3,216,034 bytes; SHA-256 `F619E2B2B0038C56F50F9887E06CB1A6D05FFED24290C49CBC70E515741E77D3` |
| Installer | Weekly current-user NSIS installer produced | PASS | Release asset `Gpt.Orbit.Weekly_0.1.0_x64-setup.exe`; 3,221,444 bytes; SHA-256 `8274ADBEF4E802B255EC0C03E7CDBD14AC4B6B561C4EF8BC06E2DEC13A7E44AF` |
| Installer | Upgrade preserves placement and preferences | PASS | Silent current-user upgrade over the earlier packages retained separate Standard and Weekly position/topmost preference files |
| Installer | Uninstall removes app/autostart without touching Codex credentials | NOT RUN | Manual install matrix |
| Live comparison | Weekly badge matches a one-shot GET whose stdout is the `available_count` integer only | PASS | Final installed accessibility exposed badge integer `3`; one GET with redirects disabled, 10s timeout, and 64KiB cap printed only integer `3` |

Acceptance commands:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance/verify-window.ps1 -Variant Standard
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance/verify-window.ps1 -Variant Weekly
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance/capture-widget.ps1 -Variant Weekly -CaptureInstalledCollapsed
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/acceptance/capture-widget.ps1 -Variant Weekly -CaptureInstalledExpanded
```

Notes:

- Do not place tokens, account IDs, credit IDs, raw responses, header values, email addresses, auth URLs, browser content, or conversation content in screenshots or logs.
- The installed collapsed screenshot is `weekly-collapsed.png`; installed expanded-window capture is NOT RUN and no public expanded screenshot is retained.
- The live comparison is GET-only. If the endpoint rejects the request, record only the reason category and do not relax TLS, redirect, or host restrictions.
- Weekly preferences are stored under the independent `com.codex-orbit.weekly` identifier.
- Direct WGC screen capture was not supported on this Windows 10.0.19043 host. Computer Use returned `SetIsBorderRequired failed: 不支持此接口 (0x80004002)` for application and system-tool windows, so input stopped per guidance.
- `weekly-collapsed.png` is installed-runtime evidence captured passively from the known process window bounds; no pointer or keyboard input was sent.
