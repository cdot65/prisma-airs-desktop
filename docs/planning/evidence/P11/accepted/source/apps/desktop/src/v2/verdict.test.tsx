import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { Verdict } from './verdict'

describe('verdict (I9)', () => {
    it('is colour plus icon plus word, with the word as the accessible text', () => {
        render(<Verdict tone='blocked' word='Blocked' reason='Prompt injection · high confidence' />)
        const verdict = screen.getByText('Blocked').closest('[data-slot="verdict"]') as HTMLElement
        expect(verdict).toHaveAttribute('data-tone', 'blocked')
        expect(verdict.style.color).toBe('var(--v2-block)')
        expect(verdict.querySelector('svg')).toHaveAttribute('aria-hidden', 'true')
        expect(verdict).toHaveTextContent('Blocked · Prompt injection · high confidence')
    })
    it.each([['flagged', '--v2-flag'], ['allowed', '--v2-ok'], ['info', '--v2-soft']] as const)('%s uses its own token', (tone, token) => {
        render(<Verdict tone={tone} word={tone} />)
        const verdict = screen.getByText(tone).closest('[data-slot="verdict"]') as HTMLElement
        expect(verdict.style.color).toBe(`var(${token})`)
    })
})
