param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$DataDirectory)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'desktop-input.ps1')
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'muse-desktop'){throw 'Only Muse Desktop fixtures can be tested.'}
$handle=$app.MainWindowHandle
if($handle -eq [IntPtr]::Zero){throw 'Muse window is missing.'}
$scale=[MuseUiInput]::GetDpiForWindow($handle)/96.0
$statePath=Join-Path $DataDirectory 'sidebar.json'
$state=if(Test-Path -LiteralPath $statePath){Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json}else{[pscustomobject]@{width=380;collapsed=$false}}
if($state.collapsed){Invoke-MuseClick $handle 24 18}
Start-Sleep -Milliseconds 350
$initialLeft=[MuseUiInput]::BrowserLeft($handle)/$scale
if($initialLeft -lt 294){throw 'Expanded browser geometry is missing.'}
$from=$initialLeft-3
$target=$from+60
$null=[MuseUiInput]::SetForegroundWindow($handle)
Start-Sleep -Milliseconds 300
Invoke-MuseDrag $handle $from $target 360
$wide=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
$wideLeft=[MuseUiInput]::BrowserLeft($handle)/$scale
if([math]::Abs($wide.width-($target-3)) -gt 1 -or [math]::Abs($wideLeft-($wide.width+6)) -gt 1){throw 'Actual drag did not move the native browser with the divider.'}
Invoke-MuseDrag $handle ($wideLeft-3) ($wideLeft-43) 360
$narrow=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
$narrowLeft=[MuseUiInput]::BrowserLeft($handle)/$scale
if([math]::Abs($narrow.width-($wide.width-40)) -gt 1 -or [math]::Abs($narrowLeft-($narrow.width+6)) -gt 1){throw 'Reverse drag did not preserve native alignment.'}
Invoke-MuseClick $handle 24 18
$compact=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
if(-not $compact.collapsed -or [MuseUiInput]::BrowserLeft($handle) -ne [int](72*$scale)){throw 'Avatar-only mode did not reclaim browser space.'}
Invoke-MuseClick $handle 24 18
$expanded=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
if($expanded.collapsed -or $expanded.width -ne $narrow.width -or [math]::Abs([MuseUiInput]::BrowserLeft($handle)/$scale-($narrow.width+6)) -gt 1){throw 'Expand did not restore the dragged width.'}
$report=[pscustomobject]@{passed=$true;real_pointer_resize_both_directions=$true;native_webview_aligned=$true;compact_width=72;expanded_width_restored=$true;saved_width=$expanded.width}
$report|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $DataDirectory 'sidebar-result.json')
$report|ConvertTo-Json
