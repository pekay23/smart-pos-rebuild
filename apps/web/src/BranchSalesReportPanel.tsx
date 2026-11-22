// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

import { Fragment, useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { Download, Printer, RefreshCw, X } from 'lucide-react'
import { getHostedBranchBalanceSheet, getHostedBranchCashFlowStatement, getHostedBranchIncomeStatement, getHostedBranchTrialBalance, getHostedBranchReceipt, getHostedBranchSalesReport } from './api'
import type { HostedAccountingCoverageIssueCode, HostedBalanceSheet, HostedBranchAccountingCoverage, HostedCashFlowStatement, HostedIncomeStatement, HostedReceipt, HostedSalesReport, HostedTrialBalance } from './api'

function branchDateOffset(timeZone: string, daysAgo: number): string {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).formatToParts(new Date())
  const part = (type: Intl.DateTimeFormatPartTypes) => parts.find((entry) => entry.type === type)?.value ?? ''
  const date = new Date(Date.UTC(Number(part('year')), Number(part('month')) - 1, Number(part('day'))))
  date.setUTCDate(date.getUTCDate() - daysAgo)
  return date.toISOString().slice(0, 10)
}

function exclusiveEndDate(inclusiveDate: string): string | undefined {
  if (!inclusiveDate) return undefined
  const date = new Date(`${inclusiveDate}T00:00:00.000Z`)
  if (Number.isNaN(date.getTime())) return undefined
  date.setUTCDate(date.getUTCDate() + 1)
  return date.toISOString().slice(0, 10)
}

function csvCell(value: string | number): string {
  const raw = String(value)
  const text = /^[=+@\-\t\r]/.test(raw) ? `'${raw}` : raw
  return `"${text.replaceAll('"', '""')}"`
}

function formatMoney(currency: string, amountMinor: number | string): string {
  const amount = BigInt(amountMinor)
  const whole = (amount / 100n).toLocaleString()
  const fraction = String(amount % 100n).padStart(2, '0')
  return `${currency} ${whole}.${fraction}`
}

function formatBranchTimestamp(value: string, timeZone: string): string {
  return new Date(value).toLocaleString(undefined, { timeZone })
}

function accountingIssueLabel(issueCode: HostedAccountingCoverageIssueCode): string {
  switch (issueCode) {
    case 'sale_lines_do_not_reconcile': return 'Sale line totals do not match the recorded total'
    case 'sale_tenders_missing': return 'The sale has no recorded tenders'
    case 'sale_tenders_do_not_reconcile': return 'Sale tenders do not match the recorded total'
    case 'sale_journal_missing': return 'The sale has no accounting journal'
    case 'refund_lines_do_not_reconcile': return 'Refund lines do not match the recorded refund total'
    case 'refund_tenders_missing': return 'The refund has no recorded tenders'
    case 'refund_tenders_do_not_reconcile': return 'Refund tenders do not match the recorded total'
    case 'refund_journal_missing': return 'The refund has no accounting journal'
    case 'historical_cost_or_tax_classification_unavailable': return 'Historical cost or tax classification is incomplete'
    default: {
      const exhaustive: never = issueCode
      return exhaustive
    }
  }
}

export function BranchSalesReportPanel({
  apiBaseUrl, accessToken, tenantId, branchId, branchName, timeZone, canReviewAccounting,
}: {
  apiBaseUrl: string
  accessToken: string
  tenantId: string
  branchId: string
  branchName: string
  timeZone: string
  canReviewAccounting: boolean
}) {
  const [fromDate, setFromDate] = useState(() => branchDateOffset(timeZone, 6))
  const [toDate, setToDate] = useState(() => branchDateOffset(timeZone, 0))
  const [appliedFromDate, setAppliedFromDate] = useState(() => branchDateOffset(timeZone, 6))
  const [appliedToDate, setAppliedToDate] = useState(() => branchDateOffset(timeZone, 0))
  const [offset, setOffset] = useState(0)
  const [report, setReport] = useState<HostedSalesReport | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [receipt, setReceipt] = useState<HostedReceipt | null>(null)
  const [receiptLoadingId, setReceiptLoadingId] = useState<string | null>(null)
  const [receiptError, setReceiptError] = useState<string | null>(null)
  const [coverage, setCoverage] = useState<HostedBranchAccountingCoverage | null>(null)
  const [trialBalance, setTrialBalance] = useState<HostedTrialBalance | null>(null)
  const [incomeStatement, setIncomeStatement] = useState<HostedIncomeStatement | null>(null)
  const [balanceSheet, setBalanceSheet] = useState<HostedBalanceSheet | null>(null)
  const [cashFlowStatement, setCashFlowStatement] = useState<HostedCashFlowStatement | null>(null)
  const [coverageLoading, setCoverageLoading] = useState(false)
  const [coverageError, setCoverageError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    setLoading(true)
    setError(null)
    setReport(null)
    void getHostedBranchSalesReport({
      apiBaseUrl,
      accessToken,
      tenantId,
      branchId,
      fromDate: appliedFromDate || undefined,
      toDate: exclusiveEndDate(appliedToDate),
      offset,
      pageSize: 25,
    }).then((data) => {
      if (active) setReport(data)
    }).catch((reason: unknown) => {
      if (active) setError(reason instanceof Error ? reason.message : 'The sales report could not be loaded.')
    }).finally(() => {
      if (active) setLoading(false)
    })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, appliedFromDate, appliedToDate, branchId, offset, tenantId])

  useEffect(() => {
    if (!canReviewAccounting) {
      setCoverage(null)
      setTrialBalance(null)
      setIncomeStatement(null)
      setBalanceSheet(null)
      setCashFlowStatement(null)
      setCoverageError(null)
      setCoverageLoading(false)
      return
    }
    let active = true
    setCoverageLoading(true)
    setCoverageError(null)
    const reportInput = {
      apiBaseUrl, accessToken, tenantId, branchId,
      fromDate: appliedFromDate || undefined,
      toDate: exclusiveEndDate(appliedToDate),
    }
    const cashFlowToDate = exclusiveEndDate(appliedToDate)
    const cashFlowRequest = appliedFromDate && cashFlowToDate
      ? getHostedBranchCashFlowStatement({ apiBaseUrl, accessToken, tenantId, branchId, fromDate: appliedFromDate, toDate: cashFlowToDate })
      : Promise.resolve(null)
    void Promise.all([
      getHostedBranchTrialBalance(reportInput),
      getHostedBranchIncomeStatement(reportInput),
      getHostedBranchBalanceSheet({ apiBaseUrl, accessToken, tenantId, branchId, asOfDate: appliedToDate || branchDateOffset(timeZone, 0) }),
      cashFlowRequest,
    ]).then(([data, statement, balance, cashFlow]) => {
      if (active) {
        setTrialBalance(data)
        setIncomeStatement(statement)
        setBalanceSheet(balance)
        setCashFlowStatement(cashFlow)
        setCoverage(data.coverage)
      }
    }).catch((reason: unknown) => {
      if (active) setCoverageError(reason instanceof Error ? reason.message : 'Accounting coverage could not be loaded.')
    }).finally(() => {
      if (active) setCoverageLoading(false)
    })
    return () => { active = false }
  }, [accessToken, apiBaseUrl, appliedFromDate, appliedToDate, branchId, canReviewAccounting, tenantId, timeZone])

  function applyRange(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setOffset(0)
    setAppliedFromDate(fromDate)
    setAppliedToDate(toDate)
  }

  function downloadPage() {
    if (!report) return
    const header = ['Receipt', 'Date UTC', 'Cashier', 'Station', 'Payment methods', 'Currency', 'Subtotal minor', 'Tax minor', 'Total minor']
    const rows = report.rows.map((sale) => [
      sale.receiptNumber,
      sale.createdAt,
      sale.cashierName,
      sale.stationName,
      sale.paymentMethods,
      sale.currencyCode,
      sale.subtotalMinor,
      sale.taxMinor,
      sale.totalMinor,
    ])
    const csv = [header, ...rows].map((row) => row.map(csvCell).join(',')).join('\r\n')
    const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }))
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `advanced-pos-sales-${branchId}-${appliedFromDate || 'all'}-${appliedToDate || 'all'}.csv`
    anchor.click()
    URL.revokeObjectURL(url)
  }

  async function openReceipt(saleId: string) {
    setReceiptLoadingId(saleId)
    setReceiptError(null)
    try {
      setReceipt(await getHostedBranchReceipt({ apiBaseUrl, accessToken, tenantId, branchId, saleId }))
    } catch (reason) {
      setReceiptError(reason instanceof Error ? reason.message : 'Receipt details could not be loaded.')
    } finally { setReceiptLoadingId(null) }
  }

  const pageEnd = Math.min(offset + (report?.rows.length ?? 0), report?.matchedCount ?? 0)
  return <section className="branch-sales-report" aria-labelledby="branch-sales-report-title">
    <header className="branch-sales-report-header">
      <div><p className="eyebrow">BRANCH REPORTS</p><h2 id="branch-sales-report-title">Sales history</h2><p>{branchName} · {timeZone} business dates</p></div>
      <button type="button" className="button button-quiet" onClick={downloadPage} disabled={!report || report.rows.length === 0}><Download size={15}/> Export this page</button>
    </header>
    <form className="sales-report-filters" onSubmit={applyRange}>
      <label>From date<input type="date" value={fromDate} onChange={(event) => setFromDate(event.currentTarget.value)}/></label>
      <label>Through date<input type="date" value={toDate} onChange={(event) => setToDate(event.currentTarget.value)}/></label>
      <button type="submit" className="button button-primary" disabled={loading}><RefreshCw size={14}/> Apply</button>
    </form>
    {canReviewAccounting && <section className="accounting-coverage" aria-labelledby="accounting-coverage-title">
      <header><div><p className="eyebrow">ACCOUNTING REVIEW</p><h3 id="accounting-coverage-title">Posting coverage</h3></div>{coverageLoading && <span role="status">Updating…</span>}</header>
      {coverageError && <div className="notice notice-danger" role="alert">{coverageError}</div>}
      {coverage && <>
        {incomeStatement && <div className="sales-report-table-wrap">
          <h4>Income statement by currency</h4>
          {!incomeStatement.salesRefundCoverageComplete && <p className="notice notice-warning">This statement uses posted journal activity. Sales/refund coverage is incomplete or includes partial historical journals; review the exceptions below before relying on the result.</p>}
          <table className="sales-report-table">
            <thead><tr><th>Line</th>{incomeStatement.currencies.map((item) => <th key={item.currencyCode}>{item.currencyCode}</th>)}</tr></thead>
            <tbody>
              {([
                ['Gross sales', 'grossSalesMinor'], ['Sales returns', 'salesReturnsMinor'], ['Net sales', 'netSalesMinor'],
                ['Cost of goods sold', 'costOfGoodsSoldMinor'], ['Gross profit', 'grossProfitMinor'],
                ['Operating expenses', 'operatingExpensesMinor'], ['Operating income', 'operatingIncomeMinor'],
              ] as const).map(([label, field]) => <tr key={field}>
                <td><strong>{label}</strong></td>
                {incomeStatement.currencies.map((item) => <td key={item.currencyCode}>{formatMoney(item.currencyCode, item[field])}</td>)}
              </tr>)}
            </tbody>
          </table>
          <p className="sales-report-disclosure">Derived from posted journal balances for the selected dates. Opening balances, financing/income-tax adjustments, unposted source records and external settlement reconciliation are not included.</p>
        </div>}
        {trialBalance && <div className="sales-report-table-wrap">
          <h4>Trial balance by currency</h4>
          {!trialBalance.salesRefundCoverageComplete && <p className="notice notice-warning">This balance includes posted journals only. Sales/refund posting coverage is incomplete; review the exceptions below before relying on these totals.</p>}
          {trialBalance.currencies.map((total) => <p key={total.currencyCode} className={total.isBalanced ? 'sales-report-disclosure' : 'notice notice-danger'}>
            {total.currencyCode}: debits {formatMoney(total.currencyCode, total.totalDebitsMinor)} · credits {formatMoney(total.currencyCode, total.totalCreditsMinor)} · {total.isBalanced ? 'balanced' : 'out of balance'}
          </p>)}
          <table className="sales-report-table">
            <thead><tr><th>Account</th><th>Currency</th><th>Debits</th><th>Credits</th><th>Net debit / (credit)</th><th>Journals</th></tr></thead>
            <tbody>{trialBalance.rows.map((row) => <tr key={`${row.currencyCode}:${row.accountCode}`}>
              <td><strong>{row.accountCode} · {row.accountName}</strong><small>{row.accountType}</small></td>
              <td>{row.currencyCode}</td><td>{formatMoney(row.currencyCode, row.debitMinor)}</td><td>{formatMoney(row.currencyCode, row.creditMinor)}</td><td>{formatMoney(row.currencyCode, row.balanceMinor)}</td><td>{row.journalCount}</td>
            </tr>)}</tbody>
          </table>
          <p className="sales-report-disclosure">Amounts come from posted journals in the selected branch-local date range. Opening balances and unreconciled external bank/card settlements are not included.</p>
        </div>}
        {balanceSheet && <div className="sales-report-table-wrap">
          <h4>Balance sheet · as of {balanceSheet.asOfDate}</h4>
          {!balanceSheet.complete && <p className="notice notice-warning">Provisional journal view. Opening balances, external settlement balances and complete source coverage are not established; review accounting coverage before relying on these totals.</p>}
          {balanceSheet.currencies.map((item) => <Fragment key={item.currencyCode}>
            <p className={item.isBalanced ? 'sales-report-disclosure' : 'notice notice-danger'}>
              {item.currencyCode}: assets {formatMoney(item.currencyCode, item.assetsMinor)} · liabilities and equity {formatMoney(item.currencyCode, item.liabilitiesAndEquityMinor)} · {item.isBalanced ? 'balanced' : `difference ${formatMoney(item.currencyCode, item.differenceMinor)}`}
            </p>
            <table className="sales-report-table">
              <thead><tr><th>Account</th><th>Class</th><th>Balance</th></tr></thead>
              <tbody>
                {item.accounts.map((account) => <tr key={`${item.currencyCode}:${account.accountCode}`}>
                  <td><strong>{account.accountCode} · {account.accountName}</strong></td><td>{account.accountType}</td><td>{formatMoney(item.currencyCode, account.balanceMinor)}</td>
                </tr>)}
                <tr><td><strong>Retained earnings · posted activity</strong></td><td>equity</td><td>{formatMoney(item.currencyCode, item.retainedEarningsMinor)}</td></tr>
                <tr><td><strong>Total liabilities and equity</strong></td><td>total</td><td>{formatMoney(item.currencyCode, item.liabilitiesAndEquityMinor)}</td></tr>
              </tbody>
            </table>
          </Fragment>)}
          <p className="sales-report-disclosure">Amounts include posted journals through the selected branch-local date. Retained earnings are derived from posted revenue and expense accounts because period-close closing entries are not yet supported. This is not an audited or tax-certified statement.</p>
        </div>}
        {cashFlowStatement && <div className="sales-report-table-wrap">
          <h4>Cash movement statement · account 1000 · {cashFlowStatement.fromDate} to {cashFlowStatement.toDate} (exclusive)</h4>
          <p className="notice notice-warning">Provisional direct-method view. It includes posted physical cash only. Bank settlement, investing/financing activity, and full source coverage are not established.</p>
          {cashFlowStatement.currencies.map((item) => <Fragment key={item.currencyCode}>
            <p className={item.isReconciled ? 'sales-report-disclosure' : 'notice notice-danger'}>
              {item.currencyCode}: opening cash {formatMoney(item.currencyCode, item.openingCashMinor)} · net change {formatMoney(item.currencyCode, item.netChangeMinor)} · ending cash {formatMoney(item.currencyCode, item.endingCashMinor)} · {item.isReconciled ? 'ledger bridge reconciles' : `bridge difference ${formatMoney(item.currencyCode, item.reconciliationDifferenceMinor)}`}
            </p>
            <table className="sales-report-table">
              <thead><tr><th>Cash activity</th><th>Amount</th></tr></thead>
              <tbody>
                <tr><td>Operating cash received</td><td>{formatMoney(item.currencyCode, item.operatingInflowsMinor)}</td></tr>
                <tr><td>Operating cash paid</td><td>{formatMoney(item.currencyCode, item.operatingOutflowsMinor)}</td></tr>
                <tr><td>Net operating cash activity</td><td>{formatMoney(item.currencyCode, item.netOperatingCashFlowMinor)}</td></tr>
                <tr><td>Opening cash cutover adjustments</td><td>{formatMoney(item.currencyCode, item.cutoverAdjustmentMinor)}</td></tr>
                <tr><td>Unclassified journal cash movement</td><td>{formatMoney(item.currencyCode, item.unclassifiedJournalCashMinor)}</td></tr>
                <tr><td>Unclassified till movements · not included in ledger totals ({item.unclassifiedRegisterMovementCount})</td><td>{formatMoney(item.currencyCode, item.unclassifiedRegisterMovementNetMinor)}</td></tr>
              </tbody>
            </table>
            <h5>Tender clearing balances · not counted as cash</h5>
            <table className="sales-report-table">
              <thead><tr><th>Account</th><th>Opening</th><th>Period change</th><th>Ending</th></tr></thead>
              <tbody>{item.clearingAccounts.map((account) => <tr key={`${item.currencyCode}:${account.accountCode}`}>
                <td><strong>{account.accountCode} · {account.accountName}</strong></td>
                <td>{formatMoney(item.currencyCode, account.openingBalanceMinor)}</td>
                <td>{formatMoney(item.currencyCode, account.periodChangeMinor)}</td>
                <td>{formatMoney(item.currencyCode, account.endingBalanceMinor)}</td>
              </tr>)}</tbody>
            </table>
          </Fragment>)}
          <p className="sales-report-disclosure">Physical cash location reconciliation: {cashFlowStatement.cashReconciliationStatus} · tracked cash variance {cashFlowStatement.currencies[0] ? formatMoney(cashFlowStatement.currencies[0].currencyCode, cashFlowStatement.cashLocationVarianceMinor) : cashFlowStatement.cashLocationVarianceMinor} · {cashFlowStatement.unresolvedDrawerMovementCount} unresolved movement(s). Paired cash drops and internal transfers are excluded. Cash-location events are shown as coverage, not added to journal totals.</p>
          <ul className="sales-report-disclosure">{cashFlowStatement.limitations.map((limitation) => <li key={limitation}>{limitation}</li>)}</ul>
        </div>}
        <div className="sales-report-totals" aria-label="Accounting posting coverage">
          <article><span>Sales missing journals</span><strong>{coverage.unpostedSalesCount} <small>of {coverage.salesCount}</small></strong></article>
          <article><span>Refunds missing journals</span><strong>{coverage.unpostedRefundsCount} <small>of {coverage.refundsCount}</small></strong></article>
          <article><span>Partial historical journals</span><strong>{coverage.partialJournalCount}</strong></article>
        </div>
        {(coverage.unpostedSalesCount > 0 || coverage.unpostedRefundsCount > 0) && <p className="notice notice-warning">Some source records were not journaled because their recorded line totals or tenders did not reconcile. Review the entries below before making accounting adjustments.</p>}
        {coverage.partialJournalCount > 0 && <p className="notice notice-warning">Partial historical journals preserve recorded totals. Historical product cost or tax classification was unavailable, so current catalog settings were not used to rewrite them.</p>}
        {coverage.exceptions.length > 0 && <div className="sales-report-table-wrap">
          <table className="sales-report-table">
            <thead><tr><th>Record</th><th>Date/time ({timeZone})</th><th>Coverage issue</th><th>Amount</th></tr></thead>
            <tbody>{coverage.exceptions.map((item) => <tr key={`${item.sourceType}:${item.sourceId}`}>
              <td><strong>{item.label}</strong><small>{item.sourceType.replaceAll('_', ' ')}</small></td>
              <td>{formatBranchTimestamp(item.occurredAt, timeZone)}</td>
              <td>{accountingIssueLabel(item.issueCode)}</td>
              <td>{formatMoney(item.currencyCode, item.amountMinor)}</td>
            </tr>)}</tbody>
          </table>
          {coverage.exceptionsTruncated && <p className="sales-report-disclosure">Showing the 50 most recent exceptions. Narrow the date range to review the rest.</p>}
        </div>}
      </>}
    </section>}
    {error && <div className="notice notice-danger" role="alert">{error}</div>}
    {receiptError && <div className="notice notice-danger" role="alert">{receiptError}<button type="button" className="button button-quiet button-small" onClick={() => setReceiptError(null)}>Dismiss</button></div>}
    {!report && loading && <p className="sales-report-empty" role="status">Loading sales report…</p>}
    {report && <>
      <div className="sales-report-totals" aria-label="Sales totals for selected date range">
        <article><span>Sales</span><strong>{report.matchedCount}</strong></article>
        {report.totalsByCurrency.map((totals) => <Fragment key={totals.currencyCode}>
          <article key={`${totals.currencyCode}-subtotal`}><span>{totals.currencyCode} subtotal</span><strong>{formatMoney(totals.currencyCode, totals.subtotalMinor)}</strong></article>
          <article key={`${totals.currencyCode}-tax`}><span>{totals.currencyCode} tax</span><strong>{formatMoney(totals.currencyCode, totals.taxMinor)}</strong></article>
          <article key={`${totals.currencyCode}-total`}><span>{totals.currencyCode} total · {totals.saleCount} sales</span><strong>{formatMoney(totals.currencyCode, totals.totalMinor)}</strong></article>
        </Fragment>)}
      </div>
      <div className="sales-report-table-wrap">
        <table className="sales-report-table">
          <thead><tr><th>Receipt</th><th>Date/time ({timeZone})</th><th>Cashier</th><th>Station</th><th>Payment</th><th>Total</th><th>Actions</th></tr></thead>
          <tbody>{report.rows.map((sale) => <tr key={sale.id}>
            <td><strong>{sale.receiptNumber}</strong></td>
            <td>{formatBranchTimestamp(sale.createdAt, timeZone)}</td>
            <td>{sale.cashierName}</td>
            <td>{sale.stationName}</td>
            <td>{sale.paymentMethods || '—'}</td>
            <td>{formatMoney(sale.currencyCode, sale.totalMinor)}</td>
            <td><button type="button" className="button button-quiet button-small" onClick={() => void openReceipt(sale.id)} disabled={receiptLoadingId !== null}>{receiptLoadingId === sale.id ? 'Loading…' : 'View / print'}</button></td>
          </tr>)}</tbody>
        </table>
        {!loading && report.rows.length === 0 && <p className="sales-report-empty">No sales were recorded in this date range.</p>}
      </div>
      <footer className="sales-report-pagination">
        <span>{report.matchedCount === 0 ? '0 sales' : `${offset + 1}–${pageEnd} of ${report.matchedCount}`}</span>
        <div><button type="button" className="button button-quiet" onClick={() => setOffset(Math.max(0, offset - report.pageSize))} disabled={offset === 0 || loading}>Previous</button><button type="button" className="button button-quiet" onClick={() => setOffset(offset + report.pageSize)} disabled={pageEnd >= report.matchedCount || offset >= 50000 || loading}>Next</button></div>
      </footer>
      <p className="sales-report-disclosure">Totals cover the full selected date range. CSV export contains the currently visible page. Values are server-recorded manual tenders; this report does not confirm external processor settlement.</p>
    </>}
    {receipt && <div className="receipt-detail-backdrop">
      <section className="receipt-detail-card" role="dialog" aria-modal="true" aria-labelledby="receipt-detail-title">
        <header className="receipt-detail-actions"><div><p className="eyebrow">COMPLETED SALE</p><h3 id="receipt-detail-title">{receipt.receiptNumber}</h3></div><button type="button" className="button button-quiet button-small" onClick={() => setReceipt(null)} aria-label="Close receipt"><X size={15}/> Close</button></header>
        <div className="receipt-detail-printable">
          <h2>{branchName}</h2><p>{receipt.stationName} · {formatBranchTimestamp(receipt.createdAt, timeZone)}</p>
          <p>Receipt: {receipt.receiptNumber}</p><p>Served by: {receipt.cashierName}</p>
          {receipt.customerName && <p>Customer: {receipt.customerName}</p>}
          <div className="receipt-detail-lines">{receipt.lines.map((line) => <div key={line.lineNumber}><span>{line.quantity} × {line.name}<small>{line.sku} · {formatMoney(receipt.currencyCode, line.unitPriceMinor)} each</small></span><strong>{formatMoney(receipt.currencyCode, line.totalMinor)}</strong></div>)}</div>
          <div className="receipt-detail-totals"><div><span>Subtotal</span><strong>{formatMoney(receipt.currencyCode, receipt.subtotalMinor)}</strong></div><div><span>Tax</span><strong>{formatMoney(receipt.currencyCode, receipt.taxMinor)}</strong></div><div className="receipt-detail-grand-total"><span>Total</span><strong>{formatMoney(receipt.currencyCode, receipt.totalMinor)}</strong></div></div>
          <div className="receipt-detail-tenders"><span>Payment</span>{receipt.tenders.map((tender, index) => <div key={`${tender.label}-${index}`}><span>{tender.label}</span><strong>{formatMoney(receipt.currencyCode, tender.amountMinor)}</strong></div>)}</div>
          <p className="receipt-detail-thanks">Thank you for shopping with us.</p>
        </div>
        <footer className="receipt-detail-actions"><span>Receipt values are read from the recorded sale.</span><button type="button" className="button button-primary" onClick={() => window.print()}><Printer size={15}/> Print receipt</button></footer>
      </section>
    </div>}
  </section>
}
