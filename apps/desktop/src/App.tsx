// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { posApi } from "./lib/api";
import type { BootstrapStatus, SessionUser } from "./lib/models";
import { AppShell } from "./components/AppShell";
import { LoginPage } from "./pages/LoginPage";
import { SetupPage } from "./pages/SetupPage";
import { CustomerDisplayPage } from "./pages/CustomerDisplayPage";
import brandMark from "./assets/nimbletill-mark.svg";
import "./styles.css";
import "./sidebar.css";
import "./cashier.css";
import "./account.css";
import "./toast.css";
import "./inventory-count.css";

type StartupState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; bootstrap: BootstrapStatus };

export default function App() {
  if (new URLSearchParams(window.location.search).get("screen") === "customer-display") {
    return <CustomerDisplayPage />;
  }
  return <MainApplication />;
}

function MainApplication() {
  const [startup, setStartup] = useState<StartupState>({ kind: "loading" });
  const [session, setSession] = useState<SessionUser | null>(null);

  useEffect(() => {
    void posApi.bootstrapStatus().then(
      (bootstrap) => setStartup({ kind: "ready", bootstrap }),
      () => setStartup({ kind: "error", message: "Advanced POS could not open its local database." }),
    );
  }, []);

  async function signOut() {
    await posApi.logout();
    setSession(null);
    try {
      setStartup({ kind: "ready", bootstrap: await posApi.bootstrapStatus() });
    } catch {
      setStartup({ kind: "error", message: "You are signed out, but Advanced POS could not refresh the local workspace status." });
    }
  }

  if (startup.kind === "loading") {
    return <div className="boot-screen"><span className="brand-mark"><img src={brandMark} alt=""/></span><p>Opening your store…</p></div>;
  }

  if (startup.kind === "error") {
    return <div className="boot-screen"><div className="auth-error">{startup.message}</div></div>;
  }

  if (session) return <AppShell key={session.userId} user={session} onSignOut={signOut} onSessionChange={setSession} />;
  if (startup.bootstrap.needsSetup) {
    return <SetupPage onComplete={setSession} />;
  }
  return (
    <LoginPage
      businessName={startup.bootstrap.businessName ?? "Advanced POS"}
      onComplete={setSession}
    />
  );
}
