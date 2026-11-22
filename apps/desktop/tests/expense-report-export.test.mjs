import assert from "node:assert/strict";
import test from "node:test";
import {
  buildExpenseReportCsv,
  loadCompleteExpenseExport,
} from "../src/lib/expenseReportExport.ts";

const fixedSnapshot = {
  snapshotRowId: 11,
  reimbursementSnapshotRowId: 12,
  reclassificationSnapshotRowId: 13,
};

function expenseRow(overrides = {}) {
  return {
    id: "expense-a",
    expenseId: "expense-a",
    entryType: "expense",
    category: "Utilities",
    description: "Power",
    amountMinor: 1000,
    paymentMethod: "cash",
    cashOrigin: "not_applicable",
    cashSourceNote: null,
    costCenterKind: "station_direct",
    costCenterStationKey: "station-a",
    costCenterStationNameSnapshot: "Station A",
    costCenterRevision: 1,
    occurredAt: "2026-10-30T10:00:00.000Z",
    createdAt: "2026-10-30T10:00:00.000Z",
    recordedBy: "Owner One",
    correctionReason: null,
    externalReference: null,
    canReverse: true,
    remainingMinor: 1000,
    ...overrides,
  };
}

function queryPage({ rows, offset, matchedCount, ...snapshots }) {
  return {
    rows,
    offset,
    pageSize: 100,
    matchedCount,
    totalMinor: rows.reduce((total, row) => total + row.amountMinor, 0),
    ...snapshots,
  };
}

test("expense export pins all three cursors across every page", async () => {
  const firstRow = expenseRow();
  const returnRow = expenseRow({
    id: "return-a",
    entryType: "reimbursement",
    amountMinor: -200,
    paymentMethod: "bank_transfer",
    cashOrigin: "not_applicable",
    correctionReason: "Part of expense returned",
    externalReference: "BANK-RETURN-1",
  });
  const calls = [];
  const query = async (input) => {
    calls.push(input);
    return input.offset === 0
      ? queryPage({ rows: [firstRow], offset: 0, matchedCount: 2, ...fixedSnapshot })
      : queryPage({ rows: [returnRow], offset: 1, matchedCount: 2, ...fixedSnapshot });
  };

  const rows = await loadCompleteExpenseExport({
    query,
    filters: { fromUtc: "2026-10-01T00:00:00Z", search: "utilities" },
  });

  assert.deepEqual(rows, [firstRow, returnRow]);
  assert.equal(calls.length, 2);
  assert.deepEqual(
    {
      snapshotRowId: calls[1].snapshotRowId,
      reimbursementSnapshotRowId: calls[1].reimbursementSnapshotRowId,
      reclassificationSnapshotRowId: calls[1].reclassificationSnapshotRowId,
    },
    fixedSnapshot,
  );
  assert.equal(calls[1].offset, 1);
  assert.equal(calls[1].search, "utilities");
});

test("expense export refuses pages with changed counts or attribution snapshots", async () => {
  let calls = 0;
  const query = async (input) => {
    calls += 1;
    return input.offset === 0
      ? queryPage({ rows: [expenseRow()], offset: 0, matchedCount: 2, ...fixedSnapshot })
      : queryPage({
          rows: [expenseRow({ id: "expense-b", expenseId: "expense-b" })],
          offset: 1,
          matchedCount: 2,
          ...fixedSnapshot,
          reclassificationSnapshotRowId: fixedSnapshot.reclassificationSnapshotRowId + 1,
        });
  };

  await assert.rejects(
    loadCompleteExpenseExport({ query, filters: {} }),
    /changed during export/,
  );
  assert.equal(calls, 2);
});

test("empty reports do not request extra pages", async () => {
  let calls = 0;
  const rows = await loadCompleteExpenseExport({
    query: async () => {
      calls += 1;
      return queryPage({ rows: [], offset: 0, matchedCount: 0, ...fixedSnapshot });
    },
    filters: {},
  });

  assert.deepEqual(rows, []);
  assert.equal(calls, 1);
});

test("CSV keeps corrected centers and signed returns while escaping spreadsheet formulas", () => {
  const csv = buildExpenseReportCsv({
    currencyCode: "GHS",
    rows: [
      expenseRow({ category: '=HYPERLINK("https://example.test")', description: 'Power, "main line"' }),
      expenseRow({
        id: "return-a",
        entryType: "reimbursement",
        amountMinor: -200,
        paymentMethod: "bank_transfer",
        costCenterKind: "store_overhead",
        costCenterStationKey: null,
        costCenterStationNameSnapshot: null,
        costCenterRevision: 2,
        correctionReason: "Part of expense returned",
        externalReference: "BANK-RETURN-1",
      }),
    ],
  });

  assert.equal(csv.split("\r\n").length, 3);
  assert.match(csv, /"'=HYPERLINK\(""https:\/\/example\.test""\)"/);
  assert.match(csv, /"Power, ""main line"""/);
  assert.match(csv, /"Station A"/);
  assert.match(csv, /"Store overhead"/);
  assert.match(csv, /"'-[^,]+"/);
  assert.match(csv, /"Part of expense returned","BANK-RETURN-1"/);
});
