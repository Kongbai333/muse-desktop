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
    $coreDirectory = Join-Path $repo 'target/muse-kernel'
    & (Join-Path $repo 'kernel/build.ps1') -OutputDirectory $coreDirectory
    Copy-Item -LiteralPath (Join-Path $coreDirectory 'mihomo.exe') -Destination (Join-Path $staging 'mihomo.exe')
    $sourceDirectory = [IO.Directory]::CreateDirectory((Join-Path $staging 'kernel')).FullName
    foreach ($file in @('README.md', 'build.ps1', 'mihomo-source.zip', 'changes.patch')) {
        Copy-Item -LiteralPath (Join-Path $repo "kernel/$file") -Destination (Join-Path $sourceDirectory $file)
    }
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

从 v0.1.10 升级：旧版应用内更新不能安装此完整包。退出旧版，将整个 ZIP
解压到新的程序目录，再运行 Muse.exe。不要只复制 Muse.exe，也不要移动或删除
原数据目录；若原来指定了 --data-dir，继续使用同一参数。备份原数据后操作。

Bundled modified Mihomo is for account routing. No Windows system TUN driver
is included or supported. See kernel/README.md for sources and rebuilding.
No account data, cookies, proxy credentials or signing key is distributed.
"@
    [IO.File]::WriteAllText((Join-Path $staging 'README.txt'), $readme, [Text.UTF8Encoding]::new($false))
    $zip = Join-Path $output ($name + '.zip')
    Compress-Archive -LiteralPath $staging -DestinationPath $zip -Force
    $scanner = Join-Path $PSScriptRoot 'check-driver-free.py'
    if (Get-Command py -ErrorAction SilentlyContinue) { & py -3 -B $scanner $zip }
    else { & python -B $scanner $zip }
    if ($LASTEXITCODE -ne 0) { throw 'Portable ZIP failed the driver-free audit.' }
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
