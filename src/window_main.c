// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

#include <windows.h>

#include "app.h"
#include "resource.h"

INT_PTR DialogBoxMain(HINSTANCE instance, HWND parent)
{
    static const int dialogIds[] = {
        105, 106, 111, 205, 206, 211, 305, 306, 311,
        405, 406, 411, 505, 506, 511, 605, 606, 611
    };
    int requestedId = g_App.dialogResourceId ? g_App.dialogResourceId : IDD_MAIN_DIALOG;
    BOOL knownDialog = FALSE;
    for (size_t i = 0; i < ARRAYSIZE(dialogIds); ++i) {
        if (dialogIds[i] == requestedId) {
            knownDialog = TRUE;
            break;
        }
    }
    if (!knownDialog) return -1;

    HRSRC resource = FindResourceW(instance, MAKEINTRESOURCEW(requestedId), RT_RCDATA);
    if (resource == NULL) return -1;
    HGLOBAL loaded = LoadResource(instance, resource);
    if (loaded == NULL) return -1;
    const DLGTEMPLATE *dialog = (const DLGTEMPLATE *)LockResource(loaded);
    if (dialog == NULL) return -1;
    g_App.hwndDialog = parent;
    return DialogBoxIndirectParamW(instance, dialog, parent, MainDialogProc, 0);
}

INT_PTR CALLBACK MainDialogProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    (void)lParam;
    switch (msg) {
    case WM_INITDIALOG:
        g_App.hwndDialog = hwnd;
        SendMessageW(hwnd, WM_SETICON, ICON_SMALL,
                     (LPARAM)LoadIconW(g_App.hInstance, MAKEINTRESOURCEW(IDI_MAIN_ICON)));
        SendMessageW(hwnd, WM_SETICON, ICON_BIG,
                     (LPARAM)LoadIconW(g_App.hInstance, MAKEINTRESOURCEW(IDI_MAIN_ICON)));
        return TRUE;
    case WM_COMMAND:
        switch (LOWORD(wParam)) {
        case IDOK:
        case IDCANCEL:
            EndDialog(hwnd, LOWORD(wParam) == IDOK ? 0 : 1);
            return TRUE;
        }
        break;
    case WM_CLOSE:
        EndDialog(hwnd, 1);
        return TRUE;
    }
    return FALSE;
}
