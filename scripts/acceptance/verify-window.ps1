param(
  [Parameter(Mandatory = $false)]
  [ValidateSet("Standard", "Weekly")]
  [string] $Variant = "Standard"
)

$ErrorActionPreference = "Stop"

$checks = if ($Variant -eq "Weekly") {
  [ordered]@{
    "weekly-render-scale" = "native-size Quiet Prism visual with no transform scaling"
    "weekly-collapsed-visible-size" = "74 x 74 orb plus 28 x 42 badge with 10 px overlap"
    "weekly-collapsed-canvas-size" = "104 x 86"
    "weekly-expanded-visible-size" = "123 x 56 capsule plus 22 x 22 badge with 4 px overlap"
    "weekly-expanded-canvas-size" = "153 x 68"
    "weekly-no-flip" = "click preserves the single weekly face"
    "weekly-no-five-hour-content" = "visible and accessible content absent"
    "weekly-badge-right-visible" = "violet badge fully visible on the right in both states"
    "weekly-native-title" = "empty so title text cannot bleed through transparent pixels"
    "hover-delay-ms" = "150"
    "expand-duration-ms" = "280"
    "leave-grace-ms" = "200"
    "drag-threshold-px" = "greater than 3"
  }
} else {
  [ordered]@{
    "collapsed-visible-size" = "148 x 148"
    "collapsed-canvas-size" = "172 x 172"
    "capsule-visible-size" = "245 x 112"
    "expanded-canvas-size" = "269 x 136"
    "hover-delay-ms" = "150"
    "expand-duration-ms" = "350"
    "flip-duration-ms" = "450"
    "leave-grace-ms" = "200"
    "drag-threshold-px" = "greater than 6"
  }
}

$checks.GetEnumerator() | ForEach-Object {
  Write-Host ("{0}: {1}" -f $_.Key, $_.Value)
}

Write-Host "$Variant manual measurement required: verify with Windows screenshots."
