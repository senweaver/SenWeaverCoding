; SenWeaverCoding NSIS installer hook
;
; Registers the install directory into HKCU\Environment\Path so that the
; bundled `sen.exe` CLI becomes callable from any cmd / PowerShell / Windows
; Terminal session after install. Removes it on uninstall.
;
; Tauri's default NSIS template already includes MUI2 / LogicLib / WinMessages
; / WordFunc / StrFunc, so we can rely on ${WordFind}/${WordReplace} below.

!include "WinMessages.nsh"
!include "LogicLib.nsh"
!include "WordFunc.nsh"

!macro _SenPathAdd
  ReadRegStr $R0 HKCU "Environment" "Path"
  ${If} $R0 == ""
    WriteRegExpandStr HKCU "Environment" "Path" "$INSTDIR"
  ${Else}
    ${WordFind} "$R0" "$INSTDIR" "E+1{" $R1
    ${If} $R1 == ""
      WriteRegExpandStr HKCU "Environment" "Path" "$R0;$INSTDIR"
    ${EndIf}
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro _SenPathRemove
  ReadRegStr $R0 HKCU "Environment" "Path"
  ${If} $R0 != ""
    ${WordReplace} "$R0" ";$INSTDIR" "" "+" $R1
    ${WordReplace} "$R1" "$INSTDIR;" "" "+" $R1
    ${WordReplace} "$R1" "$INSTDIR"  "" "+" $R1
    WriteRegExpandStr HKCU "Environment" "Path" "$R1"
    SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro _SenPathAdd
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro _SenPathRemove
!macroend
