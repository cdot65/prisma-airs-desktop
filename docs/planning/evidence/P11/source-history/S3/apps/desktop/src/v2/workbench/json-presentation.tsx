import { useLayoutEffect, useMemo, useRef, type CSSProperties } from 'react'
import { jsonLanguage } from '@codemirror/lang-json'
import { classHighlighter, highlightTree } from '@lezer/highlight'

// Large editable bodies remain ordinary editable text. Highlighting never
// changes their value or introduces a second body/preview size contract.
const HIGHLIGHT_LIMIT = 256 * 1024
const TOKEN_LIMIT = 12_000
const MONO = "ui-monospace, 'SF Mono', Menlo, monospace"

/** JSON whitespace changes only: retain number/string spellings and duplicate keys. */
export function formatJson(text: string): string | null {
    if (text.trim() === '' || text.length > HIGHLIGHT_LIMIT) return null
    const tree = jsonLanguage.parser.parse(text)
    const tokens: string[] = []
    let invalid = false
    tree.iterate({ enter(node) {
        if (node.type.isError) invalid = true
        if (!node.node.firstChild) tokens.push(text.slice(node.from, node.to))
    } })
    if (invalid || tokens.length === 0 || tokens.length > TOKEN_LIMIT) return null
    const parts: string[] = []
    let depth = 0
    let length = 0
    const put = (s: string) => { parts.push(s); length += s.length }
    const line = () => put(`\n${'  '.repeat(depth)}`)
    for (let i = 0; i < tokens.length; i += 1) {
        const token = tokens[i]!
        const previous = tokens[i - 1]
        const next = tokens[i + 1]
        if (token === '{' || token === '[') {
            put(token)
            if (next !== '}' && next !== ']') {
                depth += 1
                if (depth > 64) return null
                line()
            }
        } else if (token === '}' || token === ']') {
            if (previous !== '{' && previous !== '[') { depth -= 1; line() }
            put(token)
        } else if (token === ',') { put(','); line() }
        else if (token === ':') put(': ')
        else put(token)
        if (length > 1024 * 1024) return null
    }
    return parts.join('')
}

function color(classes: string): string {
    if (classes.includes('propertyName')) return 'var(--v2-acc)'
    if (classes.includes('string')) return 'var(--v2-ok)'
    if (classes.includes('number') || classes.includes('bool') || classes.includes('null')) return 'var(--v2-flag)'
    return 'var(--v2-soft)'
}

export function JsonText({ text, numbered = false }: { text: string; numbered?: boolean }) {
    const ranges = useMemo(() => {
        if (text.length > HIGHLIGHT_LIMIT) return []
        const ranges: { from: number; to: number; classes: string }[] = []
        highlightTree(jsonLanguage.parser.parse(text), classHighlighter, (from, to, classes) => ranges.push({ from, to, classes }))
        return ranges.length > TOKEN_LIMIT ? [] : ranges
    }, [text])
    let rangeIndex = 0
    const paint = (from: number, to: number) => {
        const out = []
        let offset = from
        while (rangeIndex < ranges.length && ranges[rangeIndex]!.to <= from) rangeIndex += 1
        for (let i = rangeIndex; i < ranges.length; i += 1) {
            const r = ranges[i]!
            if (r.from >= to) break
            const start = Math.max(from, r.from)
            const end = Math.min(to, r.to)
            if (start > offset) out.push(text.slice(offset, start))
            out.push(<span key={start} data-json-token={r.classes} style={{ color: color(r.classes) }}>{text.slice(start, end)}</span>)
            offset = end
        }
        out.push(text.slice(offset, to))
        return out
    }
    if (!numbered) return <>{paint(0, text.length)}</>
    let offset = 0
    return <>{text.split('\n').map((line) => {
        const from = offset
        offset += line.length + 1
        return <span key={from} data-line=''>{paint(from, from + line.length)}{'\n'}</span>
    })}</>
}

/** A native textarea owns editing, selection, IME and copy. Its inert color layer is never a second input. */
export function JsonBodyEditor({ id, value, readOnly, style, onChange }: { id: string; value: string; readOnly: boolean; style: CSSProperties; onChange: (text: string) => void }) {
    const paint = useRef<HTMLPreElement>(null)
    const input = useRef<HTMLTextAreaElement>(null)
    const highlighted = value.length <= HIGHLIGHT_LIMIT
    const sync = () => {
        if (!input.current || !paint.current) return
        // A non-overlay native scrollbar reduces the textarea's text width.
        // Match that width before wrapping the inert color layer.
        if (input.current.clientWidth > 0) paint.current.style.width = `${input.current.clientWidth + 2}px`
        paint.current.scrollTop = input.current.scrollTop
        paint.current.scrollLeft = input.current.scrollLeft
    }
    useLayoutEffect(() => {
        sync()
        if (!input.current || typeof ResizeObserver === 'undefined') return
        const observer = new ResizeObserver(sync)
        observer.observe(input.current)
        return () => observer.disconnect()
    }, [highlighted])
    const shared: CSSProperties = { ...style, fontFamily: MONO, fontWeight: 400, letterSpacing: 'normal', whiteSpace: 'pre-wrap', overflowWrap: 'break-word', tabSize: 4 }
    return <div className='relative min-w-0' data-slot='json-body-editor'>
        {highlighted ? <pre ref={paint} aria-hidden='true' style={{ ...shared, position: 'absolute', inset: 0, margin: 0, height: '100%', overflow: 'hidden', pointerEvents: 'none', borderColor: 'transparent' }}><JsonText text={value} />{value.endsWith('\n') ? ' ' : ''}</pre> : null}
        <textarea ref={input} id={id} value={value} readOnly={readOnly} spellCheck={false} style={{ ...shared, display: 'block', position: 'relative', background: highlighted ? 'transparent' : style.background, color: highlighted ? 'transparent' : 'var(--v2-ink)', caretColor: 'var(--v2-ink)' }} onScroll={sync} onChange={(e) => onChange(e.target.value)} />
    </div>
}

/** One complete template string, with noninteractive inline variable color. */
export function UrlTemplateInput({ label, value, onChange }: { label: string; value: string; onChange: (text: string) => void }) {
    const paint = useRef<HTMLPreElement>(null)
    const parts = value.split(/(\{\{[^{}]+\}\})/g)
    const shared: CSSProperties = { fontFamily: MONO, fontSize: 12.5, lineHeight: '32px', letterSpacing: 'normal', padding: 0, border: 0, margin: 0 }
    return <span className='relative min-w-0 flex-grow' data-slot='template-input'>
        <pre ref={paint} aria-hidden='true' style={{ ...shared, position: 'absolute', inset: 0, overflow: 'hidden', whiteSpace: 'pre', pointerEvents: 'none', color: 'var(--v2-ink)' }}>{parts.map((part, i) => part.startsWith('{{') && part.endsWith('}}') ? <span key={i} data-variable='' style={{ color: 'var(--v2-acc)' }}>{part}</span> : part)}</pre>
        <input aria-label={label} value={value} autoComplete='off' spellCheck={false} className='relative min-h-0 min-w-0 w-full bg-transparent' style={{ ...shared, borderRadius: 0, minHeight: 0, background: 'transparent', color: 'transparent', caretColor: 'var(--v2-ink)' }} onScroll={(e) => { if (paint.current) paint.current.scrollLeft = e.currentTarget.scrollLeft }} onChange={(e) => onChange(e.target.value)} />
    </span>
}
