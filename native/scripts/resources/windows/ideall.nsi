Unicode true
!include "MUI2.nsh"

!ifndef IDEALL_VERSION
  !error "IDEALL_VERSION is required"
!endif
!ifndef IDEALL_VERSION_NUMERIC
  !error "IDEALL_VERSION_NUMERIC is required"
!endif
!ifndef IDEALL_BINARY
  !error "IDEALL_BINARY is required"
!endif
!ifndef IDEALL_LICENSE
  !error "IDEALL_LICENSE is required"
!endif
!ifndef IDEALL_THIRD_PARTY_LICENSES
  !error "IDEALL_THIRD_PARTY_LICENSES is required"
!endif
!ifndef IDEALL_README
  !error "IDEALL_README is required"
!endif
!ifndef IDEALL_ICON
  !error "IDEALL_ICON is required"
!endif
!ifndef IDEALL_OUTPUT
  !error "IDEALL_OUTPUT is required"
!endif

Name "ideall"
OutFile "${IDEALL_OUTPUT}"
InstallDir "$PROGRAMFILES64\ideall"
InstallDirRegKey HKLM "Software\ideall" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
Icon "${IDEALL_ICON}"
UninstallIcon "${IDEALL_ICON}"
VIProductVersion "${IDEALL_VERSION_NUMERIC}.0"
VIAddVersionKey /LANG=1033 "ProductName" "ideall"
VIAddVersionKey /LANG=1033 "CompanyName" "ideall contributors"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Copyright ideall contributors"
VIAddVersionKey /LANG=1033 "FileDescription" "ideall installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${IDEALL_VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${IDEALL_VERSION}"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "ideall" MainSection
  SectionIn RO
  SetShellVarContext all
  SetRegView 64
  SetOutPath "$INSTDIR"
  File /oname=ideall.exe "${IDEALL_BINARY}"
  File /oname=LICENSE "${IDEALL_LICENSE}"
  File /oname=THIRD_PARTY_LICENSES.md "${IDEALL_THIRD_PARTY_LICENSES}"
  File /oname=README.md "${IDEALL_README}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\ideall" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ideall" "DisplayName" "ideall"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ideall" "DisplayVersion" "${IDEALL_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ideall" "Publisher" "ideall contributors"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ideall" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ideall" "UninstallString" '"$INSTDIR\uninstall.exe"'
  CreateShortcut "$SMPROGRAMS\ideall.lnk" "$INSTDIR\ideall.exe"
  CreateShortcut "$DESKTOP\ideall.lnk" "$INSTDIR\ideall.exe"
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  SetRegView 64
  Delete "$DESKTOP\ideall.lnk"
  Delete "$SMPROGRAMS\ideall.lnk"
  Delete "$INSTDIR\ideall.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\THIRD_PARTY_LICENSES.md"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ideall"
  DeleteRegKey HKLM "Software\ideall"
SectionEnd
