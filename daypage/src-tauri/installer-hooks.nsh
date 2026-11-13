!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Daypage"
  ${If} $0 != ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Daypage" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Daypage"
  ${If} $UpdateMode <> 1
  ${AndIf} $0 == '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Daypage"
  ${EndIf}
!macroend
