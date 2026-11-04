import { describe, expect, it } from 'vitest'
import tokens from '../styles/tokens.css?raw'

function declarations(selector: string): Record<string, string> {
    const start = tokens.indexOf(`${selector} {`)
    if (start < 0) throw new Error(`Missing palette: ${selector}`)
    const body = tokens.slice(start, tokens.indexOf('}', start))
    return Object.fromEntries([...body.matchAll(/--v2-([\w-]+): (#[\da-f]{6});/g)].map((m) => [m[1]!, m[2]!]))
}

const luminance = (hex: string) => {
    const values = [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16) / 255).map((c) => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
    return values[0]! * 0.2126 + values[1]! * 0.7152 + values[2]! * 0.0722
}
const contrast = (a: string, b: string) => {
    const [low, high] = [luminance(a), luminance(b)].sort((x, y) => x - y)
    return (high! + 0.05) / (low! + 0.05)
}

describe.each(['prismaAirs', 'gruvbox'])('%s palette contrast', (family) => {
    it.each(['light', 'dark'])('%s retains readable text and visible controls', (appearance) => {
        const palette = {
            ...declarations(':root'),
            ...(appearance === 'dark' ? declarations(":root[data-appearance='dark']") : {}),
            ...(family === 'gruvbox' ? declarations(`:root[data-color-theme='gruvbox'][data-appearance='${appearance}']`) : {}),
        }
        for (const surface of ['bg', 'paper', 'inset']) {
            for (const ink of ['ink', 'soft', 'faint']) expect(contrast(palette[ink]!, palette[surface]!), `${ink} on ${surface}`).toBeGreaterThanOrEqual(4.5)
            for (const control of ['ctl', 'acc']) expect(contrast(palette[control]!, palette[surface]!), `${control} on ${surface}`).toBeGreaterThanOrEqual(3)
        }
        expect(contrast(palette['acc-ink']!, palette.acc!)).toBeGreaterThanOrEqual(4.5)
        for (const status of ['block', 'flag', 'ok']) expect(contrast(palette[status]!, palette[`${status}-bg`]!)).toBeGreaterThanOrEqual(4.5)
    })
})
