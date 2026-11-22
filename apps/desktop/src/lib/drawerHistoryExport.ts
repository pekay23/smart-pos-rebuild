// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { posApi } from "./api";
import type { DrawerReport } from "./models";
import { formatMoney } from "./money";

const pageSize = 100;

function csvCell(value: string) {
  const leading = value.trimStart().charAt(0);
  const safe = ["=", "+", "-", "@"].includes(leading) ? `'${value}` : value;
  return `"${safe.replaceAll('"', '""')}"`;
}

function createCsv(rows: DrawerReport[], currencyCode: string) {
  const header = ["Cashier", "Station", "Opened", "Closed", "Cash sales count", "Opening float", "Cash sales", "Paid in", "Paid out", "Cash drops", "Expected cash", "Counted cash", "Difference", "Currency"];
  const data = rows.map((row) => [
    row.cashierName,
    row.stationName,
    row.openedAt,
    row.closedAt ?? "",
    String(row.completedSaleCount),
    formatMoney(row.openingCashMinor, currencyCode),
    formatMoney(row.cashSalesMinor, currencyCode),
    formatMoney(row.paidInMinor, currencyCode),
    formatMoney(row.paidOutMinor, currencyCode),
    formatMoney(row.cashDropsMinor, currencyCode),
    formatMoney(row.expectedCashMinor, currencyCode),
    row.countedCashMinor === null ? "" : formatMoney(row.countedCashMinor, currencyCode),
    row.differenceMinor === null ? "" : formatMoney(row.differenceMinor, currencyCode),
    currencyCode,
  ]);
  return [header, ...data].map((row) => row.map(csvCell).join(",")).join("\r\n");
}

export async function downloadDrawerHistoryCsv(input: { snapshotRowId: number }, currencyCode: string) {
  const rows: DrawerReport[] = [];
  let offset = 0;
  let matchedCount = 0;
  do {
    const page = await posApi.closedDrawerHistory({ ...input, offset, pageSize });
    matchedCount = page.matchedCount;
    rows.push(...page.rows);
    if (!page.rows.length) break;
    offset += page.rows.length;
  } while (offset < matchedCount);

  if (rows.length !== matchedCount) {
    throw new Error("Drawer history changed while the export was prepared. Try again.");
  }

  const url = URL.createObjectURL(new Blob([createCsv(rows, currencyCode)], { type: "text/csv;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = `advanced-pos-drawer-history-${new Date().toISOString().slice(0, 10)}.csv`;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}
