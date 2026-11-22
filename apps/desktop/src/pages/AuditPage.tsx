// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { useEffect, useRef, useState, type FormEvent } from "react";
import { Activity, Download, Search } from "lucide-react";
import { posApi } from "../lib/api";
import type { AuditEventPage, AuditEventSummary } from "../lib/models";
import { formatDateTime } from "../lib/money";

const pageSize = 50;
const csvCell = (value: string) => {
  const safe = /^[\s]*[=+@-]/.test(value) ? `'${value}` : value;
  return `"${safe.replaceAll('"', '""')}"`;
};

function downloadCsv(rows: AuditEventSummary[]) {
  const lines = [["Date and time", "Staff member", "Action", "Record type", "Record ID"], ...rows.map((event) => [event.createdAt, event.actorName ?? "Deleted user", event.action, event.entityType, event.entityId ?? ""])];
  const blob = new Blob([`\uFEFF${lines.map((line) => line.map(csvCell).join(",")).join("\r\n")}`], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "advanced-pos-activity-log.csv";
  link.click();
  URL.revokeObjectURL(url);
}

export function AuditPage() {
  const [searchInput, setSearchInput] = useState("");
  const [search, setSearch] = useState("");
  const [offset, setOffset] = useState(0);
  const [result, setResult] = useState<AuditEventPage | null>(null);
  const [loadedSearch, setLoadedSearch] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [exporting, setExporting] = useState(false);
  const snapshot = useRef<{ search: string; rowId: number } | null>(null);

  useEffect(() => {
    let active = true;
    void posApi.queryAuditEvents({
      search,
      offset,
      pageSize,
      snapshotRowId: offset > 0 && snapshot.current?.search === search ? snapshot.current.rowId : undefined,
    }).then((page) => {
      if (!active) return;
      snapshot.current = { search, rowId: page.snapshotRowId };
      setResult(page);
      setLoadedSearch(search);
    }).catch(() => { if (active) setError("Activity history could not be loaded. Try again."); });
    return () => { active = false; };
  }, [offset, search]);

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    snapshot.current = null;
    setError("");
    setOffset(0);
    setSearch(searchInput.trim().slice(0, 100));
  }

  async function exportAll() {
    if (!result?.matchedCount) return;
    setExporting(true);
    try {
      const rows: AuditEventSummary[] = [];
      for (let nextOffset = 0; nextOffset < result.matchedCount; nextOffset += 100) {
        const page = await posApi.queryAuditEvents({ search, offset: nextOffset, pageSize: 100, snapshotRowId: result.snapshotRowId });
        rows.push(...page.rows);
      }
      downloadCsv(rows);
    } catch {
      setError("The activity export could not be prepared. Try again.");
    } finally {
      setExporting(false);
    }
  }

  const rows = result?.rows ?? [];
  const hasNext = Boolean(result && result.offset + rows.length < result.matchedCount);
  return <section className="reports-page audit-page">
    <div className="page-heading"><div><p className="eyebrow">CONTROL &amp; REVIEW</p><h1>Activity log</h1><p className="page-subtitle">Review changes recorded for this store. Sensitive event details are not shown here.</p></div><button className="button button-secondary" type="button" onClick={() => void exportAll()} disabled={!result?.matchedCount || loadedSearch !== search || exporting}><Download size={17}/>{exporting ? "Preparing export…" : "Export activity CSV"}</button></div>
    <form className="panel audit-filter" onSubmit={submitSearch}><label htmlFor="audit-search">Search activity</label><div><Search size={17}/><input id="audit-search" value={searchInput} maxLength={100} onChange={(event) => setSearchInput(event.target.value)} placeholder="Staff, action, record type or ID"/><button className="button button-primary button-small" type="submit">Search</button></div><small>Search checks staff names, usernames, actions, record types and record IDs.</small></form>
    <article className="panel sales-table-panel"><div className="table-toolbar"><div><h2>Recorded activity</h2><p>{result?.matchedCount ?? "—"} event{result?.matchedCount === 1 ? "" : "s"} · newest first</p></div></div><div className="table-scroll"><table><thead><tr><th>DATE &amp; TIME</th><th>STAFF MEMBER</th><th>ACTION</th><th>RECORD TYPE</th><th>RECORD ID</th></tr></thead><tbody>{rows.map((event) => <tr key={event.id}><td>{formatDateTime(event.createdAt)}</td><td>{event.actorName ?? "Deleted user"}</td><td><span className="audit-action">{event.action.replaceAll("_", " ").replaceAll(".", " · ")}</span></td><td>{event.entityType.replaceAll("_", " ")}</td><td className="mono-value">{event.entityId ?? "—"}</td></tr>)}{!rows.length && <tr><td colSpan={5}><div className="table-empty"><span className="empty-icon"><Activity size={23}/></span><strong>{error || (result ? "No activity found" : "Loading activity…")}</strong><span>{result ? "Try a different search." : ""}</span></div></td></tr>}</tbody></table></div><div className="sales-pagination"><span>{result ? `${rows.length ? result.offset + 1 : 0}–${result.offset + rows.length} of ${result.matchedCount}` : error || "Loading activity…"}</span><div><button className="button button-secondary button-small" type="button" onClick={() => setOffset(Math.max(0, offset - pageSize))} disabled={!result || result.offset === 0}>Previous</button><button className="button button-secondary button-small" type="button" onClick={() => setOffset(offset + pageSize)} disabled={!hasNext}>Next</button></div></div></article>
    <footer className="page-footer"><span>ACTIVITY LOG <b>·</b> This store</span><span>Log entries support operational review and are not a complete security audit trail</span></footer>
  </section>;
}
