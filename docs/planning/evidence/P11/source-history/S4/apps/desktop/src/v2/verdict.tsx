import type { LucideIcon } from 'lucide-react'
import { CircleAlert, CircleCheck, CircleSlash, Info } from 'lucide-react'

export type VerdictTone = 'blocked' | 'flagged' | 'allowed' | 'info'

const TONES: Record<VerdictTone, { icon: LucideIcon; fg: string; bg: string }> = {
    blocked: { icon: CircleSlash, fg: 'var(--v2-block)', bg: 'var(--v2-block-bg)' },
    flagged: { icon: CircleAlert, fg: 'var(--v2-flag)', bg: 'var(--v2-flag-bg)' },
    allowed: { icon: CircleCheck, fg: 'var(--v2-ok)', bg: 'var(--v2-ok-bg)' },
    info: { icon: Info, fg: 'var(--v2-soft)', bg: 'var(--v2-inset)' },
}

/**
 * A verdict or severity is colour plus icon plus word, never colour alone
 * (invariant I9). The word is the accessible name; the icon is decorative.
 */
export function Verdict({ tone, word, reason }: { tone: VerdictTone; word: string; reason?: string }) {
    const { icon: Icon, fg, bg } = TONES[tone]
    return (
        <span
            data-slot='verdict'
            data-tone={tone}
            style={{ color: fg, background: bg }}
            className='inline-flex min-h-7 items-center gap-1.5 rounded-[7px] px-2 text-[12px] font-semibold'
        >
            <Icon aria-hidden='true' size={14} strokeWidth={2.4} />
            <span>{word}</span>
            {reason && <span className='font-normal opacity-90'> · {reason}</span>}
        </span>
    )
}
