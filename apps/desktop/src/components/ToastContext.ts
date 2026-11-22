// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { createContext } from "react";

export type ToastKind = "success" | "error" | "info";
export type Notify = (kind: ToastKind, message: string) => void;
export const ToastContext = createContext<Notify | null>(null);
