Unicode true
RequestExecutionLevel user

!include "MUI2.nsh"

Name "Unique UID"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Unique UID"
InstallDirRegKey HKCU "Software\Unique UID" "InstallDir"
!define MUI_ICON "..\assets\icon.ico"
!define MUI_UNICON "..\assets\icon.ico"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Unique UID"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
    SetOutPath "$INSTDIR"
    File /oname=UniqueUID.exe "..\target\release\unique-uid.exe"
    File "..\LICENSE"
    File "..\assets\Inter-OFL.txt"
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    CreateShortCut "$SMPROGRAMS\Unique UID.lnk" "$INSTDIR\UniqueUID.exe"
    WriteRegStr HKCU "Software\Unique UID" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Unique UID"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\UniqueUID.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
    Delete "$SMPROGRAMS\Unique UID.lnk"
    Delete "$INSTDIR\UniqueUID.exe"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\Inter-OFL.txt"
    Delete "$INSTDIR\Uninstall.exe"
    RMDir "$INSTDIR"
    DeleteRegKey HKCU "Software\Unique UID"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
