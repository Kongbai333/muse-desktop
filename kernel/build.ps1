param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$GoExecutable = ''
)
$ErrorActionPreference = 'Stop'
$imageHash = 'b3db115379b866eeb1db722be0f31bf6bfa1610dd54a019fc748ecf9a733ab9d'
$sourceHash = '7f9b86d236164ec2c9745d7e41782e584ce5e0771c153538bab1f84938feea80'
$goHash = '178ab98950ac69ab25069307dc6ade79f3816e414cce95b4da166a115cde1e7e'
$sourceZip = Join-Path $PSScriptRoot 'mihomo-source.zip'
if ((Get-FileHash -LiteralPath $sourceZip).Hash -ne $sourceHash) { throw 'Kernel source digest mismatch.' }
$out = [IO.Directory]::CreateDirectory($OutputDirectory).FullName
$image = Join-Path $out 'mihomo.exe'
if ((Test-Path -LiteralPath $image -PathType Leaf) -and (Get-FileHash -LiteralPath $image).Hash -eq $imageHash) { return }
$scratch = [IO.Directory]::CreateDirectory((Join-Path $out ('build-' + [Guid]::NewGuid().ToString('N')))).FullName
$variables = @{
    GOTOOLCHAIN='local'; GOENV='off'; CGO_ENABLED='0'; GOOS='windows'; GOARCH='amd64'; GOAMD64='v1';
    GOFLAGS=''; GOEXPERIMENT=''; GOWORK='off'; GOPROXY='off'; GOSUMDB='off'; GOTELEMETRY='off';
    GOCACHE=(Join-Path $scratch 'cache'); GOPATH=(Join-Path $scratch 'home'); GOROOT=$null;
    GOMODCACHE=(Join-Path $scratch 'modules'); GOTMPDIR=(Join-Path $scratch 'tmp');
    APPDATA=(Join-Path $scratch 'roaming'); LOCALAPPDATA=(Join-Path $scratch 'local')
}
$previous = @{}
foreach ($key in $variables.Keys) { $previous[$key] = [Environment]::GetEnvironmentVariable($key) }
try {
    foreach ($key in $variables.Keys) { [Environment]::SetEnvironmentVariable($key, $variables[$key]) }
    [IO.Directory]::CreateDirectory($env:GOTMPDIR) | Out-Null
    if (!$GoExecutable) {
        $zip = Join-Path $scratch 'go.zip'
        Invoke-WebRequest 'https://dl.google.com/go/go1.27.4.windows-amd64.zip' -OutFile $zip
        if ((Get-FileHash -LiteralPath $zip).Hash -ne $goHash) { throw 'Go toolchain digest mismatch.' }
        [IO.Compression.ZipFile]::ExtractToDirectory($zip, $scratch)
        $GoExecutable = Join-Path $scratch 'go/bin/go.exe'
    }
    $go = (Resolve-Path -LiteralPath $GoExecutable).ProviderPath
    if ((& $go version) -cne 'go version go1.27.4 windows/amd64') { throw 'Expected the pinned Windows x64 Go 1.27.4 toolchain.' }
    $source = Join-Path $scratch 'source'
    [IO.Compression.ZipFile]::ExtractToDirectory($sourceZip, $source)
    $built = Join-Path $scratch 'mihomo.exe'
    Push-Location -LiteralPath $source
    try {
        & $go build -buildvcs=false -mod=vendor -tags with_gvisor -trimpath -ldflags '-s -w -buildid= -X github.com/metacubex/mihomo/constant.Version=v1.19.34-muse.0.1.11' -o $built .
        if ($LASTEXITCODE -ne 0) { throw 'Mihomo build failed.' }
    } finally { Pop-Location }
    if ((Get-FileHash -LiteralPath $built).Hash -ne $imageHash) { throw 'Rebuilt kernel digest mismatch.' }
    Copy-Item -LiteralPath $built -Destination $image -Force
} finally {
    foreach ($key in $previous.Keys) { [Environment]::SetEnvironmentVariable($key, $previous[$key]) }
    $resolved = (Resolve-Path -LiteralPath $scratch).ProviderPath
    if ([IO.Path]::GetDirectoryName($resolved) -ne $out -or [IO.Path]::GetFileName($resolved) -notmatch '^build-[a-f0-9]{32}$') { throw 'Unsafe build cleanup path.' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
