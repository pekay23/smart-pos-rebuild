// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Check, CircleAlert, Info, X } from "lucide-react";
import { ToastContext, type Notify, type ToastKind } from "./ToastContext";

type ToastMessage = { id: number; kind: ToastKind; message: string };

export function ToastProvider({ children }: { children: ReactNode }) {
  const [messages, setMessages] = useState<ToastMessage[]>([]);
  const notify = useCallback<Notify>((kind, message) => {
    const id = Date.now() + Math.random();
    setMessages((current) => [...current, { id, kind, message }].slice(-4));
  }, []);

  useEffect(() => {
    if (messages.length === 0) return;
    const timer = window.setTimeout(() => {
      setMessages((current) => current.slice(1));
    }, 4200);
    return () => window.clearTimeout(timer);
  }, [messages]);

  const dismiss = (id: number) => setMessages((current) => current.filter((message) => message.id !== id));
  return <ToastContext.Provider value={notify}>{children}<div className="toast-stack" aria-label="Notifications">{messages.map((item) => <div className={`app-toast toast-${item.kind}`} key={item.id} role={item.kind === "error" ? "alert" : "status"} aria-live={item.kind === "error" ? "assertive" : "polite"}>{item.kind === "success" ? <Check size={17}/> : item.kind === "error" ? <CircleAlert size={17}/> : <Info size={17}/>}<span>{item.message}</span><button type="button" onClick={() => dismiss(item.id)} aria-label="Dismiss notification"><X size={15}/></button></div>)}</div></ToastContext.Provider>;
}
