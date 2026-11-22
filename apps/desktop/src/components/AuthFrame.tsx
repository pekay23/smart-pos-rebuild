// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import type { ReactNode } from "react";
import { BadgeCheck } from "lucide-react";
import brandMark from "../assets/nimbletill-mark.svg";

export function AuthFrame({
  eyebrow,
  title,
  description,
  children,
}: {
  eyebrow: string;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <main className="auth-page">
      <section className="auth-story">
        <div className="auth-brand"><span className="brand-mark"><img src={brandMark} alt="" /></span><span>Advanced <span className="brand-word-accent">POS</span></span></div>
        <div className="auth-story-copy">
          <span className="eyebrow"><BadgeCheck size={15} /> READY FOR YOUR COUNTER</span>
          <h1>Every sale,<br />accounted for.</h1>
          <p>Keep checkout, stock, and the day’s numbers together—even when the internet is down.</p>
        </div>
        <div className="auth-story-foot"><span className="status-dot" /> Local data stays on this device</div>
      </section>
      <section className="auth-panel-wrap">
        <div className="auth-panel">
          <div className="auth-panel-heading">
            <span className="eyebrow">{eyebrow}</span>
            <h2>{title}</h2>
            <p>{description}</p>
          </div>
          {children}
        </div>
        <p className="auth-version">Advanced POS · Powered by Powered Solutions</p>
      </section>
    </main>
  );
}
