import { forwardRef, useId, useState } from 'react'
import { Callout } from '@ai-security-academy/presentation'
import { Info } from 'lucide-react'
import type { FeatureEntry, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../modules/tenants/dispatch'
import { DiagnosticsPanel } from '../modules/diagnostics/diagnostics-panel'
import { AcademyAccount } from '../modules/academy/academy-account'
import { TenantsSettings } from '../modules/tenants/tenants-settings'
import type { ObserveManagement } from '../modules/administration/management-read'
import { PageIntro } from '../v2/page-intro'

interface HeadingProps {
    id: string
    kicker: string
    title: string
}

/** Page heading that receives focus after navigation (tabIndex -1). */
export const PageHeading = forwardRef<HTMLHeadingElement, HeadingProps>(function PageHeading({ id, title }, ref) {
    return (
        <header className='mb-4 flex flex-col gap-1' data-slot='page-header'>
            <h1 ref={ref} id={id} tabIndex={-1} data-slot='page-heading' className='font-bold outline-none' style={{ fontSize: 'var(--v2-title)' }}>
                {title}
            </h1>
        </header>
    )
})

export const UnavailableView = forwardRef<HTMLHeadingElement, { feature: FeatureEntry }>(function UnavailableView({ feature }) {
    const planned = feature.availability.state === 'unavailable' ? feature.availability.plannedChunk : ''
    return (
        <section aria-label={feature.label} data-slot='feature-unavailable'>
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

interface SettingsProps {
    snapshot: ShellSnapshot
    pending: boolean
    onSetAppearance: (theme: Theme) => void
    dispatch: Dispatch
    observeManagement: ObserveManagement
}

export const SettingsView = forwardRef<HTMLHeadingElement, SettingsProps>(function SettingsView(
    { snapshot, pending, onSetAppearance, dispatch, observeManagement }
) {
    const legend = useId()
    const [themeBusy, setThemeBusy] = useState(false)
    const [message, setMessage] = useState<string | null>(null)
    async function setColorTheme(theme: ShellSnapshot['colorTheme']) {
        setThemeBusy(true)
        setMessage(null)
        const outcome = await dispatch({ kind: 'setColorTheme', theme })
        setThemeBusy(false)
        setMessage(outcomeMessage(outcome))
    }
    return (
        <section aria-label='Settings' data-slot='settings' className='flex flex-col gap-4'>
            <PageIntro description='Personalize the desktop, manage tenant connections and review local diagnostics.' />
            <div className='settings-grid'>
                <fieldset aria-labelledby={legend} aria-busy={pending || themeBusy} className='settings-card'>
                    <legend id={legend}>Appearance</legend>
                    <p className='settings-description'>Desktop preferences do not change your Academy account settings.</p>
                    <div className='settings-fields'>
                        <label className='settings-field'>
                            Appearance
                            <select aria-label='Appearance' value={snapshot.appearance} disabled={pending} onChange={(e) => onSetAppearance(e.target.value as Theme)}>
                                <option value='system'>Match system</option>
                                <option value='light'>Light</option>
                                <option value='dark'>Dark</option>
                            </select>
                        </label>
                        <label className='settings-field'>
                            Theme
                            <select aria-label='Theme' value={snapshot.colorTheme} disabled={themeBusy} onChange={(e) => void setColorTheme(e.target.value as ShellSnapshot['colorTheme'])}>
                                <option value='prismaAirs'>Prisma AIRS</option>
                                <option value='gruvbox'>Gruvbox</option>
                            </select>
                        </label>
                    </div>
                    {message && <p role='alert'>{message}</p>}
                </fieldset>
                <AcademyAccount view={snapshot.academy} dispatch={dispatch} />
                <DiagnosticsPanel view={snapshot.diagnostics} dispatch={dispatch} />
            </div>
            <TenantsSettings snapshot={snapshot} dispatch={dispatch} observeManagement={observeManagement} />
        </section>
    )
})
