; Instalador de Factorio Discord Rich Presence (Inno Setup 6).
;
; Compilar (antes hace falta `cargo build --release`):
;     ISCC.exe /DAppVersion=0.1.0 installer\factorio-discord-rp.iss
; Sale en dist\FactorioDiscordRP-Setup-<versión>.exe
;
; El MOD NO va en el instalador: se instala desde el Mod Portal de Factorio.
;
; Toda la lógica de Steam vive en la propia aplicación (--apply, --uninstall,
; --print-command…), que se puede probar con tests. Este script sólo orquesta el
; asistente y decide qué hacer según el código de salida:
;     0 hecho · 10 Steam abierto · 11 sin Steam o Factorio · 12 error de escritura

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#define AppName "Factorio Discord Rich Presence"
#define AppExe "factorio-discord-rp.exe"
#define ModUrl "https://mods.factorio.com/mod/discord-rich-presence"

[Setup]
AppId={{B7C1E5D2-3F4A-4B8E-9D6C-2A5F8E1C7D90}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=enrik-0
AppPublisherURL=https://github.com/enrik-0/factorio-discord-rich-presence
DefaultDirName={localappdata}\Programs\Factorio Discord RP
DisableProgramGroupPage=yes
; Instalación por usuario: no pide administrador.
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=FactorioDiscordRP-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Messages]
FinishedLabel=[name] se ha instalado correctamente.%n%nSólo falta un paso: instalar el mod «Discord Rich Presence» desde el Mod Portal de Factorio (o desde el propio juego, en Mods).

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Parameters: "--tray"

[Run]
Filename: "{#ModUrl}"; Description: "Abrir la página del mod en el Mod Portal (hace falta instalarlo)"; Flags: shellexec postinstall skipifsilent

[Code]
const
  ModeAuto = 0;
  ModeManual = 1;

var
  HowPage: TInputOptionWizardPage;
  CommandPage: TWizardPage;
  CommandIntro: TNewStaticText;
  CommandMemo: TNewMemo;
  CopyButton: TNewButton;
  // ¿Hay que enseñar el comando para pegarlo a mano?
  NeedManual: Boolean;
  // Por qué no se pudo configurar Steam solo (vacío si se eligió lo manual).
  ManualReason: String;

function AppPath: String;
begin
  Result := ExpandConstant('{app}\{#AppExe}');
end;

// Lanza la aplicación con parámetros y espera. Devuelve su código de salida, o -1
// si ni siquiera se pudo ejecutar.
function RunApp(Params: String): Integer;
var
  Code: Integer;
begin
  if Exec(AppPath, Params, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Result := Code
  else
    Result := -1;
end;

procedure StopRunningApp;
var
  Code: Integer;
begin
  // Si la bandeja está abierta, el .exe está bloqueado y no se podría reemplazar.
  Exec(ExpandConstant('{cmd}'), '/C taskkill /F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure CopyCommandClick(Sender: TObject);
begin
  RunApp('--copy-command');
end;

procedure InitializeWizard;
begin
  HowPage := CreateInputOptionPage(wpSelectDir,
    'Cómo configurar Steam',
    '¿Automático o manual?',
    'Elige una opción. Podrás cambiarla volviendo a ejecutar este instalador.',
    True, False);
  HowPage.Add('Automático (recomendado)');
  HowPage.Add('Manual');
  HowPage.SelectedValueIndex := ModeAuto;

  // Se coloca tras la instalación de los archivos y sólo aparece si hace falta.
  CommandPage := CreateCustomPage(wpInstalling,
    'Un último paso: pegar el comando en Steam',
    'Copia esta línea en las opciones de lanzamiento de Factorio.');

  CommandIntro := TNewStaticText.Create(CommandPage);
  CommandIntro.Parent := CommandPage.Surface;
  CommandIntro.Left := 0;
  CommandIntro.Top := 0;
  CommandIntro.Width := CommandPage.SurfaceWidth;
  CommandIntro.AutoSize := False;
  CommandIntro.WordWrap := True;
  CommandIntro.Height := ScaleY(70);

  CommandMemo := TNewMemo.Create(CommandPage);
  CommandMemo.Parent := CommandPage.Surface;
  CommandMemo.Left := 0;
  CommandMemo.Top := ScaleY(76);
  CommandMemo.Width := CommandPage.SurfaceWidth;
  CommandMemo.Height := ScaleY(60);
  CommandMemo.ReadOnly := True;
  CommandMemo.ScrollBars := ssVertical;
  CommandMemo.WordWrap := True;

  CopyButton := TNewButton.Create(CommandPage);
  CopyButton.Parent := CommandPage.Surface;
  CopyButton.Left := 0;
  CopyButton.Top := ScaleY(144);
  CopyButton.Width := ScaleX(190);
  CopyButton.Height := ScaleY(26);
  CopyButton.Caption := 'Copiar al portapapeles';
  CopyButton.OnClick := @CopyCommandClick;
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = CommandPage.ID) and (not NeedManual);
end;

// Rellena la caja con la línea que calcula la propia aplicación (ya respeta las
// opciones que el usuario tuviera). Si no se pudiera, se compone la simple.
procedure FillCommand;
var
  TempFile: String;
  Raw: AnsiString;
  Code: Integer;
begin
  TempFile := ExpandConstant('{tmp}\comando.txt');
  DeleteFile(TempFile);
  if Exec(ExpandConstant('{cmd}'),
       '/C ""' + AppPath + '" --print-command > "' + TempFile + '""',
       '', SW_HIDE, ewWaitUntilTerminated, Code)
     and LoadStringFromFile(TempFile, Raw) then
    // La aplicación imprime en UTF-8; sin decodificar, un acento en la ruta se rompe.
    CommandMemo.Text := Trim(UTF8Decode(Raw))
  else
    CommandMemo.Text := '"' + AppPath + '" %command%';

  if ManualReason <> '' then
    CommandIntro.Caption := ManualReason + #13#10#13#10 +
      'Pega esta línea en Steam: clic derecho en Factorio > Propiedades > Opciones de lanzamiento.'
  else
    CommandIntro.Caption :=
      'Pega esta línea en Steam: clic derecho en Factorio > Propiedades > Opciones de lanzamiento.';
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = CommandPage.ID then
    FillCommand;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopRunningApp;
  Result := '';
end;

// Configura Steam solo. Si algo falla no se aborta la instalación: se enseña el
// comando completo para pegarlo a mano.
procedure ConfigureSteam;
var
  Code: Integer;
begin
  Code := RunApp('--apply');

  if Code = 10 then
  begin
    // Steam abierto: sólo se cierra con permiso, y se reabre después.
    if SuppressibleMsgBox(
         'Steam está abierto, y para cambiar sus opciones hay que cerrarlo un momento.' + #13#10#13#10 +
         '¿Quieres que lo cierre y lo vuelva a abrir por ti?',
         mbConfirmation, MB_YESNO, IDNO) = IDYES then
      Code := RunApp('--apply --close-steam --restart-steam')
    else
    begin
      NeedManual := True;
      ManualReason := 'No se ha tocado Steam porque está abierto.';
      Exit;
    end;
  end;

  if Code <> 0 then
  begin
    NeedManual := True;
    case Code of
      11: ManualReason := 'No se encontró Steam o Factorio en este equipo.';
    else
      ManualReason := 'No se pudo configurar Steam automáticamente (código ' + IntToStr(Code) + ').';
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep <> ssPostInstall then
    Exit;

  // El autoarranque no se ofrece como opción propia; queda siempre desactivado
  // (una reinstalación sobre una versión que sí lo tuviera lo apaga también).
  RunApp('--autostart off');

  case HowPage.SelectedValueIndex of
    ModeAuto:
      ConfigureSteam;
    ModeManual:
      begin
        NeedManual := True;
        ManualReason := '';
      end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
  Exe: String;
begin
  if CurUninstallStep <> usUninstall then
    Exit;

  Exe := ExpandConstant('{app}\{#AppExe}');
  Exec(ExpandConstant('{cmd}'), '/C taskkill /F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec(Exe, '--autostart off', '', SW_HIDE, ewWaitUntilTerminated, Code);

  // Quita sólo nuestra parte de las opciones de Steam, dejando las del usuario.
  if Exec(Exe, '--uninstall', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 10) then
  begin
    if UninstallSilent then
      Exit;
    if MsgBox(
         'Steam está abierto. Para quitar la aplicación de las opciones de lanzamiento de Factorio ' +
         'hay que cerrarlo un momento.' + #13#10#13#10 +
         '¿Quieres que lo cierre y lo vuelva a abrir por ti?',
         mbConfirmation, MB_YESNO) = IDYES then
      Exec(Exe, '--uninstall --close-steam --restart-steam', '', SW_HIDE, ewWaitUntilTerminated, Code)
    else
      MsgBox(
        'Ha quedado la aplicación en las opciones de lanzamiento de Factorio en Steam. ' +
        'Bórrala a mano (Factorio > Propiedades > Opciones de lanzamiento) o Steam no arrancará el juego.',
        mbInformation, MB_OK);
  end;
end;
