// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState, type FormEvent } from "react";
import { ArrowRight, Eye, EyeOff, LockKeyhole, UserRound } from "lucide-react";
import { posApi } from "../lib/api";
import type { SessionUser } from "../lib/models";
import { AuthFrame } from "../components/AuthFrame";
import { getErrorMessage } from "../lib/errors";
import { accountApi } from "../lib/accountApi";
import { useAttemptCountdown } from "../lib/useAttemptCountdown";

export function LoginPage({
  businessName,
  onComplete,
}: {
  businessName: string;
  onComplete: (user: SessionUser) => void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [passkeysSupported, setPasskeysSupported] = useState(false);
  const attemptCountdown = useAttemptCountdown();
  useEffect(() => { void accountApi.passkeysAvailable().then(setPasskeysSupported).catch(() => {}); }, []);

  async function signInWithPasskey() {
    if (!username.trim()) { setError("Enter your username first, then choose your personal passkey."); return; }
    if (busy || attemptCountdown.remainingSeconds > 0) return;
    setBusy(true); setError("");
    try { onComplete(await accountApi.loginPasskey(username.trim())); }
    catch (reason: unknown) { const message = getErrorMessage(reason, "Passkey sign-in could not finish."); attemptCountdown.observe(message); setError(message); }
    finally { setBusy(false); }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || attemptCountdown.remainingSeconds > 0) return;
    setBusy(true);
    setError("");
    try {
      const user = await posApi.login({ username, password });
      onComplete(user);
    } catch (reason: unknown) {
      const message = getErrorMessage(reason, "Sign-in failed. Check your details and try again.");
      attemptCountdown.observe(message);
      setError(message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <AuthFrame
      eyebrow={businessName.toUpperCase()}
      title="Welcome back"
      description="Sign in to open your register and store workspace."
    >
      <form className="auth-form" onSubmit={(event) => void submit(event)}>
        <label className="field-label" htmlFor="username">Username</label>
        <div className="input-with-icon">
          <UserRound size={17} aria-hidden="true" />
          <input
            id="username"
            autoComplete="username"
            autoFocus
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            placeholder="Enter your username"
            required
          />
        </div>
        <label className="field-label" htmlFor="password">Password</label>
        <div className="input-with-icon">
          <LockKeyhole size={17} aria-hidden="true" />
          <input
            id="password"
            type={showPassword ? "text" : "password"}
            autoComplete="current-password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder="Enter your password"
            required
          />
          <button className="icon-button input-action" type="button" onClick={() => setShowPassword((visible) => !visible)} aria-label={showPassword ? "Hide password" : "Show password"}>
            {showPassword ? <EyeOff size={17} /> : <Eye size={17} />}
          </button>
        </div>
        {error && <p className="form-error" role="alert">{attemptCountdown.format(error)}</p>}
        <button className="button button-primary auth-submit" type="submit" disabled={busy || attemptCountdown.remainingSeconds > 0}>
          {busy ? "Signing in…" : attemptCountdown.remainingSeconds > 0 ? `Try again in ${attemptCountdown.remainingSeconds}s` : "Sign in"}<ArrowRight size={17} />
        </button>
      </form>
      {passkeysSupported && <button className="button button-secondary auth-passkey" type="button" disabled={busy || attemptCountdown.remainingSeconds > 0} onClick={() => void signInWithPasskey()}>{busy ? "Please wait…" : attemptCountdown.remainingSeconds > 0 ? `Try again in ${attemptCountdown.remainingSeconds}s` : "Sign in with passkey"}</button>}
      <p className="auth-note">Need access? Ask your store administrator to add or reset your account.</p>
      {import.meta.env.DEV && <aside className="demo-login-hint"><strong>Developer test accounts</strong><span>Owner: <code>owner / OwnerDemo2026!</code></span><span>Administrator: <code>admin / AdminDemo2026!</code></span><span>Manager: <code>manager / ManagerDemo2026!</code></span><span>Cashier: <code>cashier / CashierDemo2026!</code></span></aside>}
    </AuthFrame>
  );
}
