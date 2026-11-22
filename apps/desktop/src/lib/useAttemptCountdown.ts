// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useState } from "react";

const lockoutMessage = /^Too many attempts\b/i;

export function useAttemptCountdown() {
  const [remainingSeconds, setRemainingSeconds] = useState(0);

  useEffect(() => {
    if (remainingSeconds <= 0) return;
    const timer = window.setTimeout(() => setRemainingSeconds((seconds) => Math.max(0, seconds - 1)), 1000);
    return () => window.clearTimeout(timer);
  }, [remainingSeconds]);

  function observe(message: string) {
    const seconds = /wait\s+(\d+)\s+seconds/i.exec(message)?.[1];
    if (seconds) setRemainingSeconds(Math.max(0, Number(seconds)));
  }

  function format(message: string) {
    if (!lockoutMessage.test(message)) return message;
    if (remainingSeconds > 0) return `Too many attempts. Try again in ${remainingSeconds} seconds.`;
    return "You can try again now.";
  }

  function reset() { setRemainingSeconds(0); }

  return { remainingSeconds, observe, format, reset };
}
