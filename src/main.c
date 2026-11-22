// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

#include <windows.h>
#include <commctrl.h>
#include <shellapi.h>
#include <shlobj.h>
#include <strsafe.h>
#include <wchar.h>

#include "app.h"
#include "resource.h"

APP_GLOBALS g_App;

static BOOL IsUsableDirectory(LPCWSTR path)
{
    DWORD attrs = GetFileAttributesW(path);
    return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0;
}

static BOOL PrepareTempDirectory(void)
{
    DWORD length = GetTempPathW(MAX_PATH, g_App.tempPath);
    if (length == 0 || length >= MAX_PATH) return FALSE;
    if (IsUsableDirectory(g_App.tempPath)) return TRUE;

    length = GetWindowsDirectoryW(g_App.tempPath, MAX_PATH - 6);
    if (length == 0 || length >= MAX_PATH - 6) return FALSE;
    if (FAILED(StringCchCatW(g_App.tempPath, MAX_PATH, L"\\Temp"))) return FALSE;
    if (IsUsableDirectory(g_App.tempPath)) return TRUE;

    length = GetTempPathW(MAX_PATH - 10, g_App.tempPath);
    if (length == 0 || length >= MAX_PATH - 10) return FALSE;
    if (FAILED(StringCchCatW(g_App.tempPath, MAX_PATH, L"SmartPOS"))) return FALSE;
    if (!CreateDirectoryW(g_App.tempPath, NULL) && GetLastError() != ERROR_ALREADY_EXISTS)
        return FALSE;
    if (!IsUsableDirectory(g_App.tempPath)) return FALSE;
    SetEnvironmentVariableW(L"TEMP", g_App.tempPath);
    SetEnvironmentVariableW(L"TMP", g_App.tempPath);
    return TRUE;
}

void ParseCommandLine(LPCWSTR commandLine)
{
    int count = 0;
    LPWSTR *args = CommandLineToArgvW(commandLine, &count);
    if (args == NULL) return;
    for (int i = 1; i < count; ++i) {
        if (_wcsicmp(args[i], L"/S") == 0 || _wcsicmp(args[i], L"-S") == 0)
            g_App.isSilent = TRUE;
        if (_wcsicmp(args[i], L"/RegServer") == 0)
            g_App.configFlags |= 4;
        if ((_wcsicmp(args[i], L"--screen") == 0 || _wcsicmp(args[i], L"/screen") == 0) &&
            i + 1 < count) {
            WCHAR *end = NULL;
            unsigned long id = wcstoul(args[++i], &end, 10);
            if (end != args[i] && *end == L'\0' && id <= 0x7fff)
                g_App.dialogResourceId = (int)id;
        }
    }
    LocalFree(args);
}

int WINAPI WinMain(HINSTANCE instance, HINSTANCE previous, LPSTR commandLine, int showCommand)
{
    (void)previous;
    (void)commandLine;
    (void)showCommand;

    SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    ZeroMemory(&g_App, sizeof(g_App));
    g_App.hInstance = instance;
    g_App.version = GetVersion() & 0xBFFFFFFF;
    g_App.oleResult = (DWORD)OleInitialize(NULL);

    INITCOMMONCONTROLSEX controls = { sizeof(controls), ICC_WIN95_CLASSES | ICC_PROGRESS_CLASS };
    InitCommonControlsEx(&controls);
    LoadLibraryW(L"Msftedit.dll");
    ParseCommandLine(GetCommandLineW());

    if (!PrepareTempDirectory()) {
        MessageBoxW(NULL, L"The application could not prepare a temporary directory.",
                    L"Smart POS Server", MB_OK | MB_ICONERROR);
        g_App.exitCode = 2;
    } else {
        INT_PTR result = DialogBoxMain(instance, NULL);
        g_App.exitCode = result == -1 ? 2 : (int)result;
    }

    if (SUCCEEDED((HRESULT)g_App.oleResult)) OleUninitialize();
    return g_App.exitCode;
}
