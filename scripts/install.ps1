# Dimi installer for Windows (PowerShell) — downloads the latest release from
# GitHub and installs it into ~\.dimi\bin. Usage:
#
#   irm https://github.com/zzj3720/dimi/releases/latest/download/install.ps1 | iex
#
$ErrorActionPreference = "Stop"

$DimiHome = if ($env:DIMI_HOME) { $env:DIMI_HOME } else { Join-Path $HOME ".dimi" }
$ReleaseBase = "https://github.com/zzj3720/dimi/releases/latest/download"
$ManifestUrl = "$ReleaseBase/manifest.json"

$os = "win32"
$arch = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
  "X64" { "x64" }
  "Arm64" { "arm64" }
  default { throw "Unsupported architecture: $_" }
}
$target = "$os-$arch"

Write-Host "Downloading manifest: $ManifestUrl"
$manifest = Invoke-RestMethod -Uri $ManifestUrl

if (-not $manifest.platforms.$target) {
  throw "No build for $target in manifest"
}

$filename = $manifest.platforms.$target.filename
$checksum = $manifest.platforms.$target.checksum
$zipUrl = "$ReleaseBase/$filename"
$zipPath = Join-Path ([System.IO.Path]::GetTempPath()) $filename

Write-Host "Downloading: $zipUrl"
Invoke-WebRequest -Uri $zipUrl -OutFile $zipPath

Write-Host "Verifying sha256..."
$actual = (Get-FileHash -Algorithm SHA256 -Path $zipPath).Hash.ToLower()
if ($actual -ne $checksum) {
  throw "Checksum mismatch: expected $checksum, got $actual"
}

$binDir = Join-Path $DimiHome "bin"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Write-Host "Extracting to $binDir..."
Expand-Archive -Path $zipPath -DestinationPath $binDir -Force

Remove-Item $zipPath -Force

Write-Host ""
Write-Host "Installed Dimi to $binDir\dimi.exe"
Write-Host "Add it to your PATH:"
Write-Host "  `$env:PATH = `"`$env:USERPROFILE\.dimi\bin;`$env:PATH`""
