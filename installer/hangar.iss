; HANGAR installer (Inno Setup 6).
; Build:  ISCC /DAppVersion=0.2.0 installer\hangar.iss
; Per-user install, no admin rights. Upgrades in place (stable AppId).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define AppName "HANGAR"
#define AppExe "HANGAR.exe"
; Must match platform::acquire_instance() in src/platform.rs.
#define AppMutexName "HangarLauncher.SingleInstance"

[Setup]
AppId={{8E0B6C0A-5B0E-4F3B-9C55-6B1D0F2A7C41}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=HANGAR
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
DefaultDirName={localappdata}\Programs\Hangar
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=HANGAR-Setup-{#AppVersion}
SetupIconFile=..\assets\hangar.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Ask the user to close a running copy (and close it on silent upgrades).
AppMutex={#AppMutexName}
CloseApplications=yes
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "standard"; Description: "Standard"
Name: "custom"; Description: "Custom"; Flags: iscustom

[Components]
Name: "main"; Description: "HANGAR launcher"; Types: standard custom; Flags: fixed
Name: "sendto"; Description: "Send To menu entry (right-click a file > Send to > HANGAR)"; Types: standard custom
Name: "autostart"; Description: "Start HANGAR when I sign in to Windows"; Types: custom

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\target\release\hangar.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion; Components: main
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "..\assets\fonts\OFL-Oxanium.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion; Components: main
Source: "..\assets\fonts\OFL-JetBrainsMono.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion; Components: main

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon
Name: "{usersendto}\{#AppName}"; Filename: "{app}\{#AppExe}"; Components: sendto

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Components: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
var
  SelfTestCode: Integer;

{ After the files are in place, ask the app itself whether it can open a
  window here. HANGAR's only system prerequisite is a graphics driver with
  OpenGL 2.0+; it needs no VC++ runtime, .NET, Python or GPU toolkit. }
procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
begin
  if CurStep = ssPostInstall then
  begin
    SelfTestCode := -1;
    if Exec(ExpandConstant('{app}\{#AppExe}'), '--selftest', '', SW_HIDE, ewWaitUntilTerminated, Code) then
      SelfTestCode := Code;
    Log(Format('HANGAR self-test exit code: %d', [SelfTestCode]));
    if (SelfTestCode <> 0) and (not WizardSilent) then
      MsgBox('HANGAR was installed, but it could not open its window on this PC (self-test code ' + IntToStr(SelfTestCode) + ').' + #13#10#13#10 +
             'Missing prerequisite: a graphics driver with OpenGL 2.0 or newer.' + #13#10 +
             'This usually means the graphics driver is missing or very old, or you are on a remote-desktop or virtual machine session without GPU support.' + #13#10#13#10 +
             'Fix: install the current driver from your GPU maker (NVIDIA, AMD or Intel), then start HANGAR again. No reinstall is needed.',
             mbError, MB_OK);
  end;
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo, MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := MemoDirInfo + NewLine + NewLine + MemoComponentsInfo + NewLine + NewLine;
  if MemoTasksInfo <> '' then
    Result := Result + MemoTasksInfo + NewLine + NewLine;
  Result := Result + 'System check:' + NewLine +
    Space + 'Windows 10 or newer, 64-bit: OK' + NewLine +
    Space + 'Visual C++ runtime: not needed (built in)' + NewLine +
    Space + 'Graphics (OpenGL 2.0+): tested right after files are copied';
end;

{ Uninstall keeps the user's layout unless they opt in.
  Silent uninstall keeps it unless /PURGEDATA is passed. }
function WantsPurge(): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/PURGEDATA') = 0 then
      Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
  Purge: Boolean;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{userappdata}\Hangar');
    if DirExists(DataDir) then
    begin
      Purge := WantsPurge();
      if (not Purge) and (not UninstallSilent) then
        Purge := MsgBox('Also delete your saved layout and settings?' + #13#10#13#10 + DataDir + #13#10#13#10 +
                        'Choose No to keep them for a future install.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
      if Purge then
        DelTree(DataDir, True, True, True);
    end;
  end;
end;
