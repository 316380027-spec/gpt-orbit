# Gpt Orbit final release manifest

- Source: the revision referenced by the `v0.1.0` release tag
- Branch: `main`
- Build date: 2026-07-14 (Asia/Shanghai)

| Product | Release asset | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Gpt Orbit Weekly | `Gpt.Orbit.Weekly_0.1.0_x64-setup.exe` | 3,221,444 | `8274ADBEF4E802B255EC0C03E7CDBD14AC4B6B561C4EF8BC06E2DEC13A7E44AF` |

Final Windows checks:

- The current-user Weekly installer completed successfully over the previous package.
- Weekly restored with 104 x 86 collapsed bounds and 153 x 68 expanded bounds.
- Weekly native title is empty in configuration and is cleared again after every restore/show; three consecutive installed cold starts retained an empty title.
- Weekly installed accessibility exposed reset-credit badge integer `3`; an independent GET-only comparison printed only integer `3`.
- The final installed collapsed capture is stored as `docs/acceptance/screenshots/weekly-collapsed.png`.
- Light wallpaper, physical mixed-DPI, uninstall, and installed expanded-window capture remain NOT RUN; no public expanded screenshot is retained and no claim is made for them.

Final automated checks:

- Frontend: 16 files / 110 tests passed, including real Standard/Weekly production bundle isolation, delayed/cancelled Weekly collapse coverage, and locale-independent Chinese reset copy.
- Rust: 102 unit + 47 integration tests passed; clippy and rustfmt checks passed.
- The focused final review found no Critical or Important issue.
