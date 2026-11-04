; The installer copy of ScreenGauge, from GitHub Releases (WPC-337): installed for the signed-in user alone, with no
; administrator, in %LOCALAPPDATA%\Programs\ScreenGauge, with a Start menu entry, an entry in Installed apps and an
; uninstaller. Built by make-installer.ps1 (see README.md), which passes the exe and the app's version; the tests in
; crates/screengauge/src/installer.rs hold this script to the code.

#ifndef ScreenGaugeExe
  #error Build with make-installer.ps1, which passes /DScreenGaugeExe=<path of ScreenGauge.exe>.
#endif
#ifndef CargoVersion
  #error Build with make-installer.ps1, which passes /DCargoVersion=<the app's version>.
#endif

; The version and the publisher are the exe's version resource's (WPC-304). The four-part version is the Store
; copy's, whose package version is the app's with a fourth part of 0.
#define AppVersion GetVersionNumbersString(ScreenGaugeExe)
#define Publisher GetFileCompanyString(ScreenGaugeExe)
#if AppVersion != CargoVersion + ".0"
  #error The exe's version is not the app's: rebuild it.
#endif
#if Publisher != "WPC Solutions"
  #error The exe's publisher is not WPC Solutions.
#endif

; The resident window's class (crates/screengauge/src/resident.rs), which a running ScreenGauge always has.
#define ResidentClass "ScreenGauge.Resident"

; The Store copy's package family, which Windows names after the package's name and publisher
; (packaging/msix/AppxManifest.xml). Setup asks Windows whether the user has it installed (WPC-338).
#define StorePackageFamily "WPCSolutions.ScreenGauge_8rc0q4yanedb4"

[Setup]
; Never changed: it names the uninstall entry, HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{AppId}_is1,
; which the Store copy looks for (WPC-339), and a new one would install a second copy next to this one.
AppId={{1DF55A92-C67C-41D9-B304-B2442423D407}
AppName=ScreenGauge
AppVersion={#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL=https://github.com/carreirodev/screengauge
AppSupportURL=https://github.com/carreirodev/screengauge/issues
AppUpdatesURL=https://github.com/carreirodev/screengauge/releases/latest
VersionInfoVersion={#AppVersion}
; Installed apps shows the name alone, as for the Store copy, with the version on its own line.
UninstallDisplayName=ScreenGauge
UninstallDisplayIcon={app}\ScreenGauge.exe
SetupIconFile=..\..\assets\ScreenGauge.ico
; For the user alone: %LOCALAPPDATA%\Programs\ScreenGauge, and the uninstall entry in HKCU.
PrivilegesRequired=lowest
DefaultDirName={autopf}\ScreenGauge
DisableDirPage=yes
DisableProgramGroupPage=yes
; Windows 11 and later (build 22000 is its first), x64, and Windows 11 on Arm through Windows' emulation.
MinVersion=10.0.22000
ArchitecturesAllowed=x64compatible
; Setup itself stays 32-bit and installs in 64-bit mode: the 64-bit registry view, where ScreenGauge writes, and an
; uninstaller of 4.6 MB instead of 7.0 MB, so the installed folder keeps room under its 10 MB (measured in WPC-337:
; 5.76 MB against 8.22 MB, and a 2.6 MB download against 3.4 MB).
SetupArchitecture=x86
ArchitecturesInstallIn64BitMode=x64compatible
; ScreenGauge is closed by PrepareToInstall below. Restart Manager stays as a fallback, but never starts it again:
; started with no arguments, ScreenGauge would open its tools over the installer.
RestartApplications=no
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputBaseFilename=ScreenGauge-{#CargoVersion}-setup

[Messages]
; What Setup says on Windows 10, whose build is below MinVersion's (measured: this one, not WinVersionTooLowError).
WindowsVersionNotSupported=ScreenGauge requires Windows 11 or later.

[CustomMessages]
; The box Setup shows first when the user has the Store copy (WPC-338), with Cancel as its default button.
StoreCopyInstalled=ScreenGauge is already installed from the Microsoft Store, which keeps it up to date. Installing it here too leaves two copies on this PC. We recommend canceling.
InstallAnyway=Install anyway

[Files]
Source: "{#ScreenGaugeExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Opening it from the Start menu is a call, as the global shortcut and the tray icon (WPC-301).
Name: "{autoprograms}\ScreenGauge"; Filename: "{app}\ScreenGauge.exe"

[Run]
; Finish opens ScreenGauge as the Start menu does, unless the install is silent (owner's choice in WPC-337).
Filename: "{app}\ScreenGauge.exe"; Description: "Launch ScreenGauge"; Flags: nowait postinstall skipifsilent

[Registry]
; ScreenGauge writes these itself, at every start (ADR 0003) and when "Start with Windows" is turned on (ADR 0004):
; the installer never creates them, and the uninstaller deletes them. Only the installer copy's: the Store copy's
; package declares its own, and Windows removes them with it. StartupApproved\Run is Windows' and stays.
Root: HKCU64; Subkey: "Software\Classes\AppUserModelId\WPCSolutions.ScreenGauge"; Flags: dontcreatekey uninsdeletekey
Root: HKCU64; Subkey: "Software\Classes\CLSID\{{3BEB9695-DA23-4599-B10C-7CD6CD96E50C}"; Flags: dontcreatekey uninsdeletekey
Root: HKCU64; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "ScreenGauge"; Flags: dontcreatekey uninsdeletevalue

[UninstallDelete]
; The installer copy's settings (crates/screengauge/src/settings_file.rs): the file, the one a save writes before
; replacing it, and the damaged ones kept aside; then the notifications' header icons. Only what ScreenGauge writes,
; so a file the user saved in the folder stays, and the folder with it.
Type: files; Name: "{localappdata}\ScreenGauge\settings*.ini"
Type: files; Name: "{localappdata}\ScreenGauge\settings.ini.tmp"
Type: files; Name: "{localappdata}\ScreenGauge\notification-icon-*.png"
Type: files; Name: "{localappdata}\ScreenGauge\notification-icon-*.png.tmp"
Type: dirifempty; Name: "{localappdata}\ScreenGauge"

[Code]
const
  WM_CLOSE = $0010;
  SYNCHRONIZE = $00100000;
  PROCESS_QUERY_LIMITED_INFORMATION = $1000;
  WAIT_OBJECT_0 = 0;
  { How long ScreenGauge has to exit once asked. }
  CLOSE_TIMEOUT_MS = 10000;
  ERROR_INSUFFICIENT_BUFFER = 122;

function GetWindowThreadProcessId(Window: HWND; var ProcessId: DWORD): DWORD;
  external 'GetWindowThreadProcessId@user32.dll stdcall';
function OpenProcess(Access: DWORD; Inherit: Integer; ProcessId: DWORD): HANDLE;
  external 'OpenProcess@kernel32.dll stdcall';
function QueryFullProcessImageName(Process: HANDLE; Flags: DWORD; Name: String; var Size: DWORD): Integer;
  external 'QueryFullProcessImageNameW@kernel32.dll stdcall';
function WaitForSingleObject(Handle: HANDLE; Milliseconds: DWORD): DWORD;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: HANDLE): Integer;
  external 'CloseHandle@kernel32.dll stdcall';
{ FullNames and Buffer are pointers, passed as 0, which is null: 32 bits wide, as in this 32-bit Setup. }
function GetPackagesByPackageFamily(FamilyName: String; var Count: Cardinal; FullNames: Cardinal;
  var BufferLength: Cardinal; Buffer: Cardinal): Longint;
  external 'GetPackagesByPackageFamily@kernel32.dll stdcall';

{ Whether the user has the Store copy installed, asked of Windows by its package family, which needs no administrator.
  False when Windows can't say: the box is advice, so Setup goes on as without the Store copy, and logs why. }
function StoreCopyInstalled: Boolean;
var
  Count, BufferLength: Cardinal;
  Error: Longint;
begin
  Result := False;
  Count := 0;
  BufferLength := 0;
  { With no room for the names, Windows only counts them: ERROR_SUCCESS with none, ERROR_INSUFFICIENT_BUFFER with
    some. }
  Error := GetPackagesByPackageFamily('{#StorePackageFamily}', Count, 0, BufferLength, 0);
  if Error = ERROR_INSUFFICIENT_BUFFER then
    Result := Count > 0
  else if Error <> 0 then
    Log(Format('Windows didn''t say whether the Store copy is installed (error %d): not asking.', [Error]));
  Log(Format('Store copy packages installed for this user: %d.', [Count]));
end;

{ The box Setup shows when the user has the Store copy: True for Install anyway, False for Cancel, its default button,
  and for Esc and the close button too. Not MsgBox, whose buttons are Windows' own (Yes and No), nor
  TaskDialogMsgBox, which shows custom labels as command links, the first one the default. }
function InstallNextToStoreCopy: Boolean;
var
  Form: TSetupForm;
  Text: TNewStaticText;
  InstallButton, CancelButton: TNewButton;
  ButtonWidth: Integer;
begin
  { Sizes as Windows' own message boxes, which Setup scales to the screen's. }
  Form := CreateCustomForm(400, 124, False, False);
  try
    Form.Caption := FmtMessage(SetupMessage(msgSetupWindowTitle), ['ScreenGauge']);

    Text := TNewStaticText.Create(Form);
    Text.Parent := Form;
    Text.Left := ScaleX(16);
    Text.Top := ScaleY(16);
    Text.Width := Form.ClientWidth - ScaleX(2 * 16);
    Text.AutoSize := False;
    Text.WordWrap := True;
    Text.Height := Form.ClientHeight - ScaleY(16 + 16 + 23 + 16);
    Text.Caption := CustomMessage('StoreCopyInstalled');

    InstallButton := TNewButton.Create(Form);
    InstallButton.Parent := Form;
    InstallButton.Caption := CustomMessage('InstallAnyway');
    InstallButton.ModalResult := mrOk;

    CancelButton := TNewButton.Create(Form);
    CancelButton.Parent := Form;
    CancelButton.Caption := SetupMessage(msgButtonCancel);
    CancelButton.ModalResult := mrCancel;
    CancelButton.Default := True;
    CancelButton.Cancel := True;

    { Cancel last, on the right, as in Windows' own message boxes. }
    ButtonWidth := Form.CalculateButtonWidth([InstallButton.Caption, CancelButton.Caption]);
    CancelButton.SetBounds(Form.ClientWidth - ScaleX(16) - ButtonWidth, Form.ClientHeight - ScaleY(16 + 23),
      ButtonWidth, ScaleY(23));
    InstallButton.SetBounds(CancelButton.Left - ScaleX(8) - ButtonWidth, CancelButton.Top, ButtonWidth, ScaleY(23));

    Form.ActiveControl := CancelButton;
    Result := Form.ShowModal = mrOk;
  finally
    Form.Free;
  end;
end;

{ Before the wizard: someone who has the Store copy is told, and Cancel ends Setup with nothing installed. A silent
  install doesn't ask. }
function InitializeSetup: Boolean;
begin
  Result := True;
  if not StoreCopyInstalled then
    Exit;
  if WizardSilent then begin
    Log('The Store copy is installed, and the install is silent: not asking.');
    Exit;
  end;
  Result := InstallNextToStoreCopy;
  if Result then
    Log('The Store copy is installed: the user chose Install anyway.')
  else
    Log('The Store copy is installed: the user canceled.');
end;

{ Asks ScreenGauge to exit when it runs from Exe, as its tray menu's Exit does, and waits for it to end. One copy runs
  at a time, the Store copy included, so there is one resident window at most; when it is the Store copy's, it is left
  running. False when ScreenGauge runs from Exe and didn't end. }
function CloseScreenGauge(const Exe: String): Boolean;
var
  Window: HWND;
  ProcessId, Size: DWORD;
  Process: HANDLE;
  Path: String;
begin
  Result := True;
  Window := FindWindowByClassName('{#ResidentClass}');
  if Window = 0 then
    Exit;
  ProcessId := 0;
  GetWindowThreadProcessId(Window, ProcessId);
  Process := OpenProcess(SYNCHRONIZE or PROCESS_QUERY_LIMITED_INFORMATION, 0, ProcessId);
  if Process = 0 then begin
    { The same user's process: only one started as administrator can't be opened, and it isn't this copy's. }
    Log('ScreenGauge runs, but its process could not be opened: left running.');
    Exit;
  end;
  try
    Size := 32768;
    SetLength(Path, Size);
    if QueryFullProcessImageName(Process, 0, Path, Size) = 0 then begin
      Log('ScreenGauge runs, but its path could not be read: left running.');
      Exit;
    end;
    SetLength(Path, Size);
    { Windows compares paths ignoring case, accented letters included. }
    if AnsiLowercase(Path) <> AnsiLowercase(Exe) then begin
      Log('ScreenGauge runs from another folder, left running: ' + Path);
      Exit;
    end;
    Log('Asking ScreenGauge to exit: ' + Path);
    PostMessage(Window, WM_CLOSE, 0, 0);
    Result := WaitForSingleObject(Process, CLOSE_TIMEOUT_MS) = WAIT_OBJECT_0;
    if Result then
      Log('ScreenGauge exited.')
    else
      Log('ScreenGauge did not exit.');
  finally
    CloseHandle(Process);
  end;
end;

{ Before installing over a ScreenGauge that runs from this folder. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if CloseScreenGauge(ExpandConstant('{app}\ScreenGauge.exe')) then
    Result := ''
  else
    Result := 'ScreenGauge is still running. Exit it from its tray icon''s menu, then run Setup again.';
end;

{ Before deleting the files, so the exe isn't in use. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    if not CloseScreenGauge(ExpandConstant('{app}\ScreenGauge.exe')) then
      RaiseException('ScreenGauge is still running. Exit it from its tray icon''s menu, then uninstall it again.');
end;
