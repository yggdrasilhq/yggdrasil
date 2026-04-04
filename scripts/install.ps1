$ErrorActionPreference = "Stop"

$repo = if ($env:YGGDRASIL_MAKER_REPO) { $env:YGGDRASIL_MAKER_REPO } else { "yggdrasilhq/yggdrasil" }
$release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest"
$tag = $release.tag_name
if (-not $tag) {
    throw "Failed to resolve latest release tag."
}

$version = $tag.TrimStart("v")
$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
switch ($arch) {
    "x64" { $targetLabel = "windows-x86_64" }
    "arm64" { $targetLabel = "windows-aarch64" }
    default { throw "Unsupported Windows architecture: $arch" }
}

$installRoot = if ($env:YGGDRASIL_MAKER_INSTALL_ROOT) {
    $env:YGGDRASIL_MAKER_INSTALL_ROOT
} else {
    Join-Path $env:LOCALAPPDATA "yggdrasil-maker\direct"
}

$archiveUrl = "https://github.com/$repo/releases/download/$tag/yggdrasil-maker-$targetLabel.zip"
$checksumUrl = "$archiveUrl.sha256"
$tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("yggdrasil-maker-" + [guid]::NewGuid())
$archivePath = Join-Path $tmpDir "yggdrasil-maker.zip"
$checksumPath = Join-Path $tmpDir "yggdrasil-maker.zip.sha256"
$versionDir = Join-Path $installRoot "versions\$version"
$binDir = Join-Path $installRoot "bin"
$launcherPath = Join-Path $binDir "yggdrasil-maker.ps1"
$cmdLauncherPath = Join-Path $binDir "yggdrasil-maker.cmd"
$binaryPath = Join-Path $versionDir "yggdrasil-maker.exe"
$statePath = Join-Path $installRoot "install-state.json"

New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null
New-Item -ItemType Directory -Force -Path $versionDir | Out-Null

Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath
Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath

$expected = (Get-Content $checksumPath -Raw).Split(" ", [System.StringSplitOptions]::RemoveEmptyEntries)[0].Trim()
$actual = (Get-FileHash -Algorithm SHA256 -Path $archivePath).Hash.ToLowerInvariant()
if ($expected.ToLowerInvariant() -ne $actual) {
    throw "Checksum verification failed."
}

Expand-Archive -Path $archivePath -DestinationPath $versionDir -Force
if (-not (Test-Path $binaryPath)) {
    throw "Archive did not contain yggdrasil-maker.exe"
}

$wrapper = @"
`$target = "$binaryPath"
& `$target @args
"@
New-Item -ItemType Directory -Force -Path (Split-Path $launcherPath -Parent) | Out-Null
Set-Content -Path $launcherPath -Value $wrapper -Encoding utf8
$cmdWrapper = "@echo off`r`n`"$binaryPath`" %*`r`n"
Set-Content -Path $cmdLauncherPath -Value $cmdWrapper -Encoding ascii
Set-Content -Path $statePath -Value (@{
    active_version = $version
    active_executable = $binaryPath
} | ConvertTo-Json) -Encoding utf8

Write-Host "Installed yggdrasil-maker $version"
Write-Host "Launchers:"
Write-Host "  $launcherPath"
Write-Host "  $cmdLauncherPath"
