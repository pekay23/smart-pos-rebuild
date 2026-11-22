// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { posApi } from "./api";
import type { SaleSummary, SalesQuery } from "./models";
import { formatMoney } from "./money";

type SalesExportFilter = Pick<SalesQuery, "fromUtc" | "toUtcExclusive" | "search" | "stationKey">;

function csvCell(value: string) {
  const leading = value.trimStart().charAt(0);
  const safe = ["=", "+", "-", "@"].includes(leading) ? `'${value}` : value;
  return `"${safe.replaceAll('"', '""')}"`;
}

function createCsv(rows: SaleSummary[], currencyCode: string) {
  const header = ["Receipt", "Date", "Cashier", "Register", "Items", "Payment", "Status", "Currency", "Total"];
  const data = rows.map((sale) => [
    sale.receiptNumber,
    sale.createdAt,
    sale.cashierName,
    sale.stationNameSnapshot,
    String(sale.itemCount),
    sale.paymentMethod,
    sale.status,
    currencyCode,
    formatMoney(sale.totalMinor, currencyCode),
  ]);
  return [header, ...data].map((row) => row.map(csvCell).join(",")).join("\r\n");
}

export async function downloadSalesCsv(filter: SalesExportFilter, currencyCode: string, filename: string) {
  const rows: SaleSummary[] = [];
  let offset = 0;
  let snapshotRowId: number | undefined;
  let matchedCount = 0;
  do {
    const page = await posApi.querySales({
      ...filter,
      offset,
      pageSize: 100,
      snapshotRowId,
    });
    snapshotRowId ??= page.snapshotRowId;
    matchedCount = page.matchedCount;
    rows.push(...page.rows);
    if (!page.rows.length) break;
    offset += page.rows.length;
  } while (offset < matchedCount);

  if (rows.length !== matchedCount) {
    throw new Error("The sales export changed while it was being prepared. Try again.");
  }

  const url = URL.createObjectURL(new Blob([createCsv(rows, currencyCode)], { type: "text/csv;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}
