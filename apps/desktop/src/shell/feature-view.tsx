import { forwardRef, useId } from 'react'
import { Callout, Kicker } from '@ai-security-academy/presentation'
import { Info } from 'lucide-react'
import type { FeatureEntry, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import type { Dispatch } from '../features/tenants/dispatch'
import { TenantsSettings } from '../features/tenants/tenants-settings'

interface HeadingProps {
    id: string
    kicker: string
    title: string
}

/** Page heading that receives focus after navigation (tabIndex -1). */
export const PageHeading = forwardRef<HTMLHeadingElement, HeadingProps>(function PageHeading({ id, kicker, title }, ref) {
    return (
        <header className='mb-6 flex flex-col gap-2'>
            <Kicker>{kicker}</Kicker>
            <h1
                ref={ref}
                id={id}
                tabIndex={-1}
                data-slot='page-heading'
                className='font-display text-academy-ink text-[28px] leading-tight font-semibold tracking-[-0.01em] outline-none'
            >
                {title}
            </h1>
        </header>
    )
})

export const UnavailableView = forwardRef<HTMLHeadingElement, { feature: FeatureEntry }>(function UnavailableView({ feature }, ref) {
    const planned = feature.availability.state === 'unavailable' ? feature.availability.plannedChunk : ''
    return (
        <section aria-labelledby={`${feature.id}-heading`} data-slot='feature-unavailable'>
            <PageHeading ref={ref} id={`${feature.id}-heading`} kicker='Not available' title={feature.label} />
            <Callout tone='default' icon={Info}>
                <p>
                    {feature.label} is not available in this build. Nothing here contacts a Prisma AIRS
                    tenant or the Academy.
                </p>
                <p className='text-academy-ink-faint mt-2 font-mono text-[11px] tracking-[0.12em] uppercase'>
                    Planned in {planned}
                </p>
            </Callout>
        </section>
    )
})

const THEMES: { value: Theme; label: string; hint: string }[] = [
    { value: 'system', label: 'Match System', hint: 'Follow the macOS appearance' },
    { value: 'light', label: 'Light', hint: 'Paper light theme' },
    { value: 'dark', label: 'Dark', hint: 'Gruvbox dark theme' },
]

interface SettingsProps {
    snapshot: ShellSnapshot
    pending: boolean
    onSetAppearance: (theme: Theme) => void
    dispatch: Dispatch
}

export const SettingsView = forwardRef<HTMLHeadingElement, SettingsProps>(function SettingsView(
    { snapshot, pending, onSetAppearance, dispatch },
    ref
) {
    const legend = useId()
    const appearance = snapshot.appearance
    return (
        <section aria-labelledby='settings-heading' data-slot='settings'>
            <PageHeading ref={ref} id='settings-heading' kicker='Desktop' title='Settings' />
            <fieldset aria-labelledby={legend} aria-busy={pending} className='border-academy-rule flex flex-col gap-3 rounded-[12px] border p-5'>
                <legend id={legend} className='text-academy-ink px-1 text-[15px] font-semibold'>
                    Appearance
                </legend>
                <p className='text-academy-ink-soft text-[13px]'>
                    A desktop-wide preference. It does not change your Academy account settings.
                </p>
                {THEMES.map((option) => (
                    <label key={option.value} className='flex min-h-11 cursor-pointer items-center gap-3'>
                        <input
                            type='radio'
                            name='appearance'
                            value={option.value}
                            checked={appearance === option.value}
                            onChange={() => onSetAppearance(option.value)}
                            className='accent-[var(--academy-accent)] size-4'
                        />
                        <span className='flex flex-col'>
                            <span className='text-academy-ink text-[14px] font-medium'>{option.label}</span>
                            <span className='text-academy-ink-faint text-[12px]'>{option.hint}</span>
                        </span>
                    </label>
                ))}
            </fieldset>
            <div className='mt-6'>
                <TenantsSettings snapshot={snapshot} dispatch={dispatch} />
            </div>
        </section>
    )
})
