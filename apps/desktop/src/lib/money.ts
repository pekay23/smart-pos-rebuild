// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

export function parseMoneyMinor(value: string): number | null {
  const normalized = value.trim().replaceAll(",", "");
  const match = /^(\d{1,13})(?:\.(\d{0,2}))?$/.exec(normalized);
  if (!match) return null;
  const major = Number(match[1]);
  const fractional = Number((match[2] ?? "").padEnd(2, "0"));
  const minor = major * 100 + fractional;
  return Number.isSafeInteger(minor) ? minor : null;
}

export function formatMoney(minor: number, currencyCode: string): string {
  const amount = minor / 100;
  try {
    return new Intl.NumberFormat("en-GH", {
      style: "currency",
      currency: currencyCode,
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(amount);
  } catch {
    return `${currencyCode} ${amount.toFixed(2)}`;
  }
}

export function formatDateTime(value: string): string {
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime())
    ? value
    : new Intl.DateTimeFormat("en-GH", {
        dateStyle: "medium",
        timeStyle: "short",
      }).format(parsed);
}
