param(
  [Parameter(Mandatory = $false)]
  [string] $OutputDirectory = "docs/acceptance/screenshots",
  [Parameter(Mandatory = $false)]
  [ValidateSet("Standard", "Weekly")]
  [string] $Variant = "Weekly",
  [Parameter(Mandatory = $false)]
  [switch] $CaptureInstalledCollapsed,
  [Parameter(Mandatory = $false)]
  [switch] $CaptureInstalledExpanded
)

$ErrorActionPreference = "Stop"
if ($CaptureInstalledCollapsed -and $CaptureInstalledExpanded) {
  throw "Choose only one installed capture state"
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null

$notes = if ($Variant -eq "Weekly") { @"
Gpt Orbit Weekly capture checklist
- Place the widget near the upper-right corner of a realistic dark Windows desktop.
- Capture the native-size Quiet Prism orb and badge inside the 104 x 86 canvas.
- Capture the native-size Quiet Prism capsule and badge inside the 153 x 68 canvas.
- The weekly widget has one face: five-hour content must be absent and click must not flip it.
- Confirm the violet badge remains fully visible on the right in both states.
- Confirm no native window title is visible through transparent pixels.
- Do not include account emails, auth URLs, tokens, browser content, or conversation content.
- Save the installed collapsed image as weekly-collapsed.png. If an installed expanded capture is completed later, save it as weekly-installed-expanded.png; the installed expanded capture remains NOT RUN and no installed expanded image is currently retained.
"@
} else { @"
Gpt Orbit Standard capture checklist
- Place the widget near the upper-right corner of a realistic dark Windows desktop.
- Capture the collapsed orb and expanded five-hour capsule.
- Do not include account emails, auth URLs, tokens, browser content, or conversation content.
- Save images as standard-collapsed.png and standard-expanded.png in this directory.
"@
}

$notes | Set-Content -Path (Join-Path $OutputDirectory "capture-checklist.txt") -Encoding UTF8

if ($CaptureInstalledCollapsed -or $CaptureInstalledExpanded) {
  Add-Type -AssemblyName System.Drawing
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class NativeWindowCapture {
  [StructLayout(LayoutKind.Sequential)]
  public struct Rect {
    public int Left;
    public int Top;
    public int Right;
    public int Bottom;
  }

  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr window, out Rect rect);

}
'@

  $processName = if ($Variant -eq "Weekly") { "gpt-orbit-weekly" } else { "codex-orbit" }
  $installedDirectory = if ($Variant -eq "Weekly") { "Gpt Orbit Weekly" } else { "Gpt Orbit" }
  $installedBinary = if ($Variant -eq "Weekly") { "gpt-orbit-weekly.exe" } else { "codex-orbit.exe" }
  $expectedExecutable = Join-Path $env:LOCALAPPDATA (Join-Path $installedDirectory $installedBinary)
  $process = Get-Process -Name $processName -ErrorAction Stop |
    Where-Object {
      $_.MainWindowHandle -ne [IntPtr]::Zero -and
      [string]::Equals($_.Path, $expectedExecutable, [StringComparison]::OrdinalIgnoreCase)
    } |
    Select-Object -First 1
  if ($null -eq $process) {
    throw "$Variant installed process has no main window at the expected path"
  }
  if ($Variant -eq "Weekly" -and -not [string]::IsNullOrEmpty($process.MainWindowTitle)) {
    throw "Weekly installed window must keep its native title empty"
  }

  $rect = New-Object NativeWindowCapture+Rect
  if (-not [NativeWindowCapture]::GetWindowRect($process.MainWindowHandle, [ref] $rect)) {
    throw "$Variant installed window bounds are unavailable"
  }
  $width = $rect.Right - $rect.Left
  $height = $rect.Bottom - $rect.Top
  if ($width -le 0 -or $height -le 0) {
    throw "$Variant installed window bounds are empty"
  }
  $expectedWidth = if ($CaptureInstalledExpanded) {
    if ($Variant -eq "Weekly") { 153 } else { 269 }
  } else {
    if ($Variant -eq "Weekly") { 104 } else { 172 }
  }
  $expectedHeight = if ($Variant -eq "Weekly") {
    if ($CaptureInstalledExpanded) { 68 } else { 86 }
  } else {
    if ($CaptureInstalledExpanded) { 136 } else { 172 }
  }
  if ($width -ne $expectedWidth -or $height -ne $expectedHeight) {
    $state = if ($CaptureInstalledExpanded) { "expanded" } else { "collapsed" }
    throw "$Variant installed $state window has unexpected dimensions"
  }

  $bitmap = New-Object System.Drawing.Bitmap($width, $height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    # Capture only the known widget bounds. This preserves the composited transparent
    # WebView pixels that PrintWindow omits on some Windows builds.
    $sourcePoint = New-Object System.Drawing.Point($rect.Left, $rect.Top)
    $graphics.CopyFromScreen($sourcePoint, [System.Drawing.Point]::Empty, $bitmap.Size)

    $fileName = if ($CaptureInstalledExpanded) {
      if ($Variant -eq "Weekly") { "weekly-installed-expanded.png" } else { "standard-installed-expanded.png" }
    } else {
      if ($Variant -eq "Weekly") { "weekly-collapsed.png" } else { "standard-collapsed.png" }
    }
    $outputPath = Join-Path $OutputDirectory $fileName
    $bitmap.Save($outputPath, [System.Drawing.Imaging.ImageFormat]::Png)
  } finally {
    $graphics.Dispose()
    $bitmap.Dispose()
  }

  $saved = Get-Item -LiteralPath $outputPath
  if ($saved.Length -le 100) {
    throw "$Variant installed window capture is unexpectedly small"
  }
  Write-Host ("{0} installed window captured passively: {1} ({2} x {3}, {4} bytes)" -f $Variant, $saved.FullName, $width, $height, $saved.Length)
}

Write-Host "$Variant capture checklist written to $OutputDirectory"
