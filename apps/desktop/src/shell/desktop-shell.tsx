import { useEffect, useRef } from 'react'
import { ShellBrand, ShellFrame, ShellRail, ShellTopbar, type ShellNavItem } from '@ai-security-academy/presentation'
import type { FeatureEntry, FeatureId, ShellSnapshot, Theme } from '@prisma-airs/desktop-contracts'
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
 * gamification, router or hosted provider is mounted here.
 */
export function DesktopShell(props: DesktopShellProps) {
    const { snapshot, active } = props
    const heading = useRef<HTMLHeadingElement>(null)
    const firstRender = useRef(true)
    const feature = snapshot.features.find((f) => f.id === active) ?? snapshot.features[0]

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
            if (event.target instanceof Element) event.target.scrollIntoView({ block: 'nearest' })
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
                    appearance={snapshot.appearance}
                    pending={props.appearancePending}
                    onSetAppearance={props.onSetAppearance}
                />
            ) : (
                <UnavailableView ref={heading} feature={feature} />
            )}
            <p role='status' aria-live='polite' data-slot='shell-status' className='text-academy-ink-soft mt-4 text-[13px]'>
                {props.status}
            </p>
        </ShellFrame>
    )
}
