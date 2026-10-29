import { forwardRef, useId, useState } from 'react'
import { Button, Callout, Kicker } from '@ai-security-academy/presentation'
import { Info } from 'lucide-react'
import type { FeatureEntry, Intent, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../features/tenants/dispatch'
import { DiagnosticsPanel } from '../features/diagnostics/diagnostics-panel'
import { AcademyAccount } from '../features/academy/academy-account'
import { TenantsSettings } from '../features/tenants/tenants-settings'
import type { ObserveManagement } from '../features/administration/management-read'
import { useV2Layout } from './presentation'

interface HeadingProps {
    id: string
    kicker: string
    title: string
}

/** Page heading that receives focus after navigation (tabIndex -1). */
export const PageHeading = forwardRef<HTMLHeadingElement, HeadingProps>(function PageHeading({ id, kicker, title }, ref) {
    // In the v2 shell: its title row (26px/700, no kicker), like every other destination (C33).
    if (useV2Layout()) {
        return (
            <header className='mb-4 flex flex-col gap-1' data-slot='page-header-v2'>
                <h1 ref={ref} id={id} tabIndex={-1} data-slot='page-heading' className='font-bold outline-none' style={{ fontSize: 'var(--v2-title)' }}>
                    {title}
                </h1>
            </header>
        )
    }
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
    observeManagement: ObserveManagement
}

export const SettingsView = forwardRef<HTMLHeadingElement, SettingsProps>(function SettingsView(
    { snapshot, pending, onSetAppearance, dispatch, observeManagement },
    ref
) {
    const legend = useId()
    const appearance = snapshot.appearance
    const [interfaceBusy, setInterfaceBusy] = useState(false)
    const [interfaceStatus, setInterfaceStatus] = useState<{ text: string; retry?: Intent } | null>(null)
    // The observed `uiVariant` stays authoritative: a refusal is reported, an
    // unknown delivery stays visibly unconfirmed with Retry (DESIGN-V2 §7),
    // and nothing here re-sends on its own.
    async function changeInterface(intent: Intent) {
        setInterfaceBusy(true)
        setInterfaceStatus(null)
        const outcome = await dispatch(intent)
        setInterfaceBusy(false)
        if (outcome.kind === 'admitted') return
        if (outcome.kind === 'unknown') setInterfaceStatus({ text: 'The interface change was sent but the desktop did not confirm it. The current interface is shown.', retry: intent })
        else setInterfaceStatus({ text: outcomeMessage(outcome) ?? 'The interface change could not be delivered.' })
    }
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
            <fieldset className='border-academy-rule mt-6 flex flex-col gap-3 rounded-[12px] border p-5' data-slot='interface'>
                <legend className='text-academy-ink px-1 text-[15px] font-semibold'>Interface</legend>
                <p className='text-academy-ink-soft text-[13px]'>
                    The new interface (Refined Native) is arriving destination by destination. Switching is a
                    desktop-wide preference and keeps every tenant and credential as it is.
                </p>
                <label className='flex min-h-11 cursor-pointer items-center gap-3'>
                    <input
                        type='checkbox'
                        checked={snapshot.uiVariant === 'v2'}
                        aria-busy={interfaceBusy}
                        onChange={(e) => void changeInterface({ kind: 'setUiVariant', variant: e.target.checked ? 'v2' : 'legacy' })}
                        className='accent-[var(--academy-accent)] size-4'
                    />
                    <span className='text-academy-ink text-[14px] font-medium'>Use the new interface</span>
                </label>
                <p role='status' aria-live='polite' data-slot='interface-status' className='text-academy-ink-soft flex items-center gap-3 text-[13px]'>
                    <span>{interfaceStatus?.text}</span>
                    {interfaceStatus?.retry && (
                        <>
                            <Button type='button' tier='secondary' onClick={() => void changeInterface(interfaceStatus.retry as Intent)}>
                                Retry
                            </Button>
                            <Button type='button' tier='secondary' onClick={() => setInterfaceStatus(null)}>
                                Dismiss
                            </Button>
                        </>
                    )}
                </p>
            </fieldset>
            <div className='mt-6'>
                <AcademyAccount view={snapshot.academy} dispatch={dispatch} />
            </div>
            <div className='mt-6'>
                <TenantsSettings snapshot={snapshot} dispatch={dispatch} observeManagement={observeManagement} />
            </div>
            <div className='mt-6'>
                <DiagnosticsPanel view={snapshot.diagnostics} dispatch={dispatch} />
            </div>
        </section>
    )
})
