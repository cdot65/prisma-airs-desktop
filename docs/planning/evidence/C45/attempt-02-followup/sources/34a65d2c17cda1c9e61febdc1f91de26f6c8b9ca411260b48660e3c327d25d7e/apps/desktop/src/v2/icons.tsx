import type { AdminProductId, FeatureId } from '@prisma-airs/desktop-contracts'

/**
 * The prototype's own icon paths (R2App.dc.html `I`), 24-unit stroked
 * outlines, so the shell's glyphs match the design exactly (C35).
 */
export const PATHS = {
    shield: 'M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z',
    cross: 'M12 3v4M12 17v4M3 12h4M17 12h4M12 5a7 7 0 1 0 0 14 7 7 0 0 0 0-14z',
    box: 'M3 7l9-4 9 4v10l-9 4-9-4zM3 7l9 4 9-4M12 11v10',
    route: 'M4 12h12M12 6l6 6-6 6M4 5v14',
    idcard: 'M3 5h18v14H3zM8 9.5a2 2 0 1 0 0 4 2 2 0 0 0 0-4zM5 17c.6-1.4 1.7-2 3-2s2.4.6 3 2M14 10h4M14 14h4',
    laptop: 'M5 5h14v10H5zM2 19h20',
    home: 'M3 11l9-7 9 7M5 10v10h14V10',
    inbox: 'M3 13h5l1 3h6l1-3h5M5 5h14l2 8v6H3v-6z',
    bot: 'M12 3v3M7 7h10a3 3 0 0 1 3 3v6a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-6a3 3 0 0 1 3-3zM9 12h.01M15 12h.01',
    code: 'M8 6l-5 6 5 6M16 6l5 6-5 6M14 4l-4 16',
    cap: 'M2 9l10-5 10 5-10 5zM6 11v5c3 2.5 9 2.5 12 0v-5',
    cases: 'M9 3h6v4H9zM7 5H5v16h14V5h-2M8 12h8M8 16h5',
    sliders: 'M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1M15 4v4M9 10v4M17 16v4',
    search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-5-5',
    chev: 'M6 9l6 6 6-6',
    chevR: 'M9 6l6 6-6 6',
    lock: 'M6 11h12v10H6zM8 11V8a4 4 0 0 1 8 0v3',
    moon: 'M20 14A8 8 0 1 1 10 4a6 6 0 0 0 10 10z',
    check: 'M5 12l5 5L20 7',
    x: 'M6 6l12 12M18 6L6 18',
    alert: 'M12 3l10 18H2zM12 10v5M12 18h.01',
    ban: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM5.6 5.6l12.8 12.8',
    info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v6M12 7h.01',
    plug: 'M9 2v6M15 2v6M6 8h12v4a6 6 0 0 1-12 0zM12 18v4',
    send: 'M4 12l16-8-6 16-3-7z',
    key: 'M14 10a4 4 0 1 0-1 2.6L20 20M17 17l2-2',
    clock: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 7v5l3 2',
    folder: 'M3 6h6l2 2h10v11H3z',
    eye: 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
    dot: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
    play: 'M7 4l13 8-13 8z',
    sun: 'M12 4v2M12 18v2M4 12h2M18 12h2M6.3 6.3l1.4 1.4M16.3 16.3l1.4 1.4M6.3 17.7l1.4-1.4M16.3 7.7l1.4-1.4M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z',
} as const

export type GlyphName = keyof typeof PATHS

/** The destinations' glyphs, as the prototype's NAV draws them. */
export const FEATURE_GLYPHS: Partial<Record<FeatureId, GlyphName>> = {
    home: 'home',
    inbox: 'inbox',
    harness: 'bot',
    academy: 'cap',
    library: 'cases',
    apiClient: 'code',
    administration: 'sliders',
    runtime: 'shield',
    gateway: 'route',
    redTeam: 'cross',
    sessions: 'clock',
    settings: 'sliders',
}

/** The Administration products' glyphs (the prototype's PRODUCTS). */
export const PRODUCT_GLYPHS: Record<AdminProductId, GlyphName> = {
    runtimeSecurity: 'shield',
    redTeam: 'cross',
    modelSkill: 'box',
    gateway: 'route',
    agenticIdentity: 'idcard',
    agenticEndpoint: 'laptop',
}

export function Glyph({ name, size = 16, stroke = 1.8, color, className }: { name: GlyphName; size?: number; stroke?: number; color?: string; className?: string }) {
    return (
        <svg width={size} height={size} viewBox='0 0 24 24' fill='none' stroke={color ?? 'currentColor'} strokeWidth={stroke} strokeLinecap='round' strokeLinejoin='round' aria-hidden='true' className={className} style={{ flexShrink: 0 }}>
            <path d={PATHS[name]} />
        </svg>
    )
}
