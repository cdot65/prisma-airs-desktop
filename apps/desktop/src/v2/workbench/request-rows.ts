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
    /**
     * The separator exactly as the line has it (`=` for a param; `:` and the
     * spacing after it for a header), so a row written back is the line it
     * came from. A new row takes the default.
     */
    sep?: string
}

export type RowKind = 'query' | 'headers'

const SEPARATOR: Record<RowKind, string> = { query: '=', headers: ':' }
const DEFAULT_SEP: Record<RowKind, string> = { query: '=', headers: ': ' }

/**
 * Rows from the field's text, or null when a non-empty line has no
 * separator (the text is then edited as text, never rewritten). Names and
 * values are verbatim: desktop-core keeps a query value as written (a
 * padded `q=  padded  ` stays padded), and a space typed at the end of a
 * value must survive the next render for the next word to follow it (the
 * table is controlled: trimming here lost it before the next character).
 * A header's value is the text after the colon and its spacing; that
 * spacing is kept on the row and written back as it was.
 */
export function parseRows(text: string, kind: RowKind): Row[] | null {
    const rows: Row[] = []
    for (const raw of text.split('\n')) {
        if (raw.trim() === '') continue
        const enabled = !raw.startsWith('# ')
        const line = enabled ? raw : raw.slice(2)
        const at = line.indexOf(SEPARATOR[kind])
        if (at < 0) return null
        const rest = line.slice(at + 1)
        const spacing = kind === 'headers' ? (/^\s*/.exec(rest)?.[0] ?? '') : ''
        rows.push({
            enabled,
            name: line.slice(0, at),
            value: rest.slice(spacing.length),
            sep: SEPARATOR[kind] + spacing,
        })
    }
    return rows
}

/** The field's text for these rows: each row on the line it came from, a new row with the default separator. */
export function serializeRows(rows: Row[], kind: RowKind): string {
    return rows.map((r) => `${r.enabled ? '' : '# '}${r.name}${r.sep ?? DEFAULT_SEP[kind]}${r.value}`).join('\n')
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
export { formatJson as prettyJson } from './json-presentation'
