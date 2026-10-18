import { useCallback, useEffect, useRef } from 'react'
import { ShellBrand, ShellFrame, ShellRail, ShellTopbar, type ShellNavItem } from '@ai-security-academy/presentation'
import type { DesktopClient, FeatureEntry, FeatureId, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
import { AcademyView } from '../features/academy/academy-view'
import { AdministrationView } from '../features/administration/administration-view'
import { RuntimeView } from '../features/runtime/runtime-view'
import type { Dispatch } from '../features/tenants/dispatch'
import { CommandPalette } from './command-palette'
import { SettingsView, UnavailableView } from './feature-view'
import { FEATURE_ICONS } from './icons'

export interface DesktopShellProps {
    snapshot: ShellSnapshot
    active: FeatureId
    paletteOpen: boolean
    appearancePending: boolean
    status: string
    onNavigate: (id: FeatureId) => void
    onOpenPalette: () => void
    onClosePalette: () => void
    onSetAppearance: (theme: Theme) => void
    dispatch: Dispatch
    /** The native client, for the Academy learner transport. */
    client: DesktopClient
}

function navItem(feature: FeatureEntry): ShellNavItem {
    const unavailable = feature.availability.state === 'unavailable'
    return {
        id: feature.id,
        label: feature.label,
        icon: FEATURE_ICONS[feature.id],
        ...(unavailable ? { count: 'Soon', countLabel: 'Not available in this build' } : {}),
    }
}

/**
 * Desktop composition of the Academy-owned shell: local feature navigation,
 * native-menu-driven palette and Rust-owned appearance. No learner account,
 * gamification, router or hosted provider is mounted at the shell level:
 * Academy's learner pages and their router mount only inside the Academy
 * destination.
 */
export function DesktopShell(props: DesktopShellProps) {
    const { snapshot, active } = props
    const heading = useRef<HTMLHeadingElement>(null)
    const firstRender = useRef(true)
    const feature = snapshot.features.find((f) => f.id === active) ?? snapshot.features[0]
    const client = props.client
    const observeManagement = useCallback((operation: string) => client.observeManagement(operation), [client])

    useEffect(() => {
        // Move focus to the new page heading after navigation (not on launch).
        if (firstRender.current) {
            firstRender.current = false
            return
        }
        heading.current?.focus()
    }, [active])

    useEffect(() => {
        // WebKit does not scroll when arrow keys move focus within a radio
        // group, so bring every newly focused control into view; the root
        // scroll padding keeps it clear of the sticky topbar and the shared
        // frame's bottom fade (C01-N10).
        const reveal = (event: FocusEvent) => {
            const target = event.target
            if (!(target instanceof Element)) return
            target.scrollIntoView({ block: 'nearest' })
            // WebKit's own Tab scrolling ignores scroll-padding and runs after
            // focus events, which can leave a control under the bottom fade at
            // the minimum window size (C08-N11): on the next frame, move a
            // focused page control inside the padded area.
            if (!target.closest('[data-slot="app-main"]')) return
            window.requestAnimationFrame(() => {
                if (document.activeElement !== target) return
                const rect = target.getBoundingClientRect()
                if (rect.height === 0) return
                const style = getComputedStyle(document.documentElement)
                const top = parseFloat(style.scrollPaddingTop) || 0
                const bottom = window.innerHeight - (parseFloat(style.scrollPaddingBottom) || 0)
                if (rect.bottom > bottom && rect.top - (rect.bottom - bottom) >= top) window.scrollBy(0, rect.bottom - bottom)
                else if (rect.top < top) window.scrollBy(0, rect.top - top)
            })
        }
        document.addEventListener('focusin', reveal)
        return () => document.removeEventListener('focusin', reveal)
    }, [])

    if (!feature) return null
    return (
        <ShellFrame
            rail={
                <ShellRail
                    brand={<ShellBrand name='Prisma AIRS' tagline='Desktop v2' />}
                    nav={snapshot.features.map(navItem)}
                    activeId={feature.id}
                    navLabel='Features'
                    // The rail scrolls when zoom or a small window makes its
                    // destinations taller than the viewport (C01-N09/N10).
                    // Below 900 CSS px (e.g. 200% zoom) the frame's 60px
                    // bottom fade spans the rail too, so the rail can scroll
                    // a focused destination clear of it.
                    className='overflow-y-auto overscroll-contain max-[900px]:pb-[82px] max-[900px]:scroll-pb-[82px]'
                    renderLink={({ item, active: isActive, children, className, ...rest }) => (
                        // Buttons center text by default; the shared rail
                        // styling assumes left-aligned link content.
                        <button
                            type='button'
                            {...rest}
                            className={`${className} w-full text-left`}
                            onClick={() => props.onNavigate(item.id as FeatureId)}
                        >
                            {children}
                        </button>
                    )}
                    footer={
                        snapshot.fixtureBuild ? (
                            <p data-slot='build-identity' className='text-academy-warning px-2.5 py-3 font-mono text-[10px] tracking-[0.14em] uppercase'>
                                Fixture build
                            </p>
                        ) : undefined
                    }
                />
            }
            topbar={
                <ShellTopbar
                    breadcrumbs={[{ label: 'Prisma AIRS' }, { label: feature.label }]}
                    renderCrumbLink={(crumb, className) => <span className={className}>{crumb.label}</span>}
                    actions={
                        <button
                            type='button'
                            data-slot='palette-trigger'
                            onClick={props.onOpenPalette}
                            className='border-academy-rule text-academy-ink-faint hover:text-academy-ink rounded-academy-sm flex min-h-11 items-center gap-2 border px-3 py-2 font-mono text-[11px]'
                        >
                            Go to…
                            <kbd className='text-[10px]' aria-label='Command K'>
                                ⌘K
                            </kbd>
                        </button>
                    }
                />
            }
            palette={
                <CommandPalette
                    open={props.paletteOpen}
                    features={snapshot.features}
                    onSelect={props.onNavigate}
                    onClose={props.onClosePalette}
                />
            }
        >
            {feature.id === 'settings' ? (
                <SettingsView
                    ref={heading}
                    snapshot={snapshot}
                    pending={props.appearancePending}
                    onSetAppearance={props.onSetAppearance}
                    dispatch={props.dispatch}
                    observeManagement={observeManagement}
                />
            ) : feature.id === 'administration' && feature.availability.state === 'available' ? (
                <AdministrationView
                    // A new tenant or management credential remounts the view,
                    // so no render ever pairs it with the previous one's rows.
                    key={snapshot.active?.context ?? 'none'}
                    ref={heading}
                    snapshot={snapshot}
                    dispatch={props.dispatch}
                    observe={observeManagement}
                    onNavigate={props.onNavigate}
                />
            ) : feature.id === 'academy' && feature.availability.state === 'available' ? (
                <AcademyView ref={heading} snapshot={snapshot} client={props.client} onNavigate={props.onNavigate} />
            ) : feature.id === 'runtime' && feature.availability.state === 'available' ? (
                <RuntimeView ref={heading} snapshot={snapshot} dispatch={props.dispatch} onNavigate={props.onNavigate} />
            ) : (
                <UnavailableView ref={heading} feature={feature} />
            )}
            {/* At 900 CSS px and wider the shared frame pads the page bottom by
             * only 32px under its 60px fade; this space lets a page's last
             * control scroll clear of the fade (C08-N11). */}
            <p role='status' aria-live='polite' data-slot='shell-status' className='text-academy-ink-soft mt-4 pb-14 text-[13px]'>
                {props.status}
            </p>
        </ShellFrame>
    )
}
