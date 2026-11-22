param(
    [switch]$Diagnostics,
    [switch]$ProxySettings,
    [switch]$SmokeTest,
    [switch]$ProxySmokeTest,
    [switch]$ProxyEditSmokeTest,
    [switch]$AccountDialogSmokeTest,
    [switch]$NoBuild,
    [string]$DataDirectory
)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$previousCargoHome = $env:CARGO_HOME
$previousTargetDirectory = $env:CARGO_TARGET_DIR
if ($ProxyEditSmokeTest) { $ProxySmokeTest = $true }
if ($ProxySmokeTest) { $SmokeTest = $true }
if ($AccountDialogSmokeTest) { $SmokeTest = $true }

try {
    $env:CARGO_HOME = Join-Path $projectRoot '.cargo-home'
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target'
    Push-Location -LiteralPath $projectRoot
    try {
        if (-not $NoBuild) {
            cargo build --locked -p muse-desktop
            if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
        }
        $binary = Join-Path $projectRoot 'target/debug/muse-desktop.exe'
        if (-not (Test-Path -LiteralPath $binary)) { throw "Executable not found: $binary" }
        if (-not $DataDirectory) {
            $folder = if ($SmokeTest) { 'smoke-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff') }
                elseif ($Diagnostics) { 'diagnostics' } else { 'development' }
            $DataDirectory = Join-Path $projectRoot ".runtime/$folder"
        }
        $null = New-Item -ItemType Directory -Path $DataDirectory -Force
        $resolvedData = (Resolve-Path -LiteralPath $DataDirectory).Path
        $appArguments = @('--data-dir', $resolvedData)
        if ($Diagnostics) { $appArguments += '--diagnostics' }
        if ($ProxySettings) { $appArguments += '--proxy-settings' }
        if ($SmokeTest) { $appArguments += '--smoke-test' }
        if ($ProxySmokeTest) { $appArguments += '--proxy-smoke-test' }
        if ($ProxyEditSmokeTest) { $appArguments += '--proxy-edit-smoke-test' }
        if ($AccountDialogSmokeTest) { $appArguments += '--account-dialog-smoke-test' }
        if ($SmokeTest) {
            # Use the same interactive desktop owner as ordinary previews.
            $process = & (Join-Path $PSScriptRoot 'start-desktop.ps1') -Binary $binary -AppArguments $appArguments -WorkingDirectory $projectRoot
            if (-not $process.WaitForExit(60000)) {
                throw "Smoke process did not exit; inspect process $($process.Id) and $resolvedData"
            }
            $report = Join-Path $resolvedData 'smoke-result.json'
            if (-not (Test-Path -LiteralPath $report)) { throw "Smoke report missing: $report" }
            $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
            $result | ConvertTo-Json
            if (-not $result.passed) { throw "Native integration smoke failed: $report" }
        }
        else {
            $process = & (Join-Path $PSScriptRoot 'start-desktop.ps1') -Binary $binary -AppArguments $appArguments -WorkingDirectory $projectRoot
            Write-Output "Muse Desktop started independently. Process: $($process.Id). Data: $resolvedData"
        }
    }
    finally { Pop-Location }
}
finally {
    $env:CARGO_HOME = $previousCargoHome
    $env:CARGO_TARGET_DIR = $previousTargetDirectory
}

