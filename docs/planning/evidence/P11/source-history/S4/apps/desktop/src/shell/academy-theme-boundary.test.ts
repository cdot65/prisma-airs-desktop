import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import tokens from '../styles/tokens.css?raw'

// Inspect the real stylesheet's selector participation. jsdom cannot resolve
// inherited CSS variables or qualify native computed colors and focus pixels.
let stylesheet: HTMLStyleElement
let fixture: HTMLDivElement
let rules: CSSStyleRule[]

beforeEach(() => {
    stylesheet = document.createElement('style')
    stylesheet.textContent = tokens
    document.head.append(stylesheet)
    rules = [...stylesheet.sheet!.cssRules].filter((rule): rule is CSSStyleRule => rule.type === CSSRule.STYLE_RULE)
    fixture = document.createElement('div')
    fixture.innerHTML = `
        <div data-v2-shell>
            <header data-desktop-theme><button id="header">Explain</button></header>
            <nav data-desktop-theme><button id="rail" data-inset-focus aria-current="page">Settings</button></nav>
            <main data-desktop-theme><section class="settings-card">
                <input id="desktop-input"><select id="desktop-select"><option>Light</option></select>
                <textarea id="desktop-textarea"></textarea><button id="desktop-button">Save</button>
            </section></main>
            <main><section data-slot="academy-pages">
                <input id="academy-input"><select id="academy-select"><option>General theme</option></select>
                <textarea id="academy-textarea"></textarea><button id="academy-button">Continue</button>
            </section></main>
        </div>
        <div data-v2-scrim data-desktop-theme><div role="dialog"><input id="dialog-input"><button id="dialog-button">Apply</button></div></div>
        <div data-slot="wizard" data-desktop-theme><input id="wizard-input"><button id="wizard-button">Continue</button></div>
        <div data-academy-portal><input id="academy-portal-input"><button id="academy-portal-button">Close</button></div>`
    document.body.append(fixture)
})

afterEach(() => {
    stylesheet.remove()
    fixture.remove()
})

function desktopRulesFor(id: string, properties: string[]): string[] {
    const control = fixture.querySelector<HTMLElement>(`#${id}`)!
    control.focus()
    return rules.filter((rule) => properties.some((property) => rule.style.getPropertyValue(property).includes('var(--v2-')) && control.matches(rule.selectorText)).map((rule) => rule.selectorText)
}

describe('desktop CSS ownership at the Academy boundary', () => {
    it.each(['academy-input', 'academy-select', 'academy-textarea', 'academy-portal-input'])('leaves %s form colors to Academy', (id) => {
        expect(desktopRulesFor(id, ['background', 'color', 'border'])).toEqual([])
    })

    it.each(['academy-input', 'academy-select', 'academy-textarea', 'academy-button', 'academy-portal-button'])('leaves %s focus treatment to Academy', (id) => {
        expect(desktopRulesFor(id, ['outline', 'outline-color'])).toEqual([])
    })

    it.each(['desktop-input', 'desktop-select', 'desktop-textarea', 'dialog-input', 'wizard-input'])('keeps desktop form colors on %s', (id) => {
        expect(desktopRulesFor(id, ['background', 'color', 'border']).length).toBeGreaterThan(0)
    })

    it.each(['header', 'rail', 'desktop-button', 'dialog-button', 'wizard-button'])('keeps desktop focus treatment on %s', (id) => {
        expect(desktopRulesFor(id, ['outline']).length).toBeGreaterThan(0)
        if (id === 'rail') expect(desktopRulesFor(id, ['outline-color']).length).toBeGreaterThan(0)
    })
})
