import type { ReportKind, SessionSearch } from '@prisma-airs/desktop-contracts'

export type Row = Record<string, unknown>
export type Verdict = 'malicious' | 'benign' | 'unknown'

const text = (value: unknown): string => (typeof value === 'string' ? value : '')

/** v1's verdict of a violation breakdown's counts. */
function violation(counts: unknown): boolean | null {
    if (typeof counts !== 'object' || counts === null) return null
    const c = counts as Record<string, unknown>
    const n = (k: string) => (typeof c[k] === 'number' ? (c[k] as number) : null)
    const buckets = ['critical', 'high', 'medium', 'low'].map(n)
    const total = n('total')
    if ((total ?? 0) > 0 || buckets.some((b) => (b ?? 0) > 0)) return true
    if (total === 0 || buckets.every((b) => b === 0)) return false
    return null
}

/** The detectors a row reports a violation for (v1 `session_detectors`). */
export function detectors(row: Row): string[] {
    const entries = row.detection_type_violation_breakdown
    if (!Array.isArray(entries)) return []
    return entries
        .filter((e): e is Row => typeof e === 'object' && e !== null && violation((e as Row).violation_breakdown) === true)
        .map((e) => text(e.detection_type))
        .filter((d) => d.length > 0)
}

/** A row's verdict and action as v1 read them (`Scan::read`). */
export function verdictOf(row: Row): { verdict: Verdict; action: string } {
    const status = text(row.status).trim().toLowerCase()
    const counts = violation(row.violation_breakdown)
    const verdict: Verdict =
        detectors(row).length > 0 || counts === true || ['block', 'blocked', 'alert', 'alerted'].includes(status)
            ? 'malicious'
            : counts === false || ['allow', 'allowed'].includes(status)
              ? 'benign'
              : 'unknown'
    const action =
        status === 'block' || status === 'blocked'
            ? 'Blocked'
            : status === 'alert' || status === 'alerted'
              ? 'Alerted'
              : status === 'allow' || status === 'allowed'
                ? 'Allowed'
                : status
    return { verdict, action }
}

export const VERDICT_TEXT: Record<Verdict, string> = { malicious: 'Malicious', benign: 'Benign', unknown: 'No verdict' }

export const sessionId = (row: Row) => text(row.session_id)
export const appId = (row: Row) => text(row.application_id)
export const appName = (row: Row) => text(row.application_name) || text(row.app_name)
export const scanId = (row: Row) => text(row.scan_id)
export const subRequest = (row: Row) => (typeof row.scan_sub_req_id === 'number' ? row.scan_sub_req_id : 0)

/** v1's quick-pick categories (Rust refuses any other non-empty value). */
export const CATEGORIES: Record<ReportKind, readonly string[]> = {
    false_positive: ['Benign business content', 'Expected internal data', 'Wrong detector', 'Test or sample data'],
    false_negative: ['Prompt injection missed', 'Sensitive data missed', 'Harmful content missed', 'Malicious URL missed'],
}

export const KIND_TEXT: Record<ReportKind, string> = { false_positive: 'False positive', false_negative: 'False negative' }

/** The kind a scan's verdict suggests (v1 `ReportKind::suggested`). */
export const suggestedKind = (verdict: Verdict): ReportKind => (verdict === 'malicious' ? 'false_positive' : 'false_negative')

/** What a search covered, in words: no claim beyond what it read. */
export function coverage(search: SessionSearch, found: number): string {
    const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? '' : word.endsWith('h') ? 'es' : 's'}`
    switch (search.kind) {
        case 'sessionId':
            if (search.cached) return 'Found in this investigation.'
            if (found > 0)
                return search.complete
                    ? `Found among all ${plural(search.scanned, 'session')} of the last 30 days.`
                    : `Found among the newest ${plural(search.scanned, 'session')} of the last 30 days.`
            return search.complete
                ? `No session with this ID in the last 30 days (${plural(search.scanned, 'session')} read).`
                : `Not among the newest ${plural(search.scanned, 'session')} of the last 30 days. Older sessions were not read, so it may still exist.`
        case 'scanId':
            return search.scan !== null ? 'Scan found. Its metadata is shown; content stays hidden.' : 'The service has no scan with this ID.'
        case 'detector': {
            const head = `${plural(found, 'session')} reporting ${search.query} among ${plural(search.scanned, 'session')} read from the last 24 hours`
            if (search.matchLimit) return `${head}. Stopped at ${plural(100, 'match')}; more may exist.`
            if (search.complete) return `${head}. That is every session of the last 24 hours.`
            return `${head}. Stopped after ${plural(1000, 'session')}; later sessions were not read.`
        }
    }
}
