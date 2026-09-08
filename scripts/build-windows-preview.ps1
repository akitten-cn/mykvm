$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
  throw "Windows preview requires a native Windows runner."
}

npm.cmd ci
if ($LASTEXITCODE -ne 0) {
  throw "npm ci failed with exit code $LASTEXITCODE"
}

npm.cmd exec tauri -- build --bundles nsis --no-sign --ci
if ($LASTEXITCODE -ne 0) {
  throw "Tauri NSIS build failed with exit code $LASTEXITCODE"
}

$BundleDir = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle\nsis"
$Artifacts = @(Get-ChildItem -LiteralPath $BundleDir -Filter "*.exe" -File)
if ($Artifacts.Count -eq 0) {
  throw "No NSIS executable was generated in $BundleDir"
}

$ChecksumPath = Join-Path $BundleDir "SHA256SUMS"
$Checksums = foreach ($Artifact in $Artifacts) {
  $Hash = (Get-FileHash -LiteralPath $Artifact.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  "$Hash  $($Artifact.Name)"
}
[IO.File]::WriteAllLines($ChecksumPath, $Checksums)

foreach ($Artifact in $Artifacts) {
  Write-Host "Created unsigned Windows preview: $($Artifact.FullName)"
}
Write-Host "Created checksums: $ChecksumPath"
