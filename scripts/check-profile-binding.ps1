param([switch]$NoBuild,[string]$ReportDirectory)
$ErrorActionPreference='Stop'
$projectRoot=(Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
if ($ReportDirectory) {
    $report=Join-Path $ReportDirectory 'smoke-result.json'
    if(-not (Test-Path -LiteralPath $report)){throw 'Native profile check has not returned a result yet.'}
    $result=Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
    $result | ConvertTo-Json
    if(-not $result.passed){throw "Profile override was not rejected: $report"}
    return
}
$previousCargo=$env:CARGO_HOME
$previousTarget=$env:CARGO_TARGET_DIR
$previousOverride=$env:WEBVIEW2_USER_DATA_FOLDER
try {
    $env:CARGO_HOME=Join-Path $projectRoot '.cargo-home'
    $env:CARGO_TARGET_DIR=Join-Path $projectRoot 'target'
    if(-not $NoBuild) {
        Push-Location -LiteralPath $projectRoot
        try { cargo build --locked -p muse-desktop; if($LASTEXITCODE -ne 0){throw 'Build failed.'} } finally { Pop-Location }
    }
    $fixture=Join-Path $projectRoot ('.runtime/binding-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
    $override=Join-Path $fixture 'unexpected-browser-folder'
    $null=New-Item -ItemType Directory -Path $override -Force
    $env:WEBVIEW2_USER_DATA_FOLDER=$override.Replace('\','/')
    $fixtureArgument=$fixture.Replace('\','/')
    $process=Start-Process -FilePath (Join-Path $projectRoot 'target/debug/muse-desktop.exe') -ArgumentList @('--data-dir',('"'+$fixtureArgument+'"'),'--proxy-smoke-test','--profile-binding-smoke-test') -WindowStyle Normal -RedirectStandardError (Join-Path $fixture 'stderr.log') -PassThru
    $null=$process.WaitForInputIdle(10000)
    # Return control to the interactive desktop during this environment-override
    # check. In this host, blocking the launch command can prevent initialization.
    [pscustomobject]@{ProcessId=$process.Id;ReportDirectory=$fixture;Status='running';ReadResultWith='.\scripts\check-profile-binding.ps1 -ReportDirectory <ReportDirectory>'}|ConvertTo-Json
} finally {
    $env:CARGO_HOME=$previousCargo
    $env:CARGO_TARGET_DIR=$previousTarget
    $env:WEBVIEW2_USER_DATA_FOLDER=$previousOverride
}
