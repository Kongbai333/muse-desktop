param(
    [string]$BinaryPath = (Join-Path $PSScriptRoot '../target/release/muse-desktop.exe'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../dist'),
    [string]$Tag = ''
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).ProviderPath
$manifest = Get-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Raw
$version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?)"').Groups[1].Value
if (!$version) { throw 'Cannot determine the workspace version.' }
if ($Tag -and $Tag -cne "v$version") { throw "Release tag $Tag does not match source version v$version." }
if (!(Test-Path -LiteralPath $BinaryPath -PathType Leaf)) { throw "Executable not found: $BinaryPath" }

$null = New-Item -ItemType Directory -Path $OutputDirectory -Force
$output = (Resolve-Path -LiteralPath $OutputDirectory).ProviderPath
$name = "Muse-Desktop-$version-win-x64"
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$scratch = Join-Path $tempRoot ('muse-package-' + [guid]::NewGuid().ToString('N'))
$staging = Join-Path $scratch $name
$null = New-Item -ItemType Directory -Path $staging -Force
try {
    Copy-Item -LiteralPath $BinaryPath -Destination (Join-Path $staging 'Muse.exe')
    foreach ($file in @('LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md')) {
        Copy-Item -LiteralPath (Join-Path $repo $file) -Destination (Join-Path $staging $file)
    }
    Copy-Item -LiteralPath (Join-Path $repo 'LICENSES') -Destination (Join-Path $staging 'LICENSES') -Recurse
    $readme = @"
Muse Desktop $version - Windows x64

Extract this ZIP and run Muse.exe. Windows 10/11 x64 and Microsoft Edge WebView2
Evergreen Runtime are required. This portable package does not require .NET.

Account profiles and generated media are stored under
%LOCALAPPDATA%\MuseDesktopNext by default. The --data-dir argument selects an
existing data directory. Do not delete your data directory when upgrading.

This archive contains application files and license notices only. It does not
contain account data, cookies, proxy credentials or a signing key.
"@
    [IO.File]::WriteAllText((Join-Path $staging 'README.txt'), $readme, [Text.UTF8Encoding]::new($false))
    $zip = Join-Path $output ($name + '.zip')
    Compress-Archive -LiteralPath $staging -DestinationPath $zip -Force
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText(($zip + '.sha256'), "$hash  $name.zip`n", [Text.UTF8Encoding]::new($false))
    Write-Output $zip
    Write-Output ($zip + '.sha256')
} finally {
    $resolved = [IO.Path]::GetFullPath($scratch)
    if (!$resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
        !(Split-Path -Leaf $resolved).StartsWith('muse-package-')) { throw 'Unsafe package cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
