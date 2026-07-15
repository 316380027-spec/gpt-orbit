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
    if (Test-Path -LiteralPath $_ -PathType Leaf) {
      Select-String -LiteralPath $_ -Pattern $pattern -AllMatches
    }
  }

  if ($hits) {
    throw "Public-tree scan matched '$pattern':`n$($hits -join "`n")"
  }
}

Write-Host 'Public-tree scan passed.'
