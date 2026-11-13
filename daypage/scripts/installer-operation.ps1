param([string]$Installer,[string]$Target,[switch]$Uninstall,[switch]$Update)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\verification'))+[IO.Path]::DirectorySeparatorChar
$resolved=[IO.Path]::GetFullPath($Target)
if(-not $resolved.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)){throw 'Installer test target must stay in verification'}
if($Uninstall){
  $uninstaller=Join-Path $resolved 'uninstall.exe'
  if(Test-Path -LiteralPath $uninstaller){
    $process=Start-Process -FilePath $uninstaller -ArgumentList '/S' -WindowStyle Hidden -PassThru
    if(-not $process.WaitForExit(60000)){throw 'Test uninstaller timed out'}
    if($process.ExitCode -ne 0){throw "Uninstaller exit $($process.ExitCode)"}
  }
}else{
  $arguments=@('/S');if($Update){$arguments+='/UPDATE'};$arguments+=('/D='+$resolved)
  $process=Start-Process -FilePath $Installer -ArgumentList $arguments -WindowStyle Hidden -PassThru
  if(-not $process.WaitForExit(120000)){throw 'Test installer timed out'}
  if($process.ExitCode -ne 0){throw "Installer exit $($process.ExitCode)"}
}
