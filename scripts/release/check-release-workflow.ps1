$ErrorActionPreference = 'Stop'

$repositoryRoot = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$validator = Join-Path $repositoryRoot 'scripts\release\validate-release-workflow.mjs'
$workflow = Join-Path $repositoryRoot '.github\workflows\release.yml'

& node $validator $workflow
if ($LASTEXITCODE -ne 0) {
  exit $LASTEXITCODE
}

Write-Host 'Release workflow contract passed.'
