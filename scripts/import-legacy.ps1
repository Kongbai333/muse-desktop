param(
    [UInt64]$LegacyAccountId = 0,
    [string]$SourceProfile,
    [string]$DataDirectory,
    [switch]$NoBuild,
    [switch]$Open
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$oldCargoHome = $env:CARGO_HOME
$oldTarget = $env:CARGO_TARGET_DIR
try {
    $env:CARGO_HOME = Join-Path $projectRoot '.cargo-home'
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    Push-Location -LiteralPath $projectRoot
    try {
        $active = @(Get-Process -Name MuseLauncher,muse-desktop -ErrorAction SilentlyContinue)
        if ($active.Count -gt 0) { throw 'Close the legacy and new Muse windows before importing.' }
        if (-not $NoBuild) { cargo build --locked -p muse-desktop; if ($LASTEXITCODE -ne 0) { throw 'Build failed.' } }
        if (-not $DataDirectory) { $DataDirectory = Join-Path $projectRoot '.runtime/development' }
        $null = New-Item -ItemType Directory -Path $DataDirectory -Force
        $DataDirectory = (Resolve-Path -LiteralPath $DataDirectory).Path
        $appArgs = @('--import-legacy','--data-dir',('"'+$DataDirectory+'"'))
        if (-not $Open) { $appArgs += '--import-only' }
        if ($LegacyAccountId -gt 0) { $appArgs += @('--legacy-account-id',[string]$LegacyAccountId) }
        if ($SourceProfile) { $resolved = (Resolve-Path -LiteralPath $SourceProfile).Path; $appArgs += @('--source-profile',('"'+$resolved+'"')) }
        $windowStyle = if ($Open) { 'Normal' } else { 'Hidden' }
        $process = Start-Process -FilePath (Join-Path $projectRoot 'target/debug/muse-desktop.exe') -ArgumentList $appArgs -WindowStyle $windowStyle -PassThru
        if (-not $Open -and -not $process.WaitForExit(60000)) { throw "Import still running; process $($process.Id)." }
        if (-not $Open) {
            $result = Get-Content -LiteralPath (Join-Path $DataDirectory 'import-result.json') -Raw | ConvertFrom-Json
            $result | ConvertTo-Json
            if (-not $result.imported) { throw $result.error }
        }
    } finally { Pop-Location }
} finally { $env:CARGO_HOME = $oldCargoHome; $env:CARGO_TARGET_DIR = $oldTarget }
