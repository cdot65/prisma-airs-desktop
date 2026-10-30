/**
 * The request's Params and Headers as rows (C41): the files keep one row
 * per line (`name=value` for params, `Name: value` for headers, `# ` in
 * front of a disabled row); the table edits those lines and writes them
 * back in the same form, so Rust reads what it always read.
 */
export interface Row {
    enabled: boolean
    name: string
    value: string
}

export type RowKind = 'query' | 'headers'

const SEPARATOR: Record<RowKind, string> = { query: '=', headers: ':' }

/**
 * Rows from the field's text, or null when a non-empty line has no
 * separator (the text is then edited as text, never rewritten).
 */
export function parseRows(text: string, kind: RowKind): Row[] | null {
    const rows: Row[] = []
    for (const raw of text.split('\n')) {
        if (raw.trim() === '') continue
        const enabled = !raw.startsWith('# ')
        const line = enabled ? raw : raw.slice(2)
        const at = line.indexOf(SEPARATOR[kind])
        if (at < 0) return null
        rows.push({
            enabled,
            name: line.slice(0, at).trim(),
            value: line.slice(at + 1).trim(),
        })
    }
    return rows
}

/** The field's text for these rows. */
export function serializeRows(rows: Row[], kind: RowKind): string {
    const sep = kind === 'query' ? '=' : ': '
    return rows.map((r) => `${r.enabled ? '' : '# '}${r.name}${sep}${r.value}`).join('\n')
}

/** How many rows are enabled (the sub-tab's count). */
export function enabledCount(text: string, kind: RowKind): number {
    const rows = parseRows(text, kind)
    return rows ? rows.filter((r) => r.enabled).length : text.split('\n').filter((l) => l.trim() !== '' && !l.startsWith('# ')).length
}

/** `{{base}}/path` → the variable prefix and the rest, for the URL's display. */
export function splitUrl(url: string): { base: string; rest: string } {
    const m = /^(\{\{[^}]+\}\})(.*)$/.exec(url)
    return m ? { base: m[1] ?? '', rest: m[2] ?? '' } : { base: '', rest: url }
}

/** A body's JSON, pretty-printed, or null when it is not JSON. */
export function prettyJson(text: string): string | null {
    try {
        return JSON.stringify(JSON.parse(text), null, 2)
    } catch {
        return null
    }
}
