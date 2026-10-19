/**
 * The case Library through the real App, DesktopClient and shell: five
 * products authored from fields read when Save is pressed, Runtime cases
 * and starters filling the scan form without scanning, specifications only
 * inspected, explicit import/export/catalog/copy intents, and refusals
 * shown with v1's messages.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { type CaseView, DesktopClient, type Intent, type LibraryView, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'

const RUNTIME: CaseView = {
    id: '11111111-2222-4333-8444-555555555555',
    product: 'runtime-security',
    name: 'Benign question',
    profile: 'prod-default',
    prompt: 'How do I reset my password?',
    response: '',
    appName: 'Desktop examples',
    expected: 'A benign control',
    payload: null,
}
const SPEC: CaseView = {
    id: '66666666-7777-4888-8999-aaaaaaaaaaaa',
    product: 'model-security',
    name: 'Provenance checklist',
    profile: '',
    prompt: 'Check the model artifact provenance',
    response: '',
    appName: '',
    expected: '',
    payload: '{\n  "artifact": "m"\n}',
}

const library = (overrides: Partial<LibraryView> = {}): LibraryView => ({
    state: 'ready',
    failure: null,
    cases: [],
    problem: null,
    unsaved: null,
    notice: null,
    choosing: false,
    fetching: false,
    export: null,
    ...overrides,
})

const snap = (lib: LibraryView = library(), tenants = [tenant()]): ShellSnapshot => ({
    ...withTenant(baseSnapshot, tenants[0] as never, CONTEXT),
    tenants,
    library: lib,
})

async function openLibrary(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Case Library/ }))
    await screen.findByRole('heading', { level: 1, name: 'Test case library' })
    return user
}

const sent = (native: Native, kind: Intent['kind']) => native.dispatched.filter((i) => i.kind === kind)

describe('Case Library', () => {
    it('opens the tenant library and authors cases from the fields read at Save', async () => {
        const native = scriptedNative(snap())
        const user = await openLibrary(native)
        expect(sent(native, 'openLibrary')).toEqual([{ kind: 'openLibrary', context: CONTEXT }])
        expect(screen.getByText(/Runtime Security cases can fill the Playground; other cases are specifications/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Create a case' }))
        const form = screen.getByRole('form', { name: 'Create a case' })
        await user.type(within(form).getByRole('textbox', { name: 'Name' }), 'Benign question')
        await user.type(within(form).getByRole('textbox', { name: 'Security profile' }), 'prod')
        await user.type(within(form).getByRole('textbox', { name: 'Application' }), 'Desk')
        await user.type(within(form).getByRole('textbox', { name: 'Input or test specification' }), 'Hello')
        expect(within(form).getByText('A test goal, never a recorded verdict')).toBeInTheDocument()
        expect(within(form).queryByRole('textbox', { name: /Product payload/ })).toBeNull()
        await user.click(within(form).getByRole('button', { name: 'Save case' }))
        expect(sent(native, 'saveCase')).toEqual([
            {
                kind: 'saveCase',
                context: CONTEXT,
                draft: { product: 'runtime-security', name: 'Benign question', profile: 'prod', prompt: 'Hello', response: '', appName: 'Desk', expected: '', payload: '' },
            },
        ])
        // A specification has a payload and no profile or application.
        await user.click(screen.getByRole('button', { name: 'Create a case' }))
        const spec = screen.getByRole('form', { name: 'Create a case' })
        await user.selectOptions(within(spec).getByRole('combobox', { name: 'Product' }), 'skill-scanning')
        expect(within(spec).queryByRole('textbox', { name: 'Security profile' })).toBeNull()
        await user.type(within(spec).getByRole('textbox', { name: 'Name' }), 'Skill review')
        expect(within(spec).getByText(/It does not execute a scan, campaign, skill, model, or Gateway request/)).toBeInTheDocument()
        await user.click(within(spec).getByRole('button', { name: 'Save case' }))
        expect(sent(native, 'saveCase')[1]).toMatchObject({ draft: { product: 'skill-scanning', profile: '', appName: '', payload: '{}' } })
    })

    it('loads a Runtime case into the scan form without scanning and only inspects specifications', async () => {
        const native = scriptedNative(snap(library({ cases: [RUNTIME, SPEC] })))
        const user = await openLibrary(native)
        expect(screen.queryByRole('button', { name: `Load ${SPEC.name}` })).toBeNull()
        await user.click(screen.getByRole('button', { name: `Inspect ${SPEC.name}` }))
        const detail = screen.getByRole('region', { name: `Details of ${SPEC.name}` })
        expect(within(detail).getByText(/Execution belongs to that product workflow/)).toBeInTheDocument()
        expect(within(detail).getByText(/"artifact": "m"/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: `Load ${RUNTIME.name}` }))
        await screen.findByRole('heading', { level: 1, name: 'Scan a prompt or response' })
        expect(screen.getByRole('textbox', { name: 'Prompt' })).toHaveValue(RUNTIME.prompt)
        expect(screen.getByRole('textbox', { name: 'Profile name or ID' })).toHaveValue('prod-default')
        expect(screen.getByRole('textbox', { name: /Application name/ })).toHaveValue('Desktop examples')
        expect(screen.getByText(/Benign question is loaded\. Goal: A benign control/)).toBeInTheDocument()
        expect(sent(native, 'scan')).toEqual([])
    })

    it('offers v1’s unsaved starters when the library is empty', async () => {
        const native = scriptedNative(snap())
        const user = await openLibrary(native)
        await user.click(screen.getByRole('button', { name: 'Try Sensitive data in a reply in Playground' }))
        await screen.findByRole('heading', { level: 1, name: 'Scan a prompt or response' })
        expect(screen.getByRole('textbox', { name: 'Response' })).toHaveValue(
            "The fictional customer's Social Security number is 123-45-6789 and email is alex@example.com."
        )
        expect(screen.getByRole('textbox', { name: /Application name/ })).toHaveValue('Desktop starter cases')
        expect(sent(native, 'scan')).toEqual([])
        expect(sent(native, 'saveCase')).toEqual([])
        // The loaded form can be saved as a case explicitly.
        await user.click(screen.getByRole('button', { name: 'Save this case' }))
        expect(sent(native, 'saveCase')[0]).toMatchObject({ draft: { product: 'runtime-security', name: 'Sensitive data in a reply' } })
    })

    it('sends explicit import, export, catalog and copy intents and shows v1’s refusals', async () => {
        const b = tenant({ id: 'tn-ffffffffffffffff', label: 'Tenant B' })
        const native = scriptedNative(snap(library({ cases: [RUNTIME] }), [tenant(), b]))
        const user = await openLibrary(native)
        await user.click(screen.getByRole('button', { name: 'Export library…' }))
        await user.click(screen.getByRole('button', { name: 'Import cases…' }))
        await user.click(screen.getByRole('button', { name: /Import bundled examples/ }))
        await user.click(screen.getByRole('button', { name: 'Import public examples' }))
        await user.selectOptions(screen.getByRole('combobox', { name: 'Copy cases to' }), 'tn-ffffffffffffffff')
        await user.click(screen.getByRole('button', { name: `Copy ${RUNTIME.name} to the chosen tenant` }))
        await user.click(screen.getByRole('button', { name: `Remove case ${RUNTIME.name}` }))
        expect(native.dispatched.filter((i) => i.kind !== 'openLibrary').map((i) => i.kind)).toEqual([
            'exportCases',
            'importCases',
            'importExamples',
            'importExamples',
            'copyCase',
            'deleteCase',
        ])
        expect(sent(native, 'importExamples')).toEqual([
            { kind: 'importExamples', context: CONTEXT, source: 'bundled' },
            { kind: 'importExamples', context: CONTEXT, source: 'public' },
        ])
        expect(sent(native, 'copyCase')[0]).toMatchObject({ id: RUNTIME.id, tenant: 'tn-ffffffffffffffff' })
        await native.push(snap(library({ cases: [RUNTIME], problem: 'full' }), [tenant(), b]))
        expect(screen.getByText('Case limit reached; remove an existing case.')).toBeInTheDocument()
        await native.push(snap(library({ cases: [RUNTIME], notice: { kind: 'examplesAdded', added: 11 } }), [tenant(), b]))
        expect(screen.getByText('Added 11 examples. Examples are never run.')).toBeInTheDocument()
        await native.push(snap(library({ cases: [RUNTIME], export: { kind: 'cases', state: 'saved', name: 'prisma-airs-test-cases.json' } }), [tenant(), b]))
        expect(screen.getByText(/Saved prisma-airs-test-cases\.json\. Cases exported as plaintext JSON with private file permissions/)).toBeInTheDocument()
    })

    it('reports a scan-form save only once its outcome is established', async () => {
        const native = scriptedNative(snap(library({ state: 'idle' })))
        const user = await openLibrary(native)
        await user.click(within(screen.getByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Runtime Security/ }))
        await user.type(screen.getByRole('textbox', { name: 'Case name' }), 'Form case')
        await user.click(screen.getByRole('button', { name: 'Save this case' }))
        expect(screen.getByText('Saving to this tenant’s case Library…')).toBeInTheDocument()
        expect(screen.queryByText('Saved to this tenant’s case Library.')).toBeNull()
        // The library's write failed: the form says so and keeps its fields.
        await native.push(snap(library({ problem: 'notSaved', unsaved: 'unavailable' })))
        expect(screen.getByText(/could not be saved on this Mac\. Use Save again/)).toBeInTheDocument()
        expect(screen.getByRole('textbox', { name: 'Case name' })).toHaveValue('Form case')
        await user.click(screen.getByRole('button', { name: 'Save this case' }))
        await native.push(snap(library({ notice: { kind: 'saved', name: 'Form case' } })))
        expect(screen.getByText('Saved to this tenant’s case Library.')).toBeInTheDocument()
    })

    it('offers an explicit retry after a failed save', async () => {
        const native = scriptedNative(snap(library({ cases: [RUNTIME], unsaved: 'unavailable', problem: 'notSaved' })))
        const user = await openLibrary(native)
        await user.click(screen.getByRole('button', { name: 'Save again' }))
        expect(sent(native, 'saveLibraryAgain')).toEqual([{ kind: 'saveLibraryAgain', context: CONTEXT }])
    })

    it('reports a library that does not open and retries only when asked', async () => {
        const native = scriptedNative(snap(library({ state: 'failed', failure: 'locked' })))
        const user = await openLibrary(native)
        expect(screen.getByRole('alert')).toHaveTextContent('The Keychain is locked.')
        expect(screen.getByRole('button', { name: 'Create a case' })).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Open the library again' }))
        expect(sent(native, 'openLibrary')).toHaveLength(2)
    })
})
