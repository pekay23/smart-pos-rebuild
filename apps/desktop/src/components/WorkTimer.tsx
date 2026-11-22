// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";
import { Clock3 } from "lucide-react";
import { accountApi, workDuration, type WorkPeriod } from "../lib/accountApi";

export function WorkTimer({ userId, temporaryCashierMode, onClick }: { userId: string; temporaryCashierMode: boolean; onClick: () => void }) {
  const [period, setPeriod] = useState<WorkPeriod | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    let active = true;
    const refresh = () => void accountApi.attendance().then((data) => {
      if (!active) return;
      setPeriod(data.periods.find((item) => item.userId === userId) ?? null);
      setError(false);
    }).catch(() => { if (active) setError(true); });
    refresh();
    const timer = window.setInterval(refresh, 15_000);
    return () => { active = false; window.clearInterval(timer); };
  }, [userId, temporaryCashierMode]);
  return <button className="work-timer" onClick={onClick} title={error ? "Work record unavailable. Open My account to retry." : "Open your work record and account"}>
    <Clock3 size={15}/><span>{error ? "Check work record" : period ? `${workDuration(period.elapsedSeconds)}${period.endedAt ? " · ended" : " worked"}` : "Work not started"}</span>
  </button>;
}
