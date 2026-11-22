$ErrorActionPreference = 'Stop'
$packager = Join-Path $PSScriptRoot 'package-windows.ps1'
if (!(Test-Path -LiteralPath $packager)) { throw 'Windows release packager is missing.' }

$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$scratch = Join-Path $tempRoot ('muse-package-test-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $scratch
try {
    $binary = Join-Path $scratch 'client.exe'
    [IO.File]::WriteAllText($binary, 'Muse packaging test executable')
    [IO.File]::WriteAllText((Join-Path $scratch 'private.keystore'), 'must never be packaged')
    $output = Join-Path $scratch 'dist'
    & $packager -BinaryPath $binary -OutputDirectory $output | Out-Null
    $zip = @(Get-ChildItem -LiteralPath $output -File -Filter '*.zip')
    if ($zip.Count -ne 1) { throw 'Expected exactly one portable ZIP.' }
    $archive = [IO.Compression.ZipFile]::OpenRead($zip[0].FullName)
    try {
        $entries = @($archive.Entries)
        $app = @($entries | Where-Object { $_.FullName.Replace('\', '/').EndsWith('/Muse.exe') })
        if ($app.Count -ne 1) { throw 'Portable ZIP must contain Muse.exe.' }
        $reader = [IO.StreamReader]::new($app[0].Open())
        try { $payload = $reader.ReadToEnd() } finally { $reader.Dispose() }
        if ($payload -cne 'Muse packaging test executable') { throw 'Packaged executable bytes changed.' }
        foreach ($name in @('LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md', 'README.txt')) {
            if (!($entries | Where-Object { $_.FullName.Replace('\', '/').EndsWith('/' + $name) })) {
                throw "Portable ZIP is missing $name."
            }
        }
        if ($entries | Where-Object { $_.FullName -match 'keystore|\.runtime|\.cargo-home|workspace\.json' }) {
            throw 'Private or machine-specific files leaked into the package.'
        }
    } finally { $archive.Dispose() }
    $checksum = Get-Content -LiteralPath ($zip[0].FullName + '.sha256') -Raw
    if (!$checksum.StartsWith((Get-FileHash -LiteralPath $zip[0].FullName).Hash.ToLowerInvariant())) {
        throw 'ZIP checksum does not match.'
    }
    $rejected = $false
    try { & $packager -BinaryPath $binary -OutputDirectory $output -Tag 'v999.999.999' | Out-Null }
    catch { $rejected = $_.Exception.Message -like '*does not match*' }
    if (!$rejected) { throw 'A tag/source version mismatch must reject packaging.' }
    Write-Output 'Windows package: executable bytes, license files, private-file exclusion, checksum and tag validation passed.'
} finally {
    $resolved = [IO.Path]::GetFullPath($scratch)
    if (!$resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -or
        !(Split-Path -Leaf $resolved).StartsWith('muse-package-test-')) { throw 'Unsafe test cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
