import type { ButtonHTMLAttributes, InputHTMLAttributes } from 'react'

/** The prototype's 32px controls (R2App): a primary accent button, a bordered secondary, a plain tertiary. */
export function Btn({ tier = 'secondary', className = '', style, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { tier?: 'primary' | 'secondary' | 'tertiary' }) {
    const look =
        tier === 'primary'
            ? { background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)', border: 0 }
            : tier === 'secondary'
              ? { background: 'var(--v2-paper)', color: 'var(--v2-ink)', border: '1px solid var(--v2-rule)' }
              : { background: 'transparent', color: 'var(--v2-soft)', border: '1px solid var(--v2-rule)' }
    return (
        <button
            type='button'
            {...rest}
            className={`flex h-8 min-h-0 items-center justify-center rounded-[7px] px-3 text-[12px] whitespace-nowrap disabled:cursor-not-allowed disabled:opacity-60 ${tier === 'primary' ? 'font-semibold' : ''} ${className}`}
            style={{ ...look, ...style }}
        />
    )
}

/** A 32px text field with the prototype's control border. */
export function Field({ className = '', style, ...rest }: InputHTMLAttributes<HTMLInputElement>) {
    return (
        <input
            {...rest}
            className={`h-8 min-h-0 rounded-[7px] border px-2.5 text-[13px] ${className}`}
            style={{ borderColor: 'var(--v2-ctl)', background: 'var(--v2-paper)', color: 'var(--v2-ink)', ...style }}
        />
    )
}
