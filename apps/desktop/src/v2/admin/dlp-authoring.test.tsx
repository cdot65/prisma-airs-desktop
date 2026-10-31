/**
 * DLP authoring on the one presentation (C44): the C10 suite's assertions,
 * reached as the v2 shell reaches them (Administration, the Runtime card's
 * DLP tab and its inventory radios; the review sheet hosts the change
 * workbench). v1's actions per inventory, the dictionary draft from typed
 * keywords, a chosen file by its token only, the whole-list replace on edit,
 * typed keywords kept when a choice is cancelled or fails, and metadata that
 * must be one JSON object.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type Intent, type ManagementResource, type ManagementResult, type ShellSnapshot, type StagedFileView } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { type Native, scriptedNative, tenant } from '../../test/native'
import { fxSwitcher } from '../fixtures'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const ITEMS: Partial<Record<ManagementResource, Record<string, unknown>[]>> = {
    dataPatterns: [{ id: 'pattern-1', name: 'Card numbers', type: 'custom' }],
    dataProfiles: [{ id: 'dp-1', name: 'PCI', detection_rules: [] }],
    filteringProfiles: [{ id: 'filter-1', name: 'Default', file_based: true, non_file_based: false }],
    dictionaries: [{ id: 'dict-1', name: 'Codes', original_file_name: 'codes.txt', category: 'Confidential', region_name: 'United States' }],
    deploymentProfiles: [{ dp_name: 'dp-0', status: 'active' }],
}

/** Administration's tenant with a ready management client (the v2 switcher fixture carries the product cards and their tabs). */
const ready = (staged: StagedFileView | null = null): ShellSnapshot => {
    const base = fxSwitcher()
    const nw = tenant({
        id: 'tn-0123456789abcdef',
        label: 'Northwind Health',
        tsgId: '1762204418',
        scan: { state: 'ready', mode: 'keychain', failure: null },
        management: {
            state: 'ready',
            mode: 'keychain',
            failure: null,
            clientId: 'svc@1762204418.iam.panserviceaccount.com',
            tokenOrigin: 'https://auth.apps.paloaltonetworks.com',
        },
    })
    return { ...base, tenants: [nw, base.tenants[1]!], active: { tenant: nw.id, context: CONTEXT }, activeFeature: 'administration', changes: { review: null, staged, results: [] } }
}

function service(native: Native) {
    const requests = new Map<string, Extract<Intent, { kind: 'management' }>['request']>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind === 'management') {
            const operation = `op-${requests.size + 1}`
            requests.set(operation, intent.request)
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${requests.size}`, operation } }
        }
        return respond(intent)
    }
    native.observeManagement = (operation): { kind: 'management'; result: ManagementResult } => {
        const request = requests.get(operation)
        if (request?.kind === 'page') {
            const items = ITEMS[request.resource] ?? []
            return {
                kind: 'management',
                result: {
                    kind: 'page',
                    page: { resource: request.resource, offset: 0, limit: 25, items: JSON.stringify(items), count: items.length, nextOffset: null, complete: true, total: null },
                },
            }
        }
        const detail = request?.kind === 'detail' ? request : null
        const item = (ITEMS[detail?.resource ?? 'dataPatterns'] ?? []).find((i) => i.id === detail?.id) ?? {}
        return {
            kind: 'management',
            result: { kind: 'detail', detail: { resource: detail?.resource ?? 'dataPatterns', id: detail?.id ?? '', found: 'found', item: JSON.stringify(item) } },
        }
    }
}

const DLP_RESOURCES = ['Data patterns', 'Data profiles', 'Data filtering profiles', 'Dictionaries']

/** Administration as the v2 shell reaches it: the Runtime card (open first) with its DLP tab and the inventory chosen in it. */
async function openInventory(native: Native, label: string) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1, name: 'Administration' })
    await chooseInventory(user, label)
    return user
}

async function chooseInventory(user: ReturnType<typeof userEvent.setup>, label: string) {
    if (DLP_RESOURCES.includes(label)) {
        await user.click(await screen.findByRole('tab', { name: 'DLP' }))
        await user.click(await within(await screen.findByRole('radiogroup', { name: 'DLP inventory' })).findByRole('radio', { name: label }))
    } else {
        await user.click(await screen.findByRole('tab', { name: label }))
    }
}

const sent = (native: Native, kind: Intent['kind']) => native.dispatched.filter((i) => i.kind === kind)

describe('DLP authoring', () => {
    it('offers exactly v1’s actions per inventory', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openInventory(native, 'Data patterns')
        expect(screen.getByRole('button', { name: /^New data pattern/ })).toBeInTheDocument()
        await user.click(await screen.findByRole('button', { name: 'Details for Card numbers' }))
        expect(await screen.findByRole('button', { name: 'Edit…' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Delete…' })).toBeInTheDocument()
        // Data profiles: create and edit, never delete.
        await chooseInventory(user, 'Data profiles')
        expect(await screen.findByRole('button', { name: /^New data profile/ })).toBeInTheDocument()
        await user.click(await screen.findByRole('button', { name: 'Details for PCI' }))
        expect(await screen.findByRole('button', { name: 'Edit…' })).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Delete…' })).toBeNull()
        // Filtering profiles: edit only.
        await chooseInventory(user, 'Data filtering profiles')
        expect(screen.queryByRole('button', { name: /^New / })).toBeNull()
        await user.click(await screen.findByRole('button', { name: 'Details for Default' }))
        expect(await screen.findByRole('button', { name: 'Edit…' })).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Delete…' })).toBeNull()
        // Deployment profiles stay read-only.
        await chooseInventory(user, 'Deployment profiles')
        expect(await screen.findByRole('tab', { name: 'Deployment profiles' })).toHaveAttribute('aria-selected', 'true')
        expect(screen.queryByRole('button', { name: /^New / })).toBeNull()
        // And say so (the legacy Administration's read-only note, kept on the one presentation).
        expect(screen.getByText('Read only. Deployment profiles are used when creating API keys; they are not changed here.')).toBeInTheDocument()
    })

    it('builds a dictionary draft from the metadata and typed keywords when Review is pressed', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openInventory(native, 'Dictionaries')
        await user.click(screen.getByRole('button', { name: /^New dictionary/ }))
        const metadata = screen.getByLabelText('dictionary metadata (JSON)') as HTMLTextAreaElement
        const keywords = screen.getByLabelText('Dictionary keywords, one per line') as HTMLTextAreaElement
        expect(JSON.parse(metadata.value)).toMatchObject({ name: 'New dictionary', original_file_name: 'keywords.txt' })
        await user.clear(keywords)
        await user.type(keywords, 'alpha{Enter}beta')
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        const prepare = sent(native, 'reviewChange')[0] as Extract<Intent, { kind: 'reviewChange' }>
        expect(prepare.draft.kind).toBe('dictionaries')
        expect(JSON.parse(prepare.draft.body)).toEqual({ metadata: JSON.parse(metadata.value), file: 'alpha\nbeta' })
    })

    it('references a file chosen in the native sheet by its token only', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openInventory(native, 'Dictionaries')
        await user.click(screen.getByRole('button', { name: /^New dictionary/ }))
        const useFile = screen.getByRole('radio', { name: /Use the chosen file/ })
        expect(useFile).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Choose keyword file…' }))
        expect(sent(native, 'chooseDictionaryFile')).toEqual([{ kind: 'chooseDictionaryFile', context: CONTEXT }])
        await native.push(ready({ state: 'failed', token: null, name: null, bytes: null, lines: null, failure: 'notText' }))
        expect(screen.getByText('That file is not UTF-8 text.')).toBeInTheDocument()
        await native.push(ready({ state: 'ready', token: 'sf-0011223344556677', name: 'keywords.txt', bytes: 17, lines: 3, failure: null }))
        expect(screen.getByText(/Chosen: keywords\.txt \(17 bytes\)/)).toBeInTheDocument()
        await user.click(screen.getByRole('radio', { name: /Use the chosen file \(keywords\.txt, 3 lines\)/ }))
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        const prepare = sent(native, 'reviewChange')[0] as Extract<Intent, { kind: 'reviewChange' }>
        const body = JSON.parse(prepare.draft.body) as Record<string, unknown>
        expect(body.fileRef).toBe('sf-0011223344556677')
        expect(body).not.toHaveProperty('file')
        expect(native.sent.join('\n')).not.toMatch(/\/Users\/|\/private\//)
    })

    it('re-sends a dictionary’s whole keyword list when editing it (v1 replace)', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openInventory(native, 'Dictionaries')
        await user.click(await screen.findByRole('button', { name: 'Details for Codes' }))
        await user.click(await screen.findByRole('button', { name: 'Edit…' }))
        const metadata = JSON.parse((screen.getByLabelText('dictionary metadata (JSON)') as HTMLTextAreaElement).value) as Record<string, unknown>
        expect(metadata).toEqual({ name: 'Codes', original_file_name: 'codes.txt', category: 'Confidential', region_name: 'United States' })
        expect((screen.getByLabelText('Dictionary keywords, one per line') as HTMLTextAreaElement).value).toBe('')
    })

    it('keeps typed keywords in use when a file choice is cancelled or fails', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openInventory(native, 'Dictionaries')
        await user.click(screen.getByRole('button', { name: /^New dictionary/ }))
        const keywords = screen.getByLabelText('Dictionary keywords, one per line') as HTMLTextAreaElement
        await user.click(screen.getByRole('button', { name: 'Choose keyword file…' }))
        await native.push(ready({ state: 'choosing', token: null, name: null, bytes: null, lines: null, failure: null }))
        // The sheet was cancelled: nothing staged.
        await native.push(ready(null))
        expect(screen.getByRole('radio', { name: /Type them/ })).toBeChecked()
        expect(screen.getByRole('radio', { name: /Use the chosen file/ })).not.toBeChecked()
        expect(keywords).toBeEnabled()
        await user.clear(keywords)
        await user.type(keywords, 'typed')
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        const prepare = sent(native, 'reviewChange')[0] as Extract<Intent, { kind: 'reviewChange' }>
        expect(JSON.parse(prepare.draft.body)).toMatchObject({ file: 'typed' })
        // A failed choice likewise leaves typing in use.
        await user.click(screen.getByRole('button', { name: 'Choose keyword file…' }))
        await native.push(ready({ state: 'failed', token: null, name: null, bytes: null, lines: null, failure: 'tooLarge' }))
        expect(keywords).toBeEnabled()
        expect(screen.getByRole('radio', { name: /Type them/ })).toBeChecked()
    })

    it('refuses metadata that is not exactly one JSON object without dispatching', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openInventory(native, 'Dictionaries')
        await user.click(screen.getByRole('button', { name: /^New dictionary/ }))
        const metadata = screen.getByLabelText('dictionary metadata (JSON)') as HTMLTextAreaElement
        await user.clear(metadata)
        await user.click(metadata)
        // A valid object followed by a top-level fragment must not be spliced.
        await user.paste('{"name":"N","original_file_name":"a.txt","category":"Confidential","region_name":"United States"},"file":"injected"')
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        expect(screen.getByText('The dictionary metadata must be one JSON object.')).toBeInTheDocument()
        expect(sent(native, 'reviewChange')).toEqual([])
    })
})
