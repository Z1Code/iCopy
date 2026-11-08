; iCopy — instalador por usuario (sin administrador). Compilar con empaquetar.ps1.
#ifndef Version
  #error Falta /DVersion=X.Y.Z
#endif
#define Raiz ".."

[Setup]
AppId={{8F2C4E1A-6B7D-4C3E-9A51-2D7E0B6F4C19}
AppName=iCopy
AppVersion={#Version}
AppVerName=iCopy {#Version}
AppPublisher=Z1Code
AppPublisherURL=https://github.com/Z1Code/iCopy
AppUpdatesURL=https://github.com/Z1Code/iCopy/releases
; Misma carpeta que usaba instalar.ps1: actualiza esa instalación sin migrar nada.
DefaultDirName={localappdata}\Programs\icopia
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#Raiz}\dist
OutputBaseFilename=iCopy-Setup-{#Version}
SetupIconFile={#Raiz}\icopia\assets\icopia.ico
UninstallDisplayIcon={app}\icopia-bandeja.exe
UninstallDisplayName=iCopy
VersionInfoVersion={#Version}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=no

[Languages]
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
es.Autoarranque=Iniciar iCopy con Windows
en.Autoarranque=Start iCopy with Windows
es.Abrir=Abrir iCopy
en.Abrir=Open iCopy
es.BorrarDatos=¿Borrar también la configuración y la sesión de iCloud de iCopy?%n%nTus fotos no se tocan.
en.BorrarDatos=Also delete iCopy's settings and iCloud session?%n%nYour photos are not touched.

[Tasks]
Name: "autoarranque"; Description: "{cm:Autoarranque}"; Check: PrimeraInstalacion

[Files]
Source: "{#Raiz}\icopia\target\release\icopia.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Raiz}\icopia\target\release\icopia-bandeja.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Raiz}\icopia\target\release\icopia-login.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Raiz}\puente\dist\puente-icloud.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\iCopy"; Filename: "{app}\icopia-bandeja.exe"

[Registry]
; Solo en la primera instalación: después lo decide la persona desde el menú del icono.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "icopia"; ValueData: """{app}\icopia-bandeja.exe"""; Tasks: autoarranque

[Run]
Filename: "{app}\icopia-bandeja.exe"; Description: "{cm:Abrir}"; Flags: nowait postinstall; Check: DebeArrancar

[Code]
var
  Primera: Boolean;

function InitializeSetup(): Boolean;
begin
  Primera := not FileExists(ExpandConstant('{localappdata}\Programs\icopia\icopia-bandeja.exe'));
  Result := True;
end;

function PrimeraInstalacion(): Boolean;
begin
  Result := Primera;
end;

{ /SINARRANCAR=1: no abrir la app al terminar (pruebas). }
function DebeArrancar(): Boolean;
begin
  Result := ExpandConstant('{param:SINARRANCAR|0}') <> '1';
end;

{ Cierra solo los procesos de ESTA instalación (nunca los de otra carpeta). }
procedure CerrarIcopy();
var
  Codigo: Integer;
begin
  Exec('powershell.exe',
    '-NoProfile -NonInteractive -Command "Get-Process icopia-bandeja,icopia-login,icopia,puente-icloud -ErrorAction SilentlyContinue | Where-Object { $_.Path -like ''' +
    ExpandConstant('{app}') + '\*'' } | Stop-Process -Force; Start-Sleep -Milliseconds 700"',
    '', SW_HIDE, ewWaitUntilTerminated, Codigo);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CerrarIcopy();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  CerrarIcopy();
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Valor: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    { Quitar el arranque con Windows solo si apunta a esta instalación. }
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'icopia', Valor) and
       (Pos(Lowercase(ExpandConstant('{app}')), Lowercase(Valor)) > 0) then
      RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'icopia');
    if (not UninstallSilent) and
       (MsgBox(CustomMessage('BorrarDatos'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
      DelTree(ExpandConstant('{userappdata}\icopia'), True, True, True);
  end;
end;
