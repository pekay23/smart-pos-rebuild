// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import type { ExpenseQuery, ExpenseQueryResult, ExpenseReportRow } from "./models.ts";
import { formatDateTime, formatMoney } from "./money.ts";

const exportPageSize = 100;

type ExpenseQueryPage = (input: ExpenseQuery) => Promise<ExpenseQueryResult>;
type ExpenseReportFilters = Pick<ExpenseQuery, "fromUtc" | "toUtcExclusive" | "search">;

function isSnapshotNumber(value: number) {
  return Number.isSafeInteger(value) && value >= 0;
}

export async function loadCompleteExpenseExport({
  query,
  filters,
}: {
  query: ExpenseQueryPage;
  filters: ExpenseReportFilters;
}): Promise<ExpenseReportRow[]> {
  const first = await query({ ...filters, offset: 0, pageSize: exportPageSize });
  const expectedCount = first.matchedCount;
  const snapshot = {
    snapshotRowId: first.snapshotRowId,
    reimbursementSnapshotRowId: first.reimbursementSnapshotRowId,
    reclassificationSnapshotRowId: first.reclassificationSnapshotRowId,
  };
  if (
    !Number.isSafeInteger(expectedCount) || expectedCount < 0 ||
    !isSnapshotNumber(snapshot.snapshotRowId) ||
    !isSnapshotNumber(snapshot.reimbursementSnapshotRowId) ||
    !isSnapshotNumber(snapshot.reclassificationSnapshotRowId) ||
    first.offset !== 0 || first.rows.length > exportPageSize || first.rows.length > expectedCount
  ) {
    throw new Error("The expense report snapshot is invalid.");
  }

  const rows = [...first.rows];
  while (rows.length < expectedCount) {
    const offset = rows.length;
    const page = await query({ ...filters, ...snapshot, offset, pageSize: exportPageSize });
    if (
      page.matchedCount !== expectedCount ||
      page.snapshotRowId !== snapshot.snapshotRowId ||
      page.reimbursementSnapshotRowId !== snapshot.reimbursementSnapshotRowId ||
      page.reclassificationSnapshotRowId !== snapshot.reclassificationSnapshotRowId ||
      page.offset !== offset || page.rows.length === 0 ||
      page.rows.length > exportPageSize || rows.length + page.rows.length > expectedCount
    ) {
      throw new Error("The expense report changed during export.");
    }
    rows.push(...page.rows);
  }
  if (rows.length !== expectedCount) {
    throw new Error("The complete matching expense report could not be exported.");
  }
  return rows;
}

function csvCell(value: string | number) {
  const text = String(value);
  const safe = /^[\s]*[=+\-@]/.test(text) ? `'${text}` : text;
  return `"${safe.replaceAll('"', '""')}"`;
}

const paymentMethodLabels: Readonly<Record<string, string>> = {
  cash: "Cash",
  card: "Card",
  mobile_money: "Mobile money",
  bank_transfer: "Bank transfer",
  other: "Other",
};

export function paymentMethodLabel(method: string) {
  return paymentMethodLabels[method] ?? method;
}

export function costCenterLabel(
  kind: ExpenseReportRow["costCenterKind"],
  stationName?: string | null,
) {
  switch (kind) {
    case "station_direct":
      return stationName ?? "Station";
    case "store_overhead":
      return "Store overhead";
    case "legacy_unassigned":
      return "Not allocated to a station";
    default: {
      const unreachable: never = kind;
      return unreachable;
    }
  }
}

function cashSourceLabel(expense: ExpenseReportRow) {
  if (expense.cashOrigin === "drawer") return "Register drawer";
  if (expense.cashOrigin === "other_cash") return `Other cash: ${expense.cashSourceNote ?? "unspecified"}`;
  if (expense.cashOrigin === "legacy_unknown") return "Legacy source unknown";
  return "Not applicable";
}

export function buildExpenseReportCsv({
  rows,
  currencyCode,
}: {
  rows: ExpenseReportRow[];
  currencyCode: string;
}) {
  const header = [
    "Entry", "Date", "Category", "Description", "Amount", "Currency", "Payment method",
    "Cash source", "Cost center", "Correction reason", "Return reference", "Recorded by",
  ].map(csvCell).join(",");
  const body = rows.map((expense) => [
    expense.entryType,
    formatDateTime(expense.occurredAt),
    expense.category,
    expense.description ?? "",
    formatMoney(expense.amountMinor, currencyCode),
    currencyCode,
    paymentMethodLabel(expense.paymentMethod),
    cashSourceLabel(expense),
    costCenterLabel(
      expense.costCenterKind,
      expense.costCenterStationNameSnapshot ?? expense.costCenterStationKey,
    ),
    expense.correctionReason ?? "",
    expense.externalReference ?? "",
    expense.recordedBy,
  ].map(csvCell).join(","));

  return [header, ...body].join("\r\n");
}
