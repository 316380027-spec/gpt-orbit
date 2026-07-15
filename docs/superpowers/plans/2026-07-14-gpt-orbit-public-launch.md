# Gpt Orbit Public Launch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish Gpt Orbit as a safe public GitHub project with a downloadable Windows release, then deliver platform-specific Xiaohongshu and Douyin covers, copy, and posting instructions.

**Architecture:** Keep the validated Tauri application as the product source of truth, add a small public-release documentation and verification layer around it, and publish the Windows installer as a GitHub Release asset rather than as a tracked binary. Social assets reuse the real product capture inside generated marketing backgrounds, with exact Chinese text composited deterministically after image generation.

**Tech Stack:** Tauri 2, Rust, React 19, TypeScript, Vite 6, Vitest 3, GitHub Actions, `tauri-apps/tauri-action`, PowerShell, built-in image generation, Pillow for final text compositing.

## Global Constraints

- Repository name is `gpt-orbit`, visibility is public, and the default branch is `main`.
- Source and Windows installer are both public; the installer is uploaded only as a GitHub Release asset.
- License is MIT.
- The first public version is `v0.1.0` and the release title is `Gpt Orbit v0.1.0`.
- Public copy must say this is a personal, unofficial project with no official affiliation to OpenAI.
- Public files must not contain credentials, tokens, account information, login URLs, local absolute paths, build output, caches, or debug logs.
- The marketing direction is product-led: use the real Gpt Orbit widget, restrained dark Windows context, blue-indigo glass, and no OpenAI logo.
- Xiaohongshu cover is 3:4 with exact title `我做了个 Codex 额度悬浮球`, subtitle `周额度终于能一眼看懂了`, and label `Windows · 免费开源`.
- Douyin cover is 9:16 with exact title `Codex 周额度悬浮球` and label `Windows 免费开源`.
- Do not claim unexecuted Windows, DPI, uninstall, or multi-monitor validation.

---

### Task 1: Freeze the validated product state

**Files:**
- Modify: existing dirty files listed by `git status --short`
- Exclude: `output/`
- Verify: `docs/acceptance/release-manifest.md`

**Interfaces:**
- Consumes: validated installed Weekly build and existing 109 frontend / 149 Rust test evidence.
- Produces: one committed source revision that the public release can point to.

- [ ] **Step 1: Inspect the exact dirty scope**

Run:

```powershell
git status --short
git diff --stat
git diff --check
```

Expected: only the final Weekly widget, acceptance documentation, and release-verification changes are present; `output/` remains untracked.

- [ ] **Step 2: Run the frontend regression suite**

Run:

```powershell
npm test -- --run
```

Expected: 16 files and 109 tests pass.

- [ ] **Step 3: Run the Rust regression suite**

Run:

```powershell
npm run rust:test
```

Expected: all Rust unit and integration tests pass with no failed tests.

- [ ] **Step 4: Build the public Weekly bundle**

Run:

```powershell
npm run build:weekly
```

Expected: TypeScript and Vite complete successfully and produce `dist/`.

- [ ] **Step 5: Commit only the validated product changes**

Run:

```powershell
git add -- docs/acceptance scripts/acceptance src src-tauri
git status --short
git commit -m "release: finalize Gpt Orbit Weekly v0.1.0"
```

Expected: tracked product changes and the two new source files are committed; `output/` is not staged.

---

### Task 2: Add public repository documentation and hygiene checks

**Files:**
- Create: `README.md`
- Create: `LICENSE`
- Create: `SECURITY.md`
- Create: `scripts/release/check-public-tree.ps1`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: final screenshot at `docs/acceptance/screenshots/weekly-collapsed.png` and package scripts from `package.json`.
- Produces: public repository landing page, MIT terms, disclosure guidance, and a failing-on-leak hygiene verifier.

- [ ] **Step 1: Write the public-tree verifier before the documentation**

Create `scripts/release/check-public-tree.ps1` with this behavior:

```powershell
$ErrorActionPreference = 'Stop'
$tracked = git ls-files
$forbiddenTracked = $tracked | Where-Object {
  $_ -match '(^|/)(target|dist|node_modules|output)(/|$)' -or
  $_ -match '(^|/)\.env(?:\.[^/]+)?(/|$)' -or
  $_ -match '\.(exe|msi|pdb|log)$'
}
if ($forbiddenTracked) {
  throw "Forbidden tracked release files:`n$($forbiddenTracked -join "`n")"
}

$patterns = @(
  'OPENAI_API_KEY\s*=',
  'sk-(?:proj-)?[A-Za-z0-9_-]{20,}',
  'gh[pousr]_[A-Za-z0-9_]{20,}',
  'github_pat_[A-Za-z0-9_]{20,}',
  '(?<![A-Za-z0-9])[A-Za-z]:(?:\\{1,2}|/)(?![\\/])[^\r\n''""``]*'
)
$textFiles = $tracked | Where-Object {
  $_ -notmatch '\.(png|jpg|jpeg|gif|ico|icns|lock)$'
}
foreach ($pattern in $patterns) {
  $hits = $textFiles | ForEach-Object {
    if (Test-Path -LiteralPath $_) {
      Select-String -LiteralPath $_ -Pattern $pattern -AllMatches
    }
  }
  if ($hits) {
    throw "Public-tree scan matched '$pattern':`n$($hits -join "`n")"
  }
}
Write-Host 'Public-tree scan passed.'
```

- [ ] **Step 2: Run the verifier and record the expected initial failure**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/release/check-public-tree.ps1
```

Expected: FAIL if existing tracked acceptance documentation exposes local installer paths; the matched files are the cleanup scope.

- [ ] **Step 3: Write the public landing documents**

`README.md` must contain, in order: title and unofficial badge, screenshot, release download link, features, Windows 11 requirements, installation, privacy/data source, development commands, verified coverage, known unverified scenarios, and MIT license link.

`SECURITY.md` must instruct users not to post login URLs, tokens, account identifiers, or full app-server payloads in public issues, and must give a private contact placeholder-free method through GitHub Security Advisories.

`LICENSE` must contain the standard MIT License text with copyright year `2026` and copyright holder `Gpt Orbit contributors`.

- [ ] **Step 4: Remove public-path leaks and extend ignores**

Add these entries to `.gitignore` if missing:

```gitignore
node_modules/
dist/
target/
output/
.env
.env.*
*.log
```

Replace absolute installer paths in tracked acceptance docs with release-relative names such as `Gpt.Orbit.Weekly_0.1.0_x64-setup.exe`, while preserving the verified byte count and SHA-256.

- [ ] **Step 5: Run public hygiene and documentation checks**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/release/check-public-tree.ps1
git diff --check
```

Expected: `Public-tree scan passed.` and no whitespace errors.

- [ ] **Step 6: Commit the public repository layer**

Run:

```powershell
git add -- README.md LICENSE SECURITY.md .gitignore scripts/release docs/acceptance
git commit -m "docs: prepare public Gpt Orbit repository"
```

---

### Task 3: Add the Windows release workflow

**Files:**
- Create: `.github/workflows/release.yml`
- Create: `scripts/release/check-release-workflow.ps1`

**Interfaces:**
- Consumes: `npm ci`, `npm test -- --run`, `npm run rust:test`, and `npm run tauri:build:weekly`.
- Produces: a tag-triggered GitHub workflow that creates a draft release and uploads the NSIS package.

- [ ] **Step 1: Write a workflow contract check**

Create `scripts/release/check-release-workflow.ps1` that reads `.github/workflows/release.yml` and fails unless all of these literal contracts exist:

```powershell
$workflow = Get-Content -Raw '.github/workflows/release.yml'
$required = @(
  'tags:',
  "'v*'",
  'contents: write',
  'windows-latest',
  'npm ci',
  'npm test -- --run',
  'npm run rust:test',
  'tauri-apps/tauri-action@v1',
  '--config src-tauri/tauri.weekly.conf.json',
  'releaseDraft: true'
)
foreach ($item in $required) {
  if (-not $workflow.Contains($item)) { throw "Missing workflow contract: $item" }
}
Write-Host 'Release workflow contract passed.'
```

- [ ] **Step 2: Run the contract check before creating the workflow**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/release/check-release-workflow.ps1
```

Expected: FAIL because `.github/workflows/release.yml` does not exist.

- [ ] **Step 3: Create the release workflow**

Create `.github/workflows/release.yml` with one `windows-latest` job. Grant `contents: write`; check out the repository; install Node 22 and stable Rust; run `npm ci`, the frontend suite, the Rust suite, and `tauri-apps/tauri-action@v1`. Configure the action with `tagName: v__VERSION__`, `releaseName: Gpt Orbit v__VERSION__`, `releaseDraft: true`, `prerelease: false`, and `args: --config src-tauri/tauri.weekly.conf.json`.

- [ ] **Step 4: Validate the workflow contract**

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/release/check-release-workflow.ps1
```

Expected: `Release workflow contract passed.`

- [ ] **Step 5: Commit the workflow**

Run:

```powershell
git add -- .github/workflows/release.yml scripts/release/check-release-workflow.ps1
git commit -m "ci: add Windows release workflow"
```

---

### Task 4: Create and verify the public GitHub repository

**Files:**
- No repository file changes expected.

**Interfaces:**
- Consumes: the clean local branch and an authenticated GitHub account.
- Produces: public `gpt-orbit` repository with `main` as its default branch.

- [ ] **Step 1: Ensure GitHub CLI is available**

Run:

```powershell
gh --version
```

If unavailable, install GitHub CLI from its official WinGet package, reopen the command environment, and rerun the check:

```powershell
winget install --id GitHub.cli --exact --source winget
```

Expected: a GitHub CLI version is printed.

- [ ] **Step 2: Authenticate through GitHub's browser flow**

Run:

```powershell
gh auth status
```

If not authenticated, run `gh auth login --web --git-protocol https`, complete the browser confirmation, and rerun `gh auth status`.

- [ ] **Step 3: Create the public repository and push**

Rename the local publication branch to `main` only after preserving the existing local development branch reference, then run:

```powershell
gh repo create gpt-orbit --public --source . --remote origin --push --description "A lightweight Windows desktop widget for viewing Codex weekly quota."
```

Expected: GitHub returns the public repository URL and `origin/main` is tracking the local publication branch.

- [ ] **Step 4: Verify repository visibility and default branch**

Run:

```powershell
gh repo view --json nameWithOwner,visibility,defaultBranchRef,url
git remote -v
git status -sb
```

Expected: `visibility` is `PUBLIC`, default branch is `main`, and the worktree is clean except for intentionally untracked generated social sources.

---

### Task 5: Publish `v0.1.0` and verify the installer

**Files:**
- Create: `docs/releases/v0.1.0.md`
- Read: `$env:GPT_ORBIT_RELEASE_DIR\Gpt Orbit Weekly_0.1.0_x64-setup.exe`

**Interfaces:**
- Consumes: verified local installer and public GitHub repository.
- Produces: public GitHub Release with installer and checksum.

- [ ] **Step 1: Verify the local release asset**

Run:

```powershell
$installer = Join-Path $env:GPT_ORBIT_RELEASE_DIR 'Gpt Orbit Weekly_0.1.0_x64-setup.exe'
Get-Item -LiteralPath $installer | Select-Object FullName,Length
Get-FileHash -LiteralPath $installer -Algorithm SHA256
```

Expected: size `3221444` bytes and SHA-256 `8274ADBEF4E802B255EC0C03E7CDBD14AC4B6B561C4EF8BC06E2DEC13A7E44AF`.

- [ ] **Step 2: Write release notes**

Create `docs/releases/v0.1.0.md` with sections: highlights, Windows 11 installation, verified checks, known unverified scenarios, SHA-256, privacy, and the unofficial-project disclaimer. Use the exact asset name and checksum from Step 1.

- [ ] **Step 3: Commit release notes and create the release**

Run:

```powershell
git add -- docs/releases/v0.1.0.md
git commit -m "docs: add v0.1.0 release notes"
git push origin main
gh release create v0.1.0 --title "Gpt Orbit v0.1.0" --notes-file docs/releases/v0.1.0.md --draft "$installer"
```

Expected: a draft release URL is returned with one `.exe` asset.

- [ ] **Step 4: Inspect and publish the release**

Run:

```powershell
gh release view v0.1.0 --json url,isDraft,assets,tagName
gh release edit v0.1.0 --draft=false
gh release view v0.1.0 --json url,isDraft,assets,tagName
```

Expected: final `isDraft` is `false`, `tagName` is `v0.1.0`, and the installer asset size is `3221444`.

- [ ] **Step 5: Verify the public download path**

Open the repository and release URLs without relying on a signed-in API response. Confirm README rendering, screenshot rendering, release visibility, asset name, and downloadable response.

---

### Task 6: Produce the Xiaohongshu and Douyin covers

**Files:**
- Create: `assets/social/xiaohongshu-cover.png`
- Create: `assets/social/douyin-cover.png`
- Create: `assets/social/source/` generated background sources

**Interfaces:**
- Consumes: real widget capture `docs/acceptance/screenshots/weekly-collapsed.png`.
- Produces: final 3:4 and 9:16 platform covers with exact Chinese copy.

- [ ] **Step 1: Generate the product-led background for Xiaohongshu**

Use the built-in image generator with the real widget capture as the product reference and this exact brief:

```text
Use case: ads-marketing
Asset type: Xiaohongshu launch cover background, 3:4 portrait
Primary request: create a polished product-led launch visual for a tiny Windows desktop quota widget made by an independent developer
Input image: preserve the real circular blue-indigo widget as the hero product; do not redesign its UI or invent new controls
Scene/backdrop: restrained realistic dark Windows 11 desktop atmosphere with soft depth and subtle window silhouettes
Composition: large clear product hero in the middle-lower area, generous clean space in the upper third for later Chinese typography, small clean footer space
Lighting/mood: refined blue-indigo glass glow, soft cyan edge light, calm premium mood
Constraints: no text, no logos, no watermark, no people, no gaming HUD, no exaggerated neon, no fake interface labels
```

- [ ] **Step 2: Generate the matching Douyin background**

Use the same product reference and visual language at 9:16, with the hero centered inside the middle safe area and clean space above and below for deterministic typography.

- [ ] **Step 3: Composite exact Chinese typography**

Use Pillow with a locally available CJK font. Render the exact approved strings, preserve at least 7% side margins, and add a subtle dark translucent plate only if contrast requires it. The output dimensions must preserve exact ratios and be at least 1242×1656 for Xiaohongshu and 1080×1920 for Douyin.

- [ ] **Step 4: Validate the covers visually and mechanically**

Check: exact text, no invented product UI, readable thumbnail, correct ratio, no watermark, no clipped glyphs, and adequate Douyin top/bottom/right safe areas. Iterate once with one targeted prompt or layout adjustment if any check fails.

- [ ] **Step 5: Commit final cover assets**

Run:

```powershell
git add -- assets/social/xiaohongshu-cover.png assets/social/douyin-cover.png
git commit -m "assets: add social launch covers"
git push origin main
```

---

### Task 7: Deliver platform copy and posting guide

**Files:**
- Create: `docs/social/gpt-orbit-launch-copy.md`

**Interfaces:**
- Consumes: final public repository URL, release URL, and cover filenames.
- Produces: copy-paste-ready Xiaohongshu note, Douyin caption, and posting checklist.

- [ ] **Step 1: Write the Xiaohongshu launch note**

Use the approved structure: pain point, why it was built, four verified functions, Windows requirement, free/open-source statement, GitHub route, unofficial disclaimer, feedback question, and focused hashtags. Do not claim features or validation absent from the current release manifest.

- [ ] **Step 2: Write the Douyin caption**

Use a short conversational hook, three to four verified benefit lines, the free/open-source statement, the exact public GitHub route, unofficial disclaimer, and a final interaction question. Include a separate pinned-comment version for the repository route.

- [ ] **Step 3: Write the posting checklist**

For each platform, specify cover file, recommended post type, title/caption placement, repository link placement according to account capability, pre-publish crop check, and post-publish verification. Do not automate the final social post because the user must review platform account state and the final public content.

- [ ] **Step 4: Check for placeholders and mismatched links**

Run:

```powershell
rg -n 'TBD|TODO|待补|占位|example\.com|YOUR_|<[^>]+>' docs/social/gpt-orbit-launch-copy.md
```

Expected: no matches.

- [ ] **Step 5: Commit and push the launch kit**

Run:

```powershell
git add -- docs/social/gpt-orbit-launch-copy.md
git commit -m "docs: add social launch kit"
git push origin main
```

---

### Task 8: Final public-launch audit

**Files:**
- Verify only; no planned edits.

**Interfaces:**
- Consumes: all prior deliverables.
- Produces: evidence that the requested launch kit is complete.

- [ ] **Step 1: Rerun the local release gates**

Run:

```powershell
npm test -- --run
npm run rust:test
npm run build:weekly
powershell -ExecutionPolicy Bypass -File scripts/release/check-public-tree.ps1
powershell -ExecutionPolicy Bypass -File scripts/release/check-release-workflow.ps1
git status -sb
```

Expected: all checks pass and no intended file is uncommitted.

- [ ] **Step 2: Verify public GitHub state**

Confirm: repository is public, default branch is `main`, README and screenshot render, release `v0.1.0` is public, installer asset is downloadable, asset size and SHA-256 match, and GitHub Actions workflow is visible.

- [ ] **Step 3: Verify launch-kit completeness**

Confirm: both cover files open at their intended ratios; all Chinese text is correct; both platform captions contain real repository/release routes; and posting instructions require no missing user decision before publishing.

- [ ] **Step 4: Hand off the result**

Return the public repository and release links, both local cover links with previews, both copy-paste-ready platform texts, and the concise posting sequence. Explicitly list any GitHub Action run still pending or platform-level action that requires the user's logged-in social account.
