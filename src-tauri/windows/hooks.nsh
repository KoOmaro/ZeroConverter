; Installer hooks (Tauri NSIS).

; Before the files go: take "Convert with ZeroConverter" out of the
; right-click menu (per-user registry keys the app wrote).
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --unregister-menu'
!macroend
