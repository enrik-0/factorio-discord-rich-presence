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

; El primero de la lista es el que se usa por defecto en una instalación
; silenciosa sin /LANG, y el preseleccionado en el selector de idioma cuando
; el Windows del usuario no coincide con ninguno de los dos declarados.
; Inglés primero: la mayoría de quien lo descargue no tendrá Windows en
; español ni en inglés necesariamente, pero el proyecto se anuncia en inglés.
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Messages]
english.FinishedLabel=All done! [name] is installed.%n%nOne thing left: install the mod from the Factorio Mod Portal, or from the game itself, under Mods.
spanish.FinishedLabel=¡Listo! [name] ya está instalado.%n%nSólo te falta el mod: instálalo desde el Mod Portal de Factorio, o desde el propio juego, en Mods.

; Todo lo que el propio [Code] muestra en pantalla vive aquí, con las dos
; variantes exigidas por Inno cuando hay más de un idioma declarado.
[CustomMessages]
english.HowCaption=How to set up Steam
english.HowDescription=Automatic or manual?
english.HowSubtext=Pick one. You can change it later by running this installer again.
english.OptAuto=Automatic (recommended)
english.OptManual=Manual
english.CommandCaption=One last step
english.CommandDescription=Paste this line into Factorio's launch options.
english.CopyButton=Copy to clipboard
english.PasteYourself=Paste it yourself: right-click Factorio > Properties > Launch Options.
english.PasteIntro=Copy this line and paste it in Steam: right-click Factorio > Properties > Launch Options.
english.CloseSteamAsk=To set up Steam automatically I need to close it for a moment.%n%nShall I close it and reopen it for you once I'm done?
english.ReasonSteamOpen=Steam was open, so I haven't touched anything.
english.ReasonNotFound=I couldn't find Steam or Factorio on this computer.
english.ReasonOther=I couldn't set up Steam for you (code %1).
english.UninstallCloseSteamAsk=To remove it from the launch options I need to close Steam for a moment.%n%nShall I close it and reopen it for you once I'm done?
english.UninstallLeftover=The app is still in Factorio's launch options, in Steam.%n%nRemove it yourself when you get a chance: right-click Factorio > Properties > Launch Options. Otherwise, Steam won't be able to start the game.
english.OpenModPage=Open the mod's page to install it

spanish.HowCaption=Cómo configurar Steam
spanish.HowDescription=¿Automático o manual?
spanish.HowSubtext=Elige una opción. Podrás cambiarla volviendo a ejecutar este instalador.
spanish.OptAuto=Automático (recomendado)
spanish.OptManual=Manual
spanish.CommandCaption=Un último paso
spanish.CommandDescription=Pega esta línea en las opciones de lanzamiento de Factorio.
spanish.CopyButton=Copiar al portapapeles
spanish.PasteYourself=Pégala tú: clic derecho en Factorio > Propiedades > Opciones de lanzamiento.
spanish.PasteIntro=Copia esta línea y pégala en Steam: clic derecho en Factorio > Propiedades > Opciones de lanzamiento.
spanish.CloseSteamAsk=Para configurar Steam automáticamente necesito cerrarlo un momento.%n%n¿Lo cierro y te lo vuelvo a abrir en cuanto termine?
spanish.ReasonSteamOpen=Steam estaba abierto, así que no he tocado nada.
spanish.ReasonNotFound=No he encontrado Steam ni Factorio en este equipo.
spanish.ReasonOther=No he podido configurar Steam por ti (código %1).
spanish.UninstallCloseSteamAsk=Para quitarla de las opciones de lanzamiento necesito cerrar Steam un momento.%n%n¿Lo cierro y te lo vuelvo a abrir en cuanto termine?
spanish.UninstallLeftover=La aplicación sigue en las opciones de lanzamiento de Factorio, en Steam.%n%nBórrala tú cuando puedas: clic derecho en Factorio > Propiedades > Opciones de lanzamiento. Si no, Steam no arrancará el juego.
spanish.OpenModPage=Abrir la página del mod para instalarlo

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Parameters: "--tray"

[Run]
Filename: "{#ModUrl}"; Description: "{cm:OpenModPage}"; Flags: shellexec postinstall skipifsilent

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
    CustomMessage('HowCaption'),
    CustomMessage('HowDescription'),
    CustomMessage('HowSubtext'),
    True, False);
  HowPage.Add(CustomMessage('OptAuto'));
  HowPage.Add(CustomMessage('OptManual'));
  HowPage.SelectedValueIndex := ModeAuto;

  // Se coloca tras la instalación de los archivos y sólo aparece si hace falta.
  CommandPage := CreateCustomPage(wpInstalling,
    CustomMessage('CommandCaption'),
    CustomMessage('CommandDescription'));

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
  CopyButton.Caption := CustomMessage('CopyButton');
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
    CommandIntro.Caption := ManualReason + #13#10#13#10 + CustomMessage('PasteYourself')
  else
    CommandIntro.Caption := CustomMessage('PasteIntro');
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
         CustomMessage('CloseSteamAsk'),
         mbConfirmation, MB_YESNO, IDNO) = IDYES then
      Code := RunApp('--apply --close-steam --restart-steam')
    else
    begin
      NeedManual := True;
      ManualReason := CustomMessage('ReasonSteamOpen');
      Exit;
    end;
  end;

  if Code <> 0 then
  begin
    NeedManual := True;
    case Code of
      11: ManualReason := CustomMessage('ReasonNotFound');
    else
      ManualReason := FmtMessage(CustomMessage('ReasonOther'), [IntToStr(Code)]);
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
         CustomMessage('UninstallCloseSteamAsk'),
         mbConfirmation, MB_YESNO) = IDYES then
      Exec(Exe, '--uninstall --close-steam --restart-steam', '', SW_HIDE, ewWaitUntilTerminated, Code)
    else
      MsgBox(CustomMessage('UninstallLeftover'), mbInformation, MB_OK);
  end;
end;
