param([string]$DataDirectory = (Join-Path $PSScriptRoot '..\.runtime\development'))
# 统计每个账号的浏览器（msedgewebview2.exe）占用的私有内存，
# 分成「固定开销」（浏览器主进程、GPU、网络等）和「页面」两部分。
# 共享浏览器方案能省下的，就是多出来的那几份「固定开销」。
$ErrorActionPreference = 'Stop'
$names = @{}
$workspace = Join-Path $DataDirectory 'workspace.json'
if (Test-Path -LiteralPath $workspace) {
    try {
        (Get-Content -LiteralPath $workspace -Raw -Encoding UTF8 | ConvertFrom-Json).accounts |
            ForEach-Object { $names[[string]$_.id] = [string]$_.name }
    } catch {}
}
$all = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'")
if ($all.Count -eq 0) { Write-Output '没有正在运行的 WebView2 进程，请先打开 Muse 并启动账号。'; return }
$private = @{}
Get-Process -Name msedgewebview2 -ErrorAction SilentlyContinue |
    ForEach-Object { $private[[int]$_.Id] = $_.PrivateMemorySize64 }
$byId = @{}; foreach ($p in $all) { $byId[[int]$p.ProcessId] = $p }
function Get-Kind($p) {
    $cmd = [string]$p.CommandLine
    if ($cmd -notmatch '--type=([\w-]+)') { return 'browser' }
    $type = $Matches[1]
    if ($type -eq 'utility' -and $cmd -match '--utility-sub-type=([\w.]+)') { return 'utility:' + ($Matches[1] -replace '^.*\.', '') }
    return $type
}
function Get-Root($p) {
    $current = $p
    for ($i = 0; $i -lt 6; $i++) {
        if ((Get-Kind $current) -eq 'browser') { return $current }
        $parent = $byId[[int]$current.ParentProcessId]
        if (-not $parent) { return $null }
        $current = $parent
    }
    return $null
}
$groups = @{}
foreach ($p in $all) {
    $root = Get-Root $p
    $key = if ($root) { [int]$root.ProcessId } else { 0 }
    if (-not $groups.ContainsKey($key)) { $groups[$key] = [ordered]@{ Root = $root; Fixed = 0L; Pages = 0L; PageCount = 0; Processes = 0 } }
    $g = $groups[$key]
    $bytes = [long]$private[[int]$p.ProcessId]
    $g.Processes++
    if ((Get-Kind $p) -eq 'renderer') { $g.Pages += $bytes; $g.PageCount++ } else { $g.Fixed += $bytes }
}
$rows = foreach ($key in $groups.Keys) {
    $g = $groups[$key]
    $label = '（未识别）'
    if ($g.Root -and ([string]$g.Root.CommandLine -match '--user-data-dir="?([^"]+)')) {
        $dir = $Matches[1]
        if ($dir -match '([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})') {
            $id = $Matches[1].ToLower()
            $label = if ($names[$id]) { $names[$id] } else { $id }
        } else { $label = Split-Path $dir -Leaf }
    }
    [pscustomobject]@{
        账号 = $label
        进程数 = $g.Processes
        '固定开销(MB)' = [math]::Round($g.Fixed / 1MB)
        页面数 = $g.PageCount
        '页面(MB)' = [math]::Round($g.Pages / 1MB)
        '合计(MB)' = [math]::Round(($g.Fixed + $g.Pages) / 1MB)
    }
}
$rows = @($rows | Sort-Object '合计(MB)' -Descending)
$rows | Format-Table -AutoSize
$fixed = ($rows | Measure-Object '固定开销(MB)' -Sum).Sum
$total = ($rows | Measure-Object '合计(MB)' -Sum).Sum
$count = $rows.Count
if ($count -gt 1) {
    $saving = [math]::Round($fixed - $fixed / $count)
    Write-Output ("共 {0} 套浏览器环境，合计约 {1} MB，其中固定开销约 {2} MB。" -f $count, $total, $fixed)
    Write-Output ("如果全部共用一套浏览器，大约能省 {0} MB（约 {1}%）。" -f $saving, [math]::Round($saving * 100 / [math]::Max($total, 1)))
} else {
    Write-Output ("只有 1 套浏览器环境，合计约 {0} MB；共享方案没有可省的部分。" -f $total)
}
