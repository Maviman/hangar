# CI smoke test: silent install -> checks -> launch -> upgrade in place -> silent uninstall.
# Usage: pwsh ci/smoke.ps1 -Setup dist\HANGAR-Setup-0.2.0.exe -Version 0.2.0
param(
  [Parameter(Mandatory)] [string] $Setup,
  [Parameter(Mandatory)] [string] $Version
)
$ErrorActionPreference = 'Stop'
$appDir  = Join-Path $env:LOCALAPPDATA 'Programs\Hangar'
$exe     = Join-Path $appDir 'HANGAR.exe'
$dataDir = Join-Path $env:APPDATA 'Hangar'
$uninstKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{8E0B6C0A-5B0E-4F3B-9C55-6B1D0F2A7C41}_is1'
$startLnk  = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\HANGAR.lnk'
$deskLnk   = Join-Path ([Environment]::GetFolderPath('Desktop')) 'HANGAR.lnk'
$sendLnk   = Join-Path $env:APPDATA 'Microsoft\Windows\SendTo\HANGAR.lnk'

function Step($m) { Write-Host "`n=== $m ===" -ForegroundColor Cyan }
function Assert($cond, $msg) { if (-not $cond) { throw "ASSERT FAILED: $msg" } else { Write-Host "ok  $msg" } }
function Run($file, $argList, $timeoutSec = 180) {
  if ([string]::IsNullOrEmpty($argList)) { $p = Start-Process -FilePath $file -PassThru }
  else { $p = Start-Process -FilePath $file -ArgumentList $argList -PassThru }
  if (-not $p.WaitForExit($timeoutSec * 1000)) { $p.Kill(); throw "Timed out after ${timeoutSec}s: $file $argList" }
  return $p.ExitCode
}

Step 'Silent install'
$code = Run $Setup '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /TASKS="desktopicon" /LOG=install.log'
Assert ($code -eq 0) "installer exit code 0 (got $code)"
Assert (Test-Path $exe) 'HANGAR.exe installed under %LOCALAPPDATA%\Programs\Hangar (per-user)'
Assert (Test-Path $startLnk) 'Start Menu shortcut exists'
Assert (Test-Path $deskLnk) 'Desktop shortcut exists (task selected)'
Assert (Test-Path $sendLnk) 'Send To entry exists (standard component)'
Assert (Test-Path $uninstKey) 'listed in Add/Remove Programs (HKCU)'
$dv = (Get-ItemProperty $uninstKey).DisplayVersion
Assert ($dv -eq $Version) "Add/Remove Programs version is $Version (got $dv)"
$fv = (Get-Item $exe).VersionInfo.ProductVersion
Assert ($fv -eq $Version) "exe ProductVersion is $Version (got $fv)"

Step 'OpenGL for the runner'
# Hosted runners have no GPU driver (OpenGL 1.1 only). Put Mesa's software
# renderer next to the exe for the launch test, exactly like a user PC's
# driver would provide it. Removed again before uninstall.
$mesaDir = Join-Path $env:RUNNER_TEMP 'mesa'
New-Item -ItemType Directory -Force $mesaDir | Out-Null
gh release download --repo pal1000/mesa-dist-win --pattern '*release-msvc.7z' --dir $mesaDir --clobber
$archive = Get-ChildItem $mesaDir -Filter '*release-msvc.7z' | Select-Object -First 1
7z x $archive.FullName "-o$mesaDir\x" -y | Out-Null
$mesaDlls = @()
foreach ($name in 'opengl32.dll', 'libgallium_wgl.dll', 'libglapi.dll', 'dxil.dll') {
  $src = Join-Path "$mesaDir\x\x64" $name
  if (Test-Path $src) { Copy-Item $src $appDir; $mesaDlls += (Join-Path $appDir $name) }
}
Assert ($mesaDlls.Count -ge 1) "Mesa DLLs copied ($($mesaDlls.Count))"
$env:GALLIUM_DRIVER = 'llvmpipe'

Step 'Launch: self-test (open window, draw frames, exit 0)'
$code = Run $exe '--selftest' 90
if ($code -ne 0) {
  $crash = Join-Path $dataDir 'crash.log'
  if (Test-Path $crash) { Get-Content $crash | Write-Host }
}
Assert ($code -eq 0) "self-test exit code 0 (got $code)"

Step 'Launch: normal start stays running and saves its layout'
$p = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds 8
Assert (-not $p.HasExited) 'app still running after 8 s'
$p.Refresh()
Write-Host "window title: '$($p.MainWindowTitle)'  working set: $([math]::Round($p.WorkingSet64/1MB)) MB"
Assert ($p.MainWindowTitle -like 'HANGAR*') 'main window titled HANGAR'
# Second copy must hand off and exit instead of running twice.
$code2 = Run $exe '' 30
Assert ($code2 -eq 0) 'second copy exits cleanly (single instance)'
Assert (-not $p.HasExited) 'first copy survived the second launch'
$p.CloseMainWindow() | Out-Null
if (-not $p.WaitForExit(15000)) { $p.Kill(); throw 'app did not close within 15 s of a normal close request' }
Assert (Test-Path (Join-Path $dataDir 'hangar.json')) 'layout saved to %APPDATA%\Hangar\hangar.json on exit'

Step 'Upgrade in place (re-run the installer)'
$code = Run $Setup '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /LOG=upgrade.log'
Assert ($code -eq 0) "re-install exit code 0 (got $code)"
Assert (@(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Where-Object { $_.PSChildName -like '*8E0B6C0A*' }).Count -eq 1) 'still exactly one Add/Remove Programs entry'
Assert (Test-Path (Join-Path $dataDir 'hangar.json')) 'layout survived the upgrade'

Step 'Silent uninstall'
$mesaDlls | ForEach-Object { Remove-Item $_ -Force }
$unins = Get-ChildItem $appDir -Filter 'unins*.exe' | Select-Object -First 1
Assert ($null -ne $unins) 'uninstaller present'
$code = Run $unins.FullName '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
Assert ($code -eq 0) "uninstaller exit code 0 (got $code)"
# The uninstaller hands over to a temp copy and returns early: wait until it is really gone.
$deadline = (Get-Date).AddSeconds(60)
while ((Test-Path $appDir) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Assert (-not (Test-Path $exe)) 'HANGAR.exe removed'
Assert (-not (Test-Path $appDir)) 'install folder removed'
Assert (-not (Test-Path $startLnk)) 'Start Menu shortcut removed'
Assert (-not (Test-Path $deskLnk)) 'Desktop shortcut removed'
Assert (-not (Test-Path $sendLnk)) 'Send To entry removed'
Assert (-not (Test-Path $uninstKey)) 'Add/Remove Programs entry removed'
Assert (Test-Path (Join-Path $dataDir 'hangar.json')) 'user data kept by default'

Step 'Opt-in data removal (/PURGEDATA)'
$code = Run $Setup '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-'
Assert ($code -eq 0) 'reinstall for purge test'
$unins = Get-ChildItem $appDir -Filter 'unins*.exe' | Select-Object -First 1
$code = Run $unins.FullName '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /PURGEDATA'
# The uninstaller hands over to a temp copy and returns early: wait until it is really gone.
$deadline = (Get-Date).AddSeconds(60)
while ((Test-Path $appDir) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Assert ($code -eq 0) 'uninstall with /PURGEDATA exit code 0'
Assert (-not (Test-Path $dataDir)) 'user data removed when opted in'

Write-Host "`nSMOKE TEST PASSED" -ForegroundColor Green
