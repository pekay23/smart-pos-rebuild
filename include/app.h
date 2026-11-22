// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

#ifndef SMARTPOS_APP_H
#define SMARTPOS_APP_H

#include <windows.h>

typedef struct APP_GLOBALS {
    DWORD version;
    DWORD oleResult;
    HWND hwndMain;
    HWND hwndDialog;
    HINSTANCE hInstance;
    WCHAR tempPath[MAX_PATH];
    WCHAR commandLine[MAX_PATH * 2];
    WCHAR stringBuffer[0x2000];
    WCHAR errorBuffer[0x1000];
    DWORD configFlags;
    int dialogResourceId;
    BOOL isInstaller;
    BOOL isSilent;
    int exitCode;
} APP_GLOBALS;

extern APP_GLOBALS g_App;
INT_PTR CALLBACK MainDialogProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam);
INT_PTR DialogBoxMain(HINSTANCE instance, HWND parent);
void ParseCommandLine(LPCWSTR commandLine);

#endif
