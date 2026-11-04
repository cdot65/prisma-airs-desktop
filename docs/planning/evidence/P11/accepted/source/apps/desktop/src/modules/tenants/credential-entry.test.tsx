/**
 * Production scan-key entry through the real App, DesktopClient and shell:
 * fresh capture at submission, transient handling, storage-mode choice and
 * truthful outcome reporting.
 */
import { act, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DesktopClient } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant, framed } from '../../test/native'

const KEY = 'synthetic-scan-key-0042'

async function openSettings(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    // C44: one presentation; the core names the destination in the snapshot.
    await native.push({ ...native.current, onboarding: 'done', activeFeature: 'settings' })
    return user
}

const keyField = () => screen.getByLabelText('AIRS API key') as HTMLInputElement

let setItem: ReturnType<typeof vi.spyOn>
beforeEach(() => {
    setItem = vi.spyOn(Storage.prototype, 'setItem')
})
afterEach(() => setItem.mockRestore())

describe('scan-key entry', () => {
    it('captures the key typed at submission time and clears the field before awaiting', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        let admit: (value: unknown) => void = () => undefined
        native.respond = () => new Promise((resolve) => (admit = resolve))
        const user = await openSettings(native)
        await user.type(keyField(), 'first-draft')
        await user.clear(keyField())
        await user.type(keyField(), KEY)
        await user.click(screen.getByRole('button', { name: 'Save key' }))
        expect(keyField().value).toBe('')
        expect(native.dispatched).toEqual([
            {
                kind: 'configureScanCredential',
                context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa',
                mode: 'keychain',
                key: KEY,
            },
        ])
        await act(async () => admit({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation: null } }))
        expect(screen.queryByRole('alert')).toBeNull()
    })

    it('keeps the key out of snapshots, observations, the page and browser storage', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        const user = await openSettings(native)
        await user.type(keyField(), KEY)
        await user.click(screen.getByRole('button', { name: 'Save key' }))
        await native.push(
            withTenant(baseSnapshot, tenant({ scan: { state: 'ready', mode: 'keychain', failure: null } }), 'cx-bbbbbbbbbbbbbbbbbbbbbbbb')
        )
        expect(screen.getByText('Scan key saved in secure storage.')).toBeInTheDocument()
        const withKey = native.sent.filter((body) => body.includes(KEY))
        expect(withKey).toHaveLength(1)
        expect(JSON.parse(withKey[0]!).intent.kind).toBe('configureScanCredential')
        expect(native.delivered.some((update) => update.includes(KEY))).toBe(false)
        expect(document.body.innerHTML).not.toContain(KEY)
        expect(setItem).not.toHaveBeenCalled()
    })

    it('sends the session-only choice when selected', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        const user = await openSettings(native)
        const scanKey = document.querySelector<HTMLElement>('[data-slot="scan-key"]') as HTMLElement
        await user.click(within(scanKey).getByRole('radio', { name: /For this session only/ }))
        await user.type(keyField(), KEY)
        await user.click(screen.getByRole('button', { name: 'Save key' }))
        expect(native.dispatched.at(-1)).toMatchObject({ kind: 'configureScanCredential', mode: 'session' })
    })

    it('reports a refusal without claiming the key was saved', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        native.respond = () => ({ outcome: 'refusal', refusal: 'staleContext' })
        const user = await openSettings(native)
        await user.type(keyField(), KEY)
        await user.click(screen.getByRole('button', { name: 'Save key' }))
        expect(await screen.findByRole('alert')).toHaveTextContent(/active tenant changed/i)
        expect(keyField().value).toBe('')
        expect(screen.getByText(/No scan key/)).toBeInTheDocument()
    })

    it('reports an unconfirmed delivery as unknown', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        native.respond = () => Promise.reject(new Error('lost'))
        const user = await openSettings(native)
        await user.type(keyField(), KEY)
        await user.click(screen.getByRole('button', { name: 'Save key' }))
        expect(await screen.findByRole('alert')).toHaveTextContent(/did not confirm/i)
        const configure = native.dispatched.filter((i) => i.kind === 'configureScanCredential')
        expect(configure.length).toBeGreaterThan(0)
        expect(new Set(native.sent.filter((b) => b.includes(KEY)))).toHaveProperty('size', 1)
    })

    it('refuses an empty key locally and focuses the field', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        const user = await openSettings(native)
        await user.click(screen.getByRole('button', { name: 'Save key' }))
        expect(await screen.findByRole('alert')).toHaveTextContent('Enter the scan key.')
        expect(keyField()).toHaveFocus()
        expect(native.dispatched).toEqual([])
    })

    it('shows locked and denied failures with explicit recovery, never a fallback', async () => {
        const native = framed(scriptedNative(
            withTenant(baseSnapshot, tenant({ scan: { state: 'failed', mode: 'keychain', failure: 'locked' } }))
        ), 'settings')
        await openSettings(native)
        expect(screen.getByText(/Secure storage is locked/)).toBeInTheDocument()
        await native.push(withTenant(baseSnapshot, tenant({ scan: { state: 'failed', mode: 'keychain', failure: 'denied' } })))
        expect(screen.getByText(/did not allow access/)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Replace key' })).toBeInTheDocument()
    })

    it('removes the scan key under the current context', async () => {
        const native = framed(scriptedNative(
            withTenant(baseSnapshot, tenant({ scan: { state: 'ready', mode: 'session', failure: null } }))
        ), 'settings')
        const user = await openSettings(native)
        expect(screen.getByText('Scan key held for this session only.')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Remove scan key' }))
        expect(native.dispatched).toEqual([{ kind: 'removeScanCredential', context: 'cx-aaaaaaaaaaaaaaaaaaaaaaaa' }])
    })

    it('discards a partially typed key when the context changes', async () => {
        const native = framed(scriptedNative(withTenant(baseSnapshot, tenant())), 'settings')
        const user = await openSettings(native)
        await user.type(keyField(), 'partial')
        await native.push(withTenant(baseSnapshot, tenant(), 'cx-cccccccccccccccccccccccc'))
        expect(keyField().value).toBe('')
        expect(native.dispatched).toEqual([])
    })
})
