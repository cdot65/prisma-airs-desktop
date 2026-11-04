import { afterEach, describe, expect, it, vi } from 'vitest'
import { followAppearance } from './appearance'

afterEach(() => {
    vi.unstubAllGlobals()
    delete document.documentElement.dataset.colorTheme
    delete document.documentElement.dataset.appearance
    delete document.documentElement.dataset.theme
})

describe('independent desktop and Academy appearance', () => {
    it('selects the desktop palette without replacing the Academy general theme', () => {
        followAppearance('light', 'prismaAirs')()
        expect(document.documentElement.dataset.theme).toBe('paper')
        expect(document.documentElement.dataset.colorTheme).toBe('prismaAirs')
        expect(document.documentElement.dataset.appearance).toBe('light')
        followAppearance('light', 'gruvbox')()
        expect(document.documentElement.dataset.theme).toBe('paper')
        expect(document.documentElement.dataset.colorTheme).toBe('gruvbox')
    })

    it('follows system appearance while retaining the chosen palette and cleans up', () => {
        let notify = () => undefined
        const remove = vi.fn()
        const query = { matches: false, addEventListener: vi.fn((_name, cb) => { notify = cb }), removeEventListener: remove }
        vi.stubGlobal('matchMedia', () => query)
        const stop = followAppearance('system', 'prismaAirs')
        expect(document.documentElement.dataset.appearance).toBe('light')
        query.matches = true
        notify()
        expect(document.documentElement.dataset.appearance).toBe('dark')
        expect(document.documentElement.dataset.theme).toBe('gruvbox')
        expect(document.documentElement.dataset.colorTheme).toBe('prismaAirs')
        stop()
        expect(remove).toHaveBeenCalledWith('change', notify)
    })
})
