// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { invoke } from "@tauri-apps/api/core";
import type { SessionUser } from "./models";

export type PersonalPasskey = { id: string; label: string; createdAt: string; lastUsedAt: string | null };
export type WorkSchedule = { userId: string; timezone: string; startTime: string; endTime: string; weekdays: number[] };
export type WorkPeriod = {
  id: string; userId: string; fullName: string; scheduledStart: string; startedAt: string; dueAt: string;
  endedAt: string | null; endReason: "staff" | "schedule" | "safety_limit" | null; elapsedSeconds: number; lateSeconds: number;
};
export type Attendance = { serverNow: string; periods: WorkPeriod[]; schedules: WorkSchedule[] };
export const accountApi = {
  passkeysAvailable: () => invoke<boolean>("passkeys_available"),
  passkeys: () => invoke<PersonalPasskey[]>("list_passkeys"),
  registerPasskey: (label: string, password: string, personalDevice: boolean) => invoke<void>("register_passkey", { label, password, personalDevice }),
  loginPasskey: (username: string) => invoke<SessionUser>("login_passkey", { username }),
  removePasskey: (id: string, password: string) => invoke<void>("remove_passkey", { id, password }),
  attendance: () => invoke<Attendance>("get_attendance"),
  saveSchedule: (input: WorkSchedule) => invoke<void>("save_work_schedule", { input }),
  startWork: () => invoke<void>("start_work"),
  endWork: () => invoke<void>("end_work"),
};
export function workDuration(seconds: number) {
  const minutes = Math.floor(Math.max(0, seconds) / 60);
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}
