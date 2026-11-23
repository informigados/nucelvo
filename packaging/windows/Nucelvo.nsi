Unicode true
RequestExecutionLevel user
CRCCheck force
SetCompressor /SOLID lzma

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!define PRODUCT_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Nucelvo"

Name "Nucelvo ${NUCELVO_VERSION}"
OutFile "${NUCELVO_INSTALLER}"
InstallDir "$LOCALAPPDATA\Programs\Nucelvo"
InstallDirRegKey HKCU "${PRODUCT_KEY}" "InstallLocation"
VIProductVersion "${NUCELVO_FILE_VERSION}"
VIAddVersionKey "ProductName" "Nucelvo"
VIAddVersionKey "CompanyName" "INformigados"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 INformigados"
VIAddVersionKey "FileDescription" "Nucelvo Setup"
VIAddVersionKey "FileVersion" "${NUCELVO_VERSION}"
VIAddVersionKey "ProductVersion" "${NUCELVO_VERSION}"

!define MUI_ICON "${NUCELVO_ICON}"
!define MUI_UNICON "${NUCELVO_ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\nucelvo.exe"
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!define MUI_LANGDLL_REGISTRY_ROOT HKCU
!define MUI_LANGDLL_REGISTRY_KEY "${PRODUCT_KEY}"
!define MUI_LANGDLL_REGISTRY_VALUENAME "InstallerLanguage"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "PortugueseBR"

LangString WrongArchitecture ${LANG_ENGLISH} "This version of Nucelvo requires native x64 Windows."
LangString WrongArchitecture ${LANG_PORTUGUESEBR} "Esta versão do Nucelvo requer Windows x64 nativo."
LangString DirectoryInUse ${LANG_ENGLISH} "Choose an empty folder for Nucelvo."
LangString DirectoryInUse ${LANG_PORTUGUESEBR} "Escolha uma pasta vazia para instalar o Nucelvo."
LangString DifferentLocation ${LANG_ENGLISH} "Nucelvo is already installed in $0. Use that folder or uninstall the existing copy first."
LangString DifferentLocation ${LANG_PORTUGUESEBR} "O Nucelvo já está instalado em $0. Use essa pasta ou desinstale a cópia atual primeiro."
LangString InstallFailed ${LANG_ENGLISH} "Setup could not finish. Close Nucelvo and check that the installation folder is writable."
LangString InstallFailed ${LANG_PORTUGUESEBR} "A instalação não pôde ser concluída. Feche o Nucelvo e verifique se você pode gravar na pasta de instalação."
LangString UninstallFailed ${LANG_ENGLISH} "Nucelvo could not be removed. Close the app and try again."
LangString UninstallFailed ${LANG_PORTUGUESEBR} "Não foi possível remover o Nucelvo. Feche o aplicativo e tente novamente."

Function .onInit
    SetRegView 64
    SetShellVarContext current
    IfSilent skipLanguage
    !insertmacro MUI_LANGDLL_DISPLAY
skipLanguage:
    ${IfNot} ${IsNativeAMD64}
        MessageBox MB_ICONSTOP "$(WrongArchitecture)" /SD IDOK
        SetErrorLevel 1
        Abort
    ${EndIf}
FunctionEnd

Section "Nucelvo"
    ReadRegStr $0 HKCU "${PRODUCT_KEY}" "InstallLocation"
    ${If} $0 != ""
        ${If} $0 != $INSTDIR
            MessageBox MB_ICONSTOP "$(DifferentLocation)" /SD IDOK
            SetErrorLevel 1
            Abort
        ${EndIf}
    ${Else}
        FindFirst $1 $2 "$INSTDIR\*"
        ${While} $2 != ""
            ${If} $2 != "."
            ${AndIf} $2 != ".."
                FindClose $1
                MessageBox MB_ICONSTOP "$(DirectoryInUse)" /SD IDOK
                SetErrorLevel 1
                Abort
            ${EndIf}
            ClearErrors
            FindNext $1 $2
            IfErrors endSearch
        ${EndWhile}
endSearch:
        FindClose $1
    ${EndIf}

    ClearErrors
    SetOutPath "$INSTDIR"
    SetOverwrite try
    ; Extract the executable first. A running copy blocks an update before metadata changes.
    File "${NUCELVO_PAYLOAD}\nucelvo.exe"
    IfErrors failed
    File "${NUCELVO_PAYLOAD}\LICENSE"
    IfErrors failed
    File "${NUCELVO_PAYLOAD}\THIRD_PARTY_NOTICES.md"
    IfErrors failed
    File "${NUCELVO_PAYLOAD}\THIRD_PARTY_LICENSES.txt"
    IfErrors failed
    WriteUninstaller "$INSTDIR\Uninstall.exe"
    IfErrors failed
    CreateShortcut "$SMPROGRAMS\Nucelvo.lnk" "$INSTDIR\nucelvo.exe"
    IfErrors failed

    WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayName" "Nucelvo"
    WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayVersion" "${NUCELVO_VERSION}"
    WriteRegStr HKCU "${PRODUCT_KEY}" "Publisher" "INformigados"
    WriteRegStr HKCU "${PRODUCT_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${PRODUCT_KEY}" "DisplayIcon" "$INSTDIR\nucelvo.exe,0"
    WriteRegStr HKCU "${PRODUCT_KEY}" "UninstallString" "$\"$INSTDIR\Uninstall.exe$\""
    WriteRegStr HKCU "${PRODUCT_KEY}" "QuietUninstallString" "$\"$INSTDIR\Uninstall.exe$\" /S"
    WriteRegStr HKCU "${PRODUCT_KEY}" "InstallerLanguage" $LANGUAGE
    WriteRegDWORD HKCU "${PRODUCT_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${PRODUCT_KEY}" "NoRepair" 1
    IfErrors failed
    Goto done
failed:
    MessageBox MB_ICONSTOP "$(InstallFailed)" /SD IDOK
    SetErrorLevel 1
    Abort
done:
SectionEnd

Function un.onInit
    SetRegView 64
    SetShellVarContext current
    !insertmacro MUI_UNGETLANGUAGE
FunctionEnd

Section "Uninstall"
    ClearErrors
    IfFileExists "$INSTDIR\nucelvo.exe" 0 removeNotices
    Delete "$INSTDIR\nucelvo.exe"
    IfErrors failed
removeNotices:
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
    Delete "$INSTDIR\THIRD_PARTY_LICENSES.txt"
    Delete "$INSTDIR\Uninstall.exe"

    ReadRegStr $0 HKCU "${PRODUCT_KEY}" "InstallLocation"
    ${If} $0 == $INSTDIR
        Delete "$SMPROGRAMS\Nucelvo.lnk"
        DeleteRegKey HKCU "${PRODUCT_KEY}"
    ${EndIf}
    ; Leave user reports or other files in place, and leave profile settings alone.
    RMDir "$INSTDIR"
    Goto done
failed:
    MessageBox MB_ICONSTOP "$(UninstallFailed)" /SD IDOK
    SetErrorLevel 1
    Abort
done:
SectionEnd
