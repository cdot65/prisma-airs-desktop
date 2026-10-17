/**
 * Production profile import through the real App, DesktopClient and shell:
 * the native sheet is requested (no renderer file access), the native
 * preview is shown faithfully, and only the operator's explicit selection is
 * sent.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type ImportView, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const REVIEW = 'rv-0011223344556677'

const preview: ImportView = {
    state: 'review',
    review: REVIEW,
    format: 'airsCliRegistry',
    fileName: 'tenants.json',
    candidates: [
        {
            index: 0,
            label: 'prod <b>eu</b>',
            tsgId: '1111111111',
            destination: { kind: 'region', region: 'unitedStates' },
            defaultDestination: true,
            problem: null,
            existingLabel: false,
        },
        {
            index: 1,
            label: 'Prod EU',
            tsgId: '2222',
            destination: { kind: 'region', region: 'unitedStates' },
            defaultDestination: true,
            problem: null,
            existingLabel: true,
        },
        {
            index: 2,
            label: '',
            tsgId: null,
            destination: null,
            defaultDestination: true,
            problem: 'invalidLabel',
            existingLabel: false,
        },
    ],
    discarded: { secretFields: 1, fileReferences: 3, otherFields: 2 },
}

const start = (): ShellSnapshot => withTenant(baseSnapshot, tenant())

async function openSettings(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    const nav = await screen.findByRole('navigation', { name: 'Features' })
    await user.click(within(nav).getByRole('button', { name: /Settings/ }))
    return user
}

const importSection = () => screen.getByRole('region', { name: 'Import tenant profiles' })

describe('profile import', () => {
    it('asks the desktop for its native sheet and never reads files in the page', async () => {
        const native = scriptedNative(start())
        const user = await openSettings(native)
        expect(document.querySelector('input[type="file"]')).toBeNull()
        await user.click(within(importSection()).getByRole('button', { name: 'Choose profile file…' }))
        expect(native.dispatched).toEqual([{ kind: 'chooseProfileImport' }])
        await native.push({ ...start(), import: { state: 'choosing' } })
        expect(within(importSection()).getByRole('status')).toHaveTextContent('Choose a file in the open dialog.')
    })

    it('shows the native preview faithfully and sends only the explicit selection', async () => {
        const native = scriptedNative(start())
        const user = await openSettings(native)
        await native.push({ ...start(), import: preview })
        const section = importSection()
        expect(within(section).getByText('tenants.json')).toBeInTheDocument()
        expect(within(section).getByText(/airs-cli tenant registry/)).toBeInTheDocument()
        expect(section.querySelector('[data-slot="import-discarded"]')).toHaveTextContent(
            'Not imported: 1 secret field(s), 3 file reference(s), 2 other field(s).'
        )
        // Labels are text, never markup.
        expect(within(section).getByText('prod <b>eu</b>')).toBeInTheDocument()
        expect(section.querySelector('b')).toBeNull()
        const boxes = within(section).getAllByRole('checkbox') as HTMLInputElement[]
        expect(boxes.map((b) => b.checked)).toEqual([true, true, false])
        expect(boxes[2]).toBeDisabled()
        expect(within(section).getByText(/cannot import: missing or invalid label/)).toBeInTheDocument()
        expect(within(section).getByText(/a tenant with this label exists/)).toBeInTheDocument()
        expect(within(section).getAllByText(/default; not in the file/)).toHaveLength(2)
        await user.click(boxes[1]!)
        await user.click(within(section).getByRole('button', { name: 'Import 1 profile' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'confirmProfileImport', review: REVIEW, selected: [0] })
    })

    it('discards a preview without importing', async () => {
        const native = scriptedNative(start())
        const user = await openSettings(native)
        await native.push({ ...start(), import: preview })
        await user.click(within(importSection()).getByRole('button', { name: 'Discard' }))
        expect(native.dispatched).toEqual([{ kind: 'discardReview', review: REVIEW }])
    })

    it('cannot submit an empty selection', async () => {
        const native = scriptedNative(start())
        const user = await openSettings(native)
        await native.push({ ...start(), import: preview })
        const section = importSection()
        for (const box of (within(section).getAllByRole('checkbox') as HTMLInputElement[]).filter((b) => b.checked)) {
            await user.click(box)
        }
        expect(within(section).getByRole('button', { name: 'Import 0 profiles' })).toBeDisabled()
        expect(native.dispatched).toEqual([])
    })

    it('reports an unreadable file with nothing imported', async () => {
        const native = scriptedNative(start())
        await openSettings(native)
        await native.push({ ...start(), import: { state: 'failed', error: 'unsupported' } })
        expect(within(importSection()).getByRole('alert')).toHaveTextContent(
            'That file is not a supported tenant-profile format. Nothing was imported.'
        )
    })

    it('shows imported tenants without keys and reports a consumed review', async () => {
        const native = scriptedNative(start())
        native.respond = () => ({ outcome: 'refusal', refusal: 'reviewConsumed' })
        const user = await openSettings(native)
        await native.push({ ...start(), import: preview })
        await user.click(within(importSection()).getByRole('button', { name: 'Import 2 profiles' }))
        expect(await within(importSection()).findByRole('alert')).toHaveTextContent('This review was already used.')
        const imported = tenant({ id: 'tn-00000000000000ff', label: 'prod <b>eu</b>' })
        await native.push({ ...start(), tenants: [tenant(), imported], import: { state: 'idle' } })
        const list = screen.getByRole('list')
        expect(within(list).getAllByRole('listitem')).toHaveLength(2)
        expect(within(list).getByText(/prod <b>eu<\/b>/)).toBeInTheDocument()
        expect(within(list).getAllByText(/scan key missing/)).toHaveLength(2)
    })
})
