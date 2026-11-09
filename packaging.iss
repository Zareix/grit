; Grit — Windows installer (Inno Setup 6).
;
; Per-user install: %LOCALAPPDATA%\Programs\Grit, no UAC, Start menu entry,
; optional desktop shortcut, uninstall entry. Upgrades reuse the same AppId
; and keep %APPDATA%\grit (settings + session) untouched.
;
; Build from the repository root:
;   ISCC.exe /DMyAppVersion=0.10.0 packaging.iss
; (the release workflow passes the tag version without its leading "v").

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif

[Setup]
AppId=com.zareix.grit
AppName=Grit
AppVersion={#MyAppVersion}
AppVerName=Grit {#MyAppVersion}
AppPublisher=Raphael C.
AppPublisherURL=https://github.com/Zareix/grit
AppSupportURL=https://github.com/Zareix/grit/issues
AppUpdatesURL=https://github.com/Zareix/grit/releases
DefaultDirName={autopf}\Grit
DefaultGroupName=Grit
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=dist
OutputBaseFilename=grit-{#MyAppVersion}-x64-windows-setup
SetupIconFile=packaging\grit.ico
UninstallDisplayIcon={app}\grit.exe
UninstallDisplayName=Grit
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

[Files]
Source: "target\release\grit.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Keep the shortcut's AUMID in sync with `App::set_app_identity` in main.rs.
Name: "{group}\Grit"; Filename: "{app}\grit.exe"; AppUserModelID: "com.zareix.grit"
Name: "{autodesktop}\Grit"; Filename: "{app}\grit.exe"; Tasks: desktopicon; AppUserModelID: "com.zareix.grit"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Run]
Filename: "{app}\grit.exe"; Description: "Launch Grit"; Flags: nowait postinstall skipifsilent
