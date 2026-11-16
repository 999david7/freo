; Inno Setup script for the Freo installer on Windows.
; Built by the release workflow, once per architecture:
;   iscc /DVersion=1.0.0 /DArch=x64 /DSourceDir=target\x86_64-pc-windows-msvc\release /DOutputDir=dist packaging\windows\freo.iss
; Arch is x86, x64 or arm64.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\release"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{5B7F2C61-3E4A-4D2B-9C1E-F7E0A1D9B4C8}
AppName=Freo
AppVersion={#Version}
AppVerName=Freo {#Version}
AppPublisher=Freo
AppPublisherURL=https://github.com/999david7/freo
AppSupportURL=https://github.com/999david7/freo/issues
AppUpdatesURL=https://github.com/999david7/freo/releases
DefaultDirName={autopf}\Freo
DefaultGroupName=Freo
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename=Freo-{#Version}-windows-{#Arch}-setup
SetupIconFile=freo.ico
UninstallDisplayIcon={app}\freo.exe
UninstallDisplayName=Freo {#Version}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; installs for all users with admin rights, or just for the current user without them
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ChangesAssociations=yes
CloseApplications=yes
#if Arch == "x64"
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#elif Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#endif

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "assoc"; Description: "Freo-Dateien (.pfprt, .pfasm, .pfdrw) mit Freo öffnen"; GroupDescription: "Dateizuordnungen:"
Name: "assocstep"; Description: "STEP-Dateien (.stp, .step) mit Freo öffnen"; GroupDescription: "Dateizuordnungen:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\freo.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\freo-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme
Source: "freo.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Freo"; Filename: "{app}\freo.exe"; IconFilename: "{app}\freo.ico"
Name: "{group}\{cm:UninstallProgram,Freo}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Freo"; Filename: "{app}\freo.exe"; IconFilename: "{app}\freo.ico"; Tasks: desktopicon

[Registry]
; one ProgID per file type, opened with freo.exe
Root: HKA; Subkey: "Software\Classes\Freo.Part"; ValueType: string; ValueData: "Freo-Teil"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Part\DefaultIcon"; ValueType: string; ValueData: "{app}\freo.ico"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Part\shell\open\command"; ValueType: string; ValueData: """{app}\freo.exe"" ""%1"""; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.pfprt\OpenWithProgids"; ValueType: string; ValueName: "Freo.Part"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Assembly"; ValueType: string; ValueData: "Freo-Baugruppe"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Assembly\DefaultIcon"; ValueType: string; ValueData: "{app}\freo.ico"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Assembly\shell\open\command"; ValueType: string; ValueData: """{app}\freo.exe"" ""%1"""; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.pfasm\OpenWithProgids"; ValueType: string; ValueName: "Freo.Assembly"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Drawing"; ValueType: string; ValueData: "Freo-Zeichnung"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Drawing\DefaultIcon"; ValueType: string; ValueData: "{app}\freo.ico"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Drawing\shell\open\command"; ValueType: string; ValueData: """{app}\freo.exe"" ""%1"""; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.pfdrw\OpenWithProgids"; ValueType: string; ValueName: "Freo.Drawing"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\Freo.Step"; ValueType: string; ValueData: "STEP-Modell"; Flags: uninsdeletekey; Tasks: assocstep
Root: HKA; Subkey: "Software\Classes\Freo.Step\DefaultIcon"; ValueType: string; ValueData: "{app}\freo.ico"; Tasks: assocstep
Root: HKA; Subkey: "Software\Classes\Freo.Step\shell\open\command"; ValueType: string; ValueData: """{app}\freo.exe"" ""%1"""; Tasks: assocstep
Root: HKA; Subkey: "Software\Classes\.stp\OpenWithProgids"; ValueType: string; ValueName: "Freo.Step"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assocstep
Root: HKA; Subkey: "Software\Classes\.step\OpenWithProgids"; ValueType: string; ValueName: "Freo.Step"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assocstep
; Freo's own file types open with Freo by default
Root: HKA; Subkey: "Software\Classes\.pfprt"; ValueType: string; ValueData: "Freo.Part"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.pfasm"; ValueType: string; ValueData: "Freo.Assembly"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.pfdrw"; ValueType: string; ValueData: "Freo.Drawing"; Flags: uninsdeletevalue; Tasks: assoc

[Run]
Filename: "{app}\freo.exe"; Description: "{cm:LaunchProgram,Freo}"; Flags: nowait postinstall skipifsilent
