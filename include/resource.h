// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

//{{NO_DEPENDENCIES}}
// Microsoft Visual C++ generated include file.
// Used by smartpos.rc
#ifndef SMARTPOS_RESOURCE_H
#define SMARTPOS_RESOURCE_H

// Main Icon
#define IDI_MAIN_ICON                   103

// Dialogs
#define IDD_MAIN_DIALOG                 105
#define IDD_DIALOG_105                  105
#define IDD_DIALOG_106                  106
#define IDD_DIALOG_111                  111
#define IDD_DIALOG_205                  205
#define IDD_DIALOG_206                  206
#define IDD_DIALOG_211                  211
#define IDD_DIALOG_305                  305
#define IDD_DIALOG_306                  306
#define IDD_DIALOG_311                  311
#define IDD_DIALOG_405                  405
#define IDD_DIALOG_406                  406
#define IDD_DIALOG_411                  411
#define IDD_DIALOG_505                  505
#define IDD_DIALOG_506                  506
#define IDD_DIALOG_511                  511
#define IDD_DIALOG_605                  605
#define IDD_DIALOG_606                  606
#define IDD_DIALOG_611                  611

// String Table
#define IDS_APP_TITLE                   1
#define IDS_APP_DESCRIPTION             2
#define IDS_COPYRIGHT                   3

// Version Info
#define VS_VERSION_INFO                 1

// Menu/Control IDs (from dialog analysis)
#define IDC_STATIC                      -1
#define IDC_EDIT_PATH                   1001
#define IDC_BUTTON_BROWSE               1002
#define IDC_LIST_FILES                  1003
#define IDC_PROGRESS                    1004
#define IDC_STATUS_TEXT                 1005
#define IDC_COMBO_PORT                  1006
#define IDC_BUTTON_START                1007
#define IDC_BUTTON_STOP                 1008
#define IDC_CHECK_AUTO                  1009
#define IDC_EDIT_LOG                    1010

// Custom Messages (WM_APP + offset)
#define WM_USER_BASE                    (WM_APP + 100)
#define WM_COPY_FILE                    (WM_USER_BASE + 0x02)
#define WM_SET_ATTR                     (WM_USER_BASE + 0x0A)
#define WM_GET_DIR                      (WM_USER_BASE + 0x0B)
#define WM_MOVE_FILE                    (WM_USER_BASE + 0x10)
#define WM_GET_FULLPATH                 (WM_USER_BASE + 0x11)
#define WM_SEARCH_PATH                  (WM_USER_BASE + 0x12)
#define WM_COPY_PATH                    (WM_USER_BASE + 0x13)
#define WM_DRIVE_INFO                   (WM_USER_BASE + 0x14)
#define WM_FILE_COUNT                   (WM_USER_BASE + 0x15)
#define WM_FILE_OP                      (WM_USER_BASE + 0x16)
#define WM_SET_TEXT                     (WM_USER_BASE + 0x17)
#define WM_GET_LENGTH                   (WM_USER_BASE + 0x18)
#define WM_CONCAT                       (WM_USER_BASE + 0x19)
#define WM_COMPARE                      (WM_USER_BASE + 0x1A)
#define WM_EXPAND_ENV                   (WM_USER_BASE + 0x1B)
#define WM_NUM_COMPARE                  (WM_USER_BASE + 0x1C)
#define WM_ARITHMETIC                   (WM_USER_BASE + 0x1D)
#define WM_FORMAT_NUM                   (WM_USER_BASE + 0x1E)
#define WM_LIST_OPS                     (WM_USER_BASE + 0x1F)
#define WM_PATH_OPS                     (WM_USER_BASE + 0x20)
#define WM_SHELL_EXEC                   (WM_USER_BASE + 0x28)
#define WM_PROCESS_WAIT                 (WM_USER_BASE + 0x29)
#define WM_HANDLE_OPS                   (WM_USER_BASE + 0x2A)
#define WM_CLIPBOARD_OPS                (WM_USER_BASE + 0x2B)
#define WM_REG_DELETE                   (WM_USER_BASE + 0x32)
#define WM_REG_WRITE                    (WM_USER_BASE + 0x33)
#define WM_REG_READ                     (WM_USER_BASE + 0x34)
#define WM_REG_ENUM                     (WM_USER_BASE + 0x35)
#define WM_FIND_HANDLE                  (WM_USER_BASE + 0x36)
#define WM_FIND_CLOSE                   (WM_USER_BASE + 0x37)
#define WM_FIND_NEXT                    (WM_USER_BASE + 0x3C)
#define WM_FIND_FIRST                   (WM_USER_BASE + 0x3D)
#define WM_FILE_READ                    (WM_USER_BASE + 0x39)
#define WM_FILE_WRITE                   (WM_USER_BASE + 0x3A)
#define WM_FILE_SEEK                    (WM_USER_BASE + 0x3B)
#define WM_INI_WRITE                    (WM_USER_BASE + 0x30)
#define WM_INI_READ                     (WM_USER_BASE + 0x31)
#define WM_DLL_LOAD                     (WM_USER_BASE + 0x2C)
#define WM_COM_CREATE                   (WM_USER_BASE + 0x2D)
#define WM_WIN_MSG                      (WM_USER_BASE + 0x20)
#define WM_WIN_FIND                     (WM_USER_BASE + 0x21)
#define WM_CTRL_OPS                     (WM_USER_BASE + 0x23)
#define WM_SET_IMAGE                    (WM_USER_BASE + 0x25)
#define WM_FONT_CREATE                  (WM_USER_BASE + 0x26)
#define WM_SHOW_ENABLE                  (WM_USER_BASE + 0x27)

// Global Data Offsets (from decompilation)
#define DAT_CONFIG_BASE                 0x00472E80
#define DAT_STRING_BUFFER               0x0047B000
#define DAT_TEMP_BUFFER                 0x004D3000
#define DAT_ERROR_BUFFER                0x004CF000
#define DAT_COMMAND_LINE                0x004CB000
#define DAT_TEMP_PATH                   0x004DF000
#define DAT_INSTALL_PATH                0x004D7000

// Window class
#define WC_SMARTPOS_MAIN                L"SmartPOSMainClass"
#define WC_SMARTPOS_DIALOG              L"SmartPOSDialogClass"

// Registry paths
#define REG_KEY_SMARTPOS                L"Software\\AmaduTech\\SmartPOS"
#define REG_KEY_DESKTOP                 L"Control Panel\\Desktop\\ResourceLocale"

#endif // RESOURCE_H
