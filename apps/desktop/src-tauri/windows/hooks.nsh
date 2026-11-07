; Installer hooks (bundle.windows.nsis.installerHooks): CashGear Sheet's file types.
;
; The install registers the app as a handler for its types, as Windows asks apps to: a ProgID for
; each type, the ProgID in that type's "Open with" list, and capabilities for Settings > Default
; apps. Which app opens a double-clicked file stays the user's choice. Uninstalling removes
; exactly these keys, and keys that were there before are left as they were.
;
; Tauri's own fileAssociations isn't used: it takes the type's default, and its uninstall writes an
; empty default value under HKCU\Software\Classes\.xlsx, which hides Excel's own from Explorer.
; See docs/decisions.md (2026-11-07). ProgIDs come from the bundle identifier, so the installer
; test's product never shares one with the real app.

!define CGS_CAPABILITIES "${MANUPRODUCTKEY}\Capabilities"

!macro CGS_REGISTER_TYPE EXT DESCRIPTION
  WriteRegStr SHCTX "Software\Classes\${BUNDLEID}.${EXT}" "" "${DESCRIPTION}"
  WriteRegStr SHCTX "Software\Classes\${BUNDLEID}.${EXT}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "Software\Classes\${BUNDLEID}.${EXT}\shell\open" "" "Open with ${PRODUCTNAME}"
  WriteRegStr SHCTX "Software\Classes\${BUNDLEID}.${EXT}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  WriteRegStr SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${BUNDLEID}.${EXT}" ""
  WriteRegStr SHCTX "${CGS_CAPABILITIES}\FileAssociations" ".${EXT}" "${BUNDLEID}.${EXT}"
!macroend

!macro CGS_UNREGISTER_TYPE EXT
  DeleteRegKey SHCTX "Software\Classes\${BUNDLEID}.${EXT}"
  DeleteRegValue SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${BUNDLEID}.${EXT}"
  ; Only keys left with no values and no subkeys, which is what this install made.
  DeleteRegKey /ifempty SHCTX "Software\Classes\.${EXT}\OpenWithProgids"
  DeleteRegKey /ifempty SHCTX "Software\Classes\.${EXT}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro CGS_REGISTER_TYPE "xlsx" "Excel Workbook"
  !insertmacro CGS_REGISTER_TYPE "xlsm" "Excel Macro-Enabled Workbook"
  !insertmacro CGS_REGISTER_TYPE "xltx" "Excel Template"
  !insertmacro CGS_REGISTER_TYPE "xltm" "Excel Macro-Enabled Template"
  !insertmacro CGS_REGISTER_TYPE "csv" "CSV File"
  WriteRegStr SHCTX "${CGS_CAPABILITIES}" "ApplicationName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${CGS_CAPABILITIES}" "ApplicationDescription" "A spreadsheet that opens and saves Excel workbooks."
  WriteRegStr SHCTX "Software\RegisteredApplications" "${PRODUCTNAME}" "${CGS_CAPABILITIES}"
  ; Explorer reads the new handlers at once.
  System::Call "shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro CGS_UNREGISTER_TYPE "xlsx"
  !insertmacro CGS_UNREGISTER_TYPE "xlsm"
  !insertmacro CGS_UNREGISTER_TYPE "xltx"
  !insertmacro CGS_UNREGISTER_TYPE "xltm"
  !insertmacro CGS_UNREGISTER_TYPE "csv"
  DeleteRegKey SHCTX "${CGS_CAPABILITIES}"
  DeleteRegValue SHCTX "Software\RegisteredApplications" "${PRODUCTNAME}"
  System::Call "shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)"
!macroend
