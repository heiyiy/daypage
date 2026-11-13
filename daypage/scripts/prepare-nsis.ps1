$ErrorActionPreference='Stop'
$cache=Join-Path $env:LOCALAPPDATA 'tauri'
$source=Join-Path $cache 'nsis-3.13'
$destination=Join-Path $cache 'NSIS'
if(-not (Test-Path -LiteralPath (Join-Path $source 'NSIS.exe'))){throw 'Verified NSIS extraction is missing'}
robocopy.exe $source $destination /E /NFL /NDL /NJH /NJS | Out-Null
if($LASTEXITCODE -gt 7){throw 'NSIS copy failed'}
$plugins=Join-Path $destination 'Plugins\x86-unicode\additional'
New-Item -ItemType Directory -Force -Path $plugins | Out-Null
$plugin=Join-Path $plugins 'nsis_tauri_utils.dll'
curl.exe --ssl-no-revoke --noproxy '*' -L --fail --max-time 60 -o $plugin https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll
if($LASTEXITCODE -ne 0){throw 'NSIS plugin download failed'}
if((Get-FileHash -LiteralPath $plugin -Algorithm SHA1).Hash -ne '75197FEE3C6A814FE035788D1C34EAD39349B860'){throw 'NSIS plugin checksum mismatch'}
Write-Output 'NSIS tool cache and plugin verified'
