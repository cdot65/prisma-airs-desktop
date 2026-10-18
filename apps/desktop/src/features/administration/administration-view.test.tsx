/**
 * Production Administration inventories through the real App, DesktopClient
 * and shell: explicit page reads with the active context, later pages,
 * partial inventories, refresh denial with stale rows, credentials replaced
 * mid-pagination, unknown values rendered as text, detail found/absent/
 * incomplete and the scan-only tenant without a management client.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    DesktopClient,
    type Intent,
    type ManagementRequest,
    type ManagementResult,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const REPLACED = 'cx-bbbbbbbbbbbbbbbbbbbbbbbb'
const SECRET = 'synthetic-management-secret-0042'
const ready = (context = CONTEXT): ShellSnapshot =>
    withTenant(
        baseSnapshot,
        tenant({
            scan: { state: 'ready', mode: 'keychain', failure: null },
            management: {
                state: 'ready',
                mode: 'keychain',
                failure: null,
                clientId: 'svc@1234567890.iam.panserviceaccount.com',
                tokenOrigin: 'https://auth.apps.paloaltonetworks.com',
            },
        }),
        context
    )

type Read = { context: string; request: ManagementRequest }
type Answer = (read: Read) => ManagementResult | Promise<ManagementResult>

/** Admits each management intent and answers its observation via `answer`. */
function management(native: Native, answer: Answer): Read[] {
    const reads: Read[] = []
    const pending = new Map<string, Read>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind !== 'management') return respond(intent)
        const read = { context: intent.context, request: intent.request }
        reads.push(read)
        const operation = `op-${reads.length}`
        pending.set(operation, read)
        return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${reads.length}`, operation } }
    }
    native.observeManagement = async (operation) => {
        const read = pending.get(operation)
        if (!read) return { kind: 'refusal', refusal: 'invalidInput' }
        return { kind: 'management', result: await answer(read) }
    }
    return reads
}

const profiles = (from: number, count: number) =>
    Array.from({ length: count }, (_, i) => ({ profile_id: `p-${from + i}`, profile_name: `Profile ${from + i}` }))

function page(request: ManagementRequest, items: unknown[], more: boolean, total: number | null = null): ManagementResult {
    if (request.kind !== 'page') throw new Error('not a page read')
    return {
        kind: 'page',
        page: {
            resource: request.resource,
            offset: request.offset,
            limit: request.limit,
            items: JSON.stringify(items),
            count: items.length,
            nextOffset: more ? request.offset + request.limit : null,
            complete: !more,
            total,
        },
    }
}

async function openAdministration(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    const nav = await screen.findByRole('navigation', { name: 'Features' })
    await user.click(within(nav).getByRole('button', { name: /^Administration/ }))
    await screen.findByRole('heading', { level: 1, name: 'Tenant inventories' })
    return user
}

const rows = () => screen.queryAllByRole('row').slice(1).map((row) => within(row).getAllByRole('cell')[0]?.textContent)
const progress = () => document.querySelector('[data-slot="administration-progress"]')?.textContent ?? ''

describe('administration inventories', () => {
    it('keeps a scan-only tenant usable and reads nothing without a management client', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant({ scan: { state: 'ready', mode: 'keychain', failure: null } })))
        const reads = management(native, () => ({ kind: 'verified' }))
        const user = await openAdministration(native)
        expect(screen.getByText(/has no ready management client/)).toBeInTheDocument()
        expect(reads).toEqual([])
        // Runtime still scans with the scan key alone.
        await user.click(within(screen.getByRole('navigation', { name: 'Features' })).getByRole('button', { name: /Runtime Security/ }))
        expect(screen.queryByText(/has no ready scan key/)).toBeNull()
        expect(native.dispatched.some((i) => i.kind === 'management')).toBe(false)
    })

    it('reads later pages, reports partial inventories and claims completeness only from the service', async () => {
        const native = scriptedNative(ready())
        const reads = management(native, ({ request }) =>
            request.kind === 'page' && request.offset === 0 ? page(request, profiles(0, 25), true) : page(request, profiles(25, 3), false)
        )
        const user = await openAdministration(native)
        await waitFor(() => expect(rows()).toHaveLength(25))
        expect(reads[0]).toEqual({ context: CONTEXT, request: { kind: 'page', resource: 'profiles', offset: 0, limit: 25 } })
        expect(progress()).toMatch(/Items 1–25 of 25\+/)
        expect(screen.getByText(/Rows not yet read are not shown and are not assumed absent/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Next page' }))
        await waitFor(() => expect(rows()).toEqual(['Profile 25', 'Profile 26', 'Profile 27']))
        expect(reads[1]?.request).toEqual({ kind: 'page', resource: 'profiles', offset: 25, limit: 25 })
        expect(progress()).toMatch(/Items 26–28 of 28/)
        expect(screen.getByRole('button', { name: 'Next page' })).toBeDisabled()
        expect(screen.queryByText(/Rows not yet read/)).toBeNull()
        await user.click(screen.getByRole('button', { name: 'First page' }))
        await waitFor(() => expect(rows()).toHaveLength(25))
        expect(reads[2]?.request).toMatchObject({ offset: 0 })
    })

    it('restarts from the first page when the credential is replaced mid-pagination', async () => {
        const native = scriptedNative(ready())
        let release: () => void = () => undefined
        const reads = management(native, async ({ context, request }) => {
            if (request.kind === 'page' && request.offset === 25) {
                // The native side cancels a read whose context retired.
                await new Promise<void>((resolve) => (release = resolve))
                return { kind: 'failed', failure: 'cancelled', outcome: 'unknown' }
            }
            return page(request, profiles(context === REPLACED ? 100 : 0, 25), true)
        })
        const user = await openAdministration(native)
        await waitFor(() => expect(rows()).toHaveLength(25))
        await user.click(screen.getByRole('button', { name: 'Next page' }))
        await waitFor(() => expect(reads).toHaveLength(2))
        await native.push(ready(REPLACED))
        await waitFor(() => expect(rows()[0]).toBe('Profile 100'))
        expect(reads[2]).toEqual({ context: REPLACED, request: { kind: 'page', resource: 'profiles', offset: 0, limit: 25 } })
        release()
        await new Promise((resolve) => setTimeout(resolve, 0))
        // The late cancelled reply for the retired credential is dropped.
        expect(screen.queryByRole('alert')).toBeNull()
        expect(rows()[0]).toBe('Profile 100')
    })

    it('keeps rows with their age when a refresh is denied', async () => {
        const native = scriptedNative(ready())
        let denied = false
        management(native, ({ request }) =>
            denied ? { kind: 'failed', failure: 'denied', outcome: 'rejected' } : page(request, profiles(0, 2), false)
        )
        const user = await openAdministration(native)
        await waitFor(() => expect(rows()).toHaveLength(2))
        denied = true
        await user.click(screen.getByRole('button', { name: 'Refresh' }))
        const alert = await screen.findByRole('alert')
        expect(alert).toHaveTextContent('not allowed to read this')
        expect(alert).toHaveTextContent(/Showing rows read at .* They may be out of date/)
        expect(rows()).toEqual(['Profile 0', 'Profile 1'])
    })

    it('shows the service’s empty inventory, and a failed first read is not empty', async () => {
        const native = scriptedNative(ready())
        management(native, ({ request }) =>
            request.kind === 'page' && request.resource === 'topics'
                ? page(request, [], false, 0)
                : { kind: 'failed', failure: 'unavailable', outcome: 'rejected' }
        )
        const user = await openAdministration(native)
        expect(await screen.findByRole('alert')).toHaveTextContent('unavailable')
        expect(screen.queryByText(/reports no/)).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Custom topics' }))
        expect(screen.getByRole('button', { name: 'Custom topics' })).toHaveAttribute('aria-current', 'true')
        expect(await screen.findByText('The service reports no custom topics in this tenant.')).toBeInTheDocument()
        expect(screen.queryByRole('alert')).toBeNull()
    })

    it('renders unknown values and markup as text and filters only the current page', async () => {
        const native = scriptedNative(ready())
        const items = [
            { profile_id: 'p-1', profile_name: '<img src=x onerror=alert(1)>', future_mode: 'quantum' },
            { profile_id: 'p-2', profile_name: 'Guardrails' },
        ]
        management(native, ({ request }) =>
            request.kind === 'page'
                ? page(request, items, false)
                : { kind: 'detail', detail: { resource: 'profiles', id: 'p-1', found: 'found', item: JSON.stringify(items[0]) } }
        )
        const user = await openAdministration(native)
        await waitFor(() => expect(rows()).toHaveLength(2))
        expect(document.querySelector('img')).toBeNull()
        expect(rows()[0]).toBe('<img src=x onerror=alert(1)>')
        await user.type(screen.getByLabelText('Filter this page'), 'guard')
        expect(rows()).toEqual(['Guardrails'])
        await user.clear(screen.getByLabelText('Filter this page'))
        await user.click(screen.getByRole('button', { name: 'Details for <img src=x onerror=alert(1)>' }))
        const panel = await waitFor(() => {
            const el = document.querySelector<HTMLElement>('[data-slot="detail-fields"]')
            if (!el) throw new Error('no detail')
            return el
        })
        expect(panel).toHaveTextContent('future_mode')
        expect(panel).toHaveTextContent('quantum')
        expect(document.querySelector('img')).toBeNull()
        expect(screen.getByText(/local previews in the previous desktop/)).toBeInTheDocument()
    })

    it('says when a detail is absent from the complete inventory or beyond the bounded walk', async () => {
        const native = scriptedNative(ready())
        const reads = management(native, ({ request }) => {
            if (request.kind === 'page') return page(request, profiles(0, 2), false)
            if (request.kind !== 'detail') throw new Error('unexpected')
            return {
                kind: 'detail',
                detail: { resource: 'profiles', id: request.id, found: request.id === 'p-0' ? 'absent' : 'incomplete', item: null },
            }
        })
        const user = await openAdministration(native)
        await waitFor(() => expect(rows()).toHaveLength(2))
        await user.click(screen.getByRole('button', { name: 'Details for Profile 0' }))
        expect(await screen.findByText(/complete inventory no longer contains this item/)).toBeInTheDocument()
        expect(reads.at(-1)).toEqual({ context: CONTEXT, request: { kind: 'detail', resource: 'profiles', id: 'p-0' } })
        await user.click(screen.getByRole('button', { name: 'Details for Profile 1' }))
        expect(await screen.findByText(/this does not mean it is absent/)).toBeInTheDocument()
    })

    it('reports a refused read without showing data and never sends secret material', async () => {
        const native = scriptedNative(ready())
        const respond = native.respond
        native.respond = (intent) =>
            intent.kind === 'management' ? { outcome: 'refusal', refusal: 'capabilityMissing' } : respond(intent)
        await openAdministration(native)
        expect(await screen.findByRole('alert')).toHaveTextContent('That is not available for the current tenant.')
        expect(rows()).toEqual([])
        expect(native.sent.join('\n')).not.toContain(SECRET)
    })
})
