// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import type { ProfitReport } from "./models";
import { formatMoney } from "./money";

type ProfitMetric = {
  name: string;
  minorAmount: number | null;
  count: number | null;
  note: string;
};

function csvCell(value: string) {
  const leading = value.trimStart().charAt(0);
  const safe = ["=", "+", "-", "@"].includes(leading) ? `'${value}` : value;
  return `"${safe.replaceAll('"', '""')}"`;
}

export function createProfitLossCsv(report: ProfitReport, from: string, to: string, currencyCode: string) {
  const costNote = report.uncostedUnits
    ? `${report.uncostedUnits} sold or returned item units lack a saved cost; profit values are withheld.`
    : "All included item costs are available.";
  const metrics: ProfitMetric[] = [
    { name: "Sales recorded", minorAmount: null, count: report.saleCount, note: "Sales and refunds use their recorded dates." },
    { name: "Net revenue before tax", minorAmount: report.netRevenueBeforeTaxMinor, count: null, note: "After recorded refunds." },
    { name: "Tax collected", minorAmount: report.taxCollectedMinor, count: null, note: "Net of recorded refunds." },
    { name: "Customer total", minorAmount: report.customerTotalMinor, count: null, note: "Net sales including tax." },
    { name: "Cost of goods sold", minorAmount: report.uncostedUnits === 0 ? report.costOfGoodsMinor : null, count: null, note: costNote },
    { name: "Costed item units", minorAmount: null, count: report.costedUnits, note: "Units with saved sale-time cost." },
    { name: "Uncosted item units", minorAmount: null, count: report.uncostedUnits, note: "Units without saved sale-time cost." },
    { name: "Gross profit", minorAmount: report.grossProfitMinor, count: null, note: report.grossProfitMinor === null ? costNote : "Net revenue before tax less cost of goods sold." },
    { name: "Direct station expenses", minorAmount: report.directExpensesMinor, count: null, note: "Original expense cost center; reimbursements reduce it on their recorded return dates." },
    { name: "Store overhead", minorAmount: report.storeOverheadExpensesMinor, count: null, note: "Shared store costs are excluded from station contribution." },
    { name: "Legacy unassigned expenses", minorAmount: report.legacyUnassignedExpensesMinor, count: null, note: "Historical costs kept unassigned rather than inferred." },
    { name: "Operating expenses in this result", minorAmount: report.operatingExpensesMinor, count: null, note: report.stationKey ? "Station-direct costs only; excludes store overhead and unassigned expenses." : "All cost centers for the selected store and period." },
    { name: report.stationKey ? "Station contribution before overhead" : "Net operating result", minorAmount: report.stationKey ? report.stationContributionMinor : report.netOperatingProfitMinor, count: null, note: report.stationKey ? (report.stationContributionMinor === null ? costNote : "Station gross profit less station-direct expenses; excludes overhead.") : (report.netOperatingProfitMinor === null ? costNote : "Gross profit less recorded operating expenses.") },
    { name: "Legacy unassigned sales", minorAmount: report.unassignedSalesRevenueMinor, count: report.unassignedSalesCount, note: "Visible as an unassigned bucket; never inferred into a station." },
    { name: "Report scope", minorAmount: null, count: null, note: report.stationKey ? `Station contribution for ${report.stationKey}; management summary before shared overhead, not a fully loaded P&L.` : "Whole-store management summary; not a complete accounting ledger or tax filing." },
  ];

  const header = ["Report", "From", "To", "Metric", "Amount in minor units", "Formatted amount", "Currency", "Count", "Notes"];
  const rows = metrics.map((metric) => [
    report.stationKey ? `Station contribution: ${report.stationKey}` : "Profit and loss summary",
    from,
    to,
    metric.name,
    metric.minorAmount === null ? "" : String(metric.minorAmount),
    metric.minorAmount === null ? (metric.name === "Cost of goods sold" && report.uncostedUnits > 0 ? "Incomplete" : "") : formatMoney(metric.minorAmount, currencyCode),
    metric.minorAmount === null && metric.count !== null ? "" : currencyCode,
    metric.count === null ? "" : String(metric.count),
    metric.note,
  ]);
  return [header, ...rows].map((row) => row.map(csvCell).join(",")).join("\r\n");
}

export function downloadProfitLossCsv(report: ProfitReport, from: string, to: string, currencyCode: string) {
  const url = URL.createObjectURL(new Blob([createProfitLossCsv(report, from, to, currencyCode)], { type: "text/csv;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = `advanced-pos-${report.stationKey ? "station-contribution" : "profit-and-loss"}-${from}-to-${to}.csv`;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}
