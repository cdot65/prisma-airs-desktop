import { GENERAL_THEMES, applyThemeAttribute, type GeneralThemeId } from '@ai-security-academy/presentation'
import type { Theme } from '@prisma-airs/desktop-contracts'

/** Maps the desktop preference to a general shared theme (never an earned skin). */
export function resolveTheme(theme: Theme, prefersDark: boolean): GeneralThemeId {
    if (theme === 'light') return GENERAL_THEMES.light
    if (theme === 'dark') return GENERAL_THEMES.dark
    return prefersDark ? GENERAL_THEMES.dark : GENERAL_THEMES.light
}

const darkQuery = () =>
    typeof window.matchMedia === 'function'
        ? window.matchMedia('(prefers-color-scheme: dark)')
        : null

/** Applies the preference now and follows OS changes while it is `system`. */
export function followAppearance(theme: Theme): () => void {
    const query = darkQuery()
    const apply = () => applyThemeAttribute(resolveTheme(theme, query?.matches ?? true))
    apply()
    if (theme !== 'system' || !query) return () => undefined
    query.addEventListener('change', apply)
    return () => query.removeEventListener('change', apply)
}
