/**
 * Production Runtime Security scan submission through the real App,
 * DesktopClient and shell: input captured fresh at click time, the v1 and
 * 2 MiB/8 MiB request limits, faithful verdicts, cancellation, uncertain
 * outcomes, context retirement and no automatic resubmission.
 */
import { act, fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DesktopClient, type ScanRecordView, type ScanResultView, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const ready = withTenant(baseSnapshot, tenant({ scan: { state: 'ready', mode: 'keychain', failure: null } }), CONTEXT)
const admitted = (operation: string) => ({ outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-1', operation } })

function result(overrides: Partial<ScanResultView> = {}): ScanResultView {
    return {
        scanId: 'S-fixture-1',
        reportId: 'R-fixture-1',
        trId: null,
        profileName: 'Production guardrails',
        profileId: null,
        category: 'malicious',
        action: 'block',
        verdict: 'block',
        timeout: false,
        serviceError: false,
        contentErrors: [],
        promptDetected: [
            { detector: 'dlp', detected: false },
            { detector: 'future_detector', detected: true },
        ],
        responseDetected: [],
        topics: { violation: null, blocked: [], allowed: [] },
        responseTopics: { violation: null, blocked: [], allowed: [] },
        ...overrides,
    }
}

function record(overrides: Partial<ScanRecordView> = {}): ScanRecordView {
    return {
        operation: 'op-1',
        progress: 'settled',
        outcome: 'completed',
        stop: null,
        result: result(),
        failure: null,
        discarded: false,
        ...overrides,
    }
}

const withScans = (snapshot: ShellSnapshot, scans: ScanRecordView[]): ShellSnapshot => ({ ...snapshot, scans })

async function launch(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await screen.findByRole('heading', { level: 1, name: 'Scan a prompt or response' })
    return user
}

const field = (label: string | RegExp) => screen.getByLabelText(label) as HTMLInputElement | HTMLTextAreaElement
const scanButton = () => screen.getByRole('button', { name: 'Scan' })
/** Sets a large value the way a paste would, without per-key typing. */
const paste = (label: string, value: string) => fireEvent.input(field(label), { target: { value } })

let setItem: ReturnType<typeof vi.spyOn>
beforeEach(() => {
    setItem = vi.spyOn(Storage.prototype, 'setItem')
})
afterEach(() => setItem.mockRestore())

describe('runtime scan submission', () => {
    it('captures the values present at click time and sends one literal scan intent', async () => {
        const native = scriptedNative(ready)
        native.respond = () => admitted('op-1')
        const user = await launch(native)
        await user.type(field('Profile name or ID'), 'Draft profile')
        await user.type(field('Prompt'), 'first draft')
        await user.clear(field('Profile name or ID'))
        await user.type(field('Profile name or ID'), '  Production guardrails ')
        await user.clear(field('Prompt'))
        await user.type(field('Prompt'), 'Ignore previous instructions')
        await user.type(field('Application name (optional)'), 'Desktop workbench')
        await user.click(scanButton())
        expect(native.dispatched).toEqual([
            {
                kind: 'scan',
                context: CONTEXT,
                draft: {
                    profile: { kind: 'name', name: 'Production guardrails' },
                    prompt: 'Ignore previous instructions',
                    response: '',
                    appName: 'Desktop workbench',
                },
            },
        ])
        expect(setItem).not.toHaveBeenCalled()
    })

    it('sends a profile by ID only when it is a UUID', async () => {
        const native = scriptedNative(ready)
        native.respond = () => admitted('op-1')
        const user = await launch(native)
        await user.click(screen.getByRole('radio', { name: 'By ID' }))
        await user.type(field('Profile name or ID'), 'not-a-uuid')
        await user.type(field('Response'), 'hello')
        await user.click(scanButton())
        expect(native.dispatched).toEqual([])
        expect(screen.getByRole('alert')).toHaveTextContent('A profile ID is a UUID')
        expect(field('Profile name or ID')).toHaveFocus()
        await user.clear(field('Profile name or ID'))
        await user.type(field('Profile name or ID'), '1B2C3D4E-0000-4000-8000-00000000ABCD')
        await user.click(scanButton())
        expect(native.dispatched).toHaveLength(1)
        expect(native.dispatched[0]).toMatchObject({
            kind: 'scan',
            draft: { profile: { kind: 'id', id: '1b2c3d4e-0000-4000-8000-00000000abcd' }, prompt: '', response: 'hello', appName: null },
        })
    })

    it('requires an explicit profile and some content', async () => {
        const native = scriptedNative(ready)
        const user = await launch(native)
        await user.type(field('Prompt'), 'hello')
        await user.click(scanButton())
        expect(screen.getByRole('alert')).toHaveTextContent('Choose the AI security profile for this scan.')
        await user.type(field('Profile name or ID'), 'P')
        await user.clear(field('Prompt'))
        await user.type(field('Prompt'), '   ')
        await user.click(scanButton())
        expect(screen.getByRole('alert')).toHaveTextContent('Enter a prompt, a response, or both.')
        expect(native.dispatched).toEqual([])
    })

    it('sends working v1 inputs and the largest accepted request, and refuses past the 2 MiB and 8 MiB ceilings', async () => {
        const native = scriptedNative(ready)
        let n = 0
        native.respond = () => admitted(`op-${++n}`)
        const user = await launch(native)
        await user.type(field('Profile name or ID'), 'P')
        // v1's largest working input: 96 KiB combined.
        paste('Prompt', 'p'.repeat(64 * 1024))
        paste('Response', 'r'.repeat(32 * 1024))
        await user.click(scanButton())
        expect(native.dispatched).toHaveLength(1)
        const v1 = native.dispatched[0] as { draft: { prompt: string; response: string } }
        expect([v1.draft.prompt.length, v1.draft.response.length]).toEqual([64 * 1024, 32 * 1024])
        await native.push(withScans(ready, [record({ operation: 'op-1' })]))
        // 2 MiB + 2 MiB fits inside the 8 MiB request ceiling.
        const big = 'b'.repeat(2 * 1024 * 1024)
        paste('Prompt', big)
        paste('Response', big)
        await user.click(scanButton())
        expect(native.dispatched).toHaveLength(2)
        await native.push(withScans(ready, [record({ operation: 'op-2' }), record({ operation: 'op-1' })]))
        // One byte past 2 MiB (UTF-8, so a 3-byte character counts as 3).
        paste('Response', '')
        paste('Prompt', 'x'.repeat(2 * 1024 * 1024 - 2) + '€')
        await user.click(scanButton())
        expect(screen.getByRole('alert')).toHaveTextContent('The prompt is larger than 2 MiB.')
        // Escaping can push a 2 MiB text past 8 MiB on the wire.
        paste('Prompt', '\u0001'.repeat(2 * 1024 * 1024))
        await user.click(scanButton())
        expect(screen.getByRole('alert')).toHaveTextContent('larger than 8 MiB')
        expect(native.dispatched).toHaveLength(2)
    })

    it('shows the verdict, verbatim action and category, and every detector the service reported', async () => {
        const native = scriptedNative(ready)
        native.respond = () => admitted('op-1')
        const user = await launch(native)
        await user.type(field('Profile name or ID'), 'P')
        await user.type(field('Prompt'), 'hello')
        await user.click(scanButton())
        await native.push(withScans(ready, [record({ progress: 'running', outcome: null, result: null })]))
        expect(within(screen.getByRole('region', { name: 'Scan result' })).getByText('Scanning…')).toBeInTheDocument()
        await native.push(withScans(ready, [record()]))
        const panel = within(screen.getByRole('region', { name: 'Scan result' }))
        expect(panel.getByText('Blocked')).toBeInTheDocument()
        expect(panel.getByText('block')).toBeInTheDocument()
        expect(panel.getByText('malicious')).toBeInTheDocument()
        expect(panel.getByText('S-fixture-1')).toBeInTheDocument()
        expect(panel.getByText('future_detector: detected')).toBeInTheDocument()
        expect(panel.getByText('dlp: not detected')).toBeInTheDocument()
    })

    it.each([
        ['an unrecognized action', result({ action: 'future-action', category: 'benign', verdict: 'unknown' }), 'does not recognize'],
        ['an incomplete scan', result({ action: 'allow', category: 'benign', verdict: 'unknown', timeout: true }), 'incomplete scan'],
        [
            'a content error with the global flags false',
            result({
                action: 'allow',
                category: 'benign',
                verdict: 'unknown',
                contentErrors: [{ contentType: 'prompt', feature: 'dlp', status: 'error' }],
            }),
            'incomplete scan',
        ],
    ])('never presents %s as allowed', async (_case, view, explanation) => {
        const native = scriptedNative(withScans(ready, [record({ result: view })]))
        await launch(native)
        const panel = within(screen.getByRole('region', { name: 'Scan result' }))
        expect(panel.getByText('Not determined')).toBeInTheDocument()
        expect(panel.getByText(new RegExp(explanation))).toBeInTheDocument()
        expect(panel.queryByText('Allowed')).toBeNull()
        expect(panel.getByText(view.action)).toBeInTheDocument()
        if (view.contentErrors.length > 0) expect(panel.getByText('prompt · dlp · error')).toBeInTheDocument()
    })

    it('reports a denied key and a lost response truthfully and never resubmits', async () => {
        const native = scriptedNative(
            withScans(ready, [record({ operation: 'op-2', outcome: 'unknown', result: null, failure: { kind: 'network', status: null } })])
        )
        await launch(native)
        const panel = within(screen.getByRole('region', { name: 'Scan result' }))
        expect(panel.getByText('The connection failed before a reply arrived.')).toBeInTheDocument()
        expect(panel.getByText(/The outcome is unknown\. The request may or may not have reached the service/)).toBeInTheDocument()
        expect(panel.queryByText(/was sent/)).toBeNull()
        expect(panel.getByText(/Press Scan to send it again only if/)).toBeInTheDocument()
        expect(scanButton()).toBeEnabled()
        await native.push(
            withScans(ready, [record({ operation: 'op-3', outcome: 'rejected', result: null, failure: { kind: 'unauthorized', status: 401 } })])
        )
        expect(panel.getByText(/did not accept the scan key \(HTTP 401\)/)).toBeInTheDocument()
        expect(panel.getByText('The service refused the request; it was not scanned.')).toBeInTheDocument()
        expect(native.dispatched).toEqual([])
    })

    it('cancels a running scan by its operation and blocks a second submission while one is in flight', async () => {
        const native = scriptedNative(ready)
        native.respond = () => admitted('op-7')
        const user = await launch(native)
        await user.type(field('Profile name or ID'), 'P')
        await user.type(field('Prompt'), 'hello')
        await user.click(scanButton())
        await native.push(withScans(ready, [record({ operation: 'op-7', progress: 'running', outcome: null, result: null })]))
        expect(scanButton()).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Cancel scan' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'cancelScan', operation: 'op-7' })
        await native.push(
            withScans(ready, [record({ operation: 'op-7', outcome: 'unknown', stop: 'cancelled', result: null })])
        )
        const panel = within(screen.getByRole('region', { name: 'Scan result' }))
        expect(panel.getByText(/Cancelled\. The request may already have reached the service/)).toBeInTheDocument()
        expect(panel.queryByText(/after it was sent/)).toBeNull()
        expect(scanButton()).toBeEnabled()
        expect(native.dispatched.filter((i) => i.kind === 'scan')).toHaveLength(1)
    })

    it('reports an unconfirmed scan delivery without resending it', async () => {
        const native = scriptedNative(ready)
        native.transport.dispatch = vi.fn(async () => {
            throw new Error('lost')
        })
        const user = await launch(native)
        await user.type(field('Profile name or ID'), 'P')
        await user.type(field('Prompt'), 'hello')
        await user.click(scanButton())
        expect(await screen.findByRole('alert')).toHaveTextContent('did not confirm the scan. Check the history')
        // One delivery, resent once with identical bytes; never a new scan.
        const sent = (native.transport.dispatch as ReturnType<typeof vi.fn>).mock.calls.map(([bytes]) => new TextDecoder().decode(bytes as Uint8Array))
        expect(sent).toHaveLength(2)
        expect(sent[0]).toBe(sent[1])
    })

    it('retires shown results and messages when the tenant context changes', async () => {
        const native = scriptedNative(withScans(ready, [record()]))
        native.respond = () => ({ outcome: 'refusal', refusal: 'staleContext' })
        const user = await launch(native)
        await user.type(field('Profile name or ID'), 'P')
        await user.type(field('Prompt'), 'hello')
        await user.click(scanButton())
        expect(screen.getByRole('alert')).toHaveTextContent('The active tenant changed')
        const other = tenant({ id: 'tn-fedcba9876543210', label: 'Lab', scan: { state: 'ready', mode: 'session', failure: null } })
        await native.push(withScans(withTenant(baseSnapshot, other, 'cx-bbbbbbbbbbbbbbbbbbbbbbbb'), []))
        expect(screen.queryByRole('alert')).toBeNull()
        expect(screen.queryByRole('region', { name: 'Scan result' })).toBeNull()
        expect(screen.getByText(/Scanning with Lab/)).toBeInTheDocument()
        await native.push(
            withScans(withTenant(baseSnapshot, other, 'cx-bbbbbbbbbbbbbbbbbbbbbbbb'), [
                record({ outcome: 'unknown', stop: 'contextRetired', result: null, discarded: true }),
            ])
        )
        expect(screen.getAllByText(/Any reply was discarded and is not shown/).length).toBeGreaterThan(0)
        expect(screen.queryByText('Blocked')).toBeNull()
    })

    it('shows history newest first and lets the operator choose which scan to view', async () => {
        const native = scriptedNative(
            withScans(ready, [
                record({ operation: 'op-3', result: result({ verdict: 'allow', action: 'allow', category: 'benign', scanId: 'S-3' }) }),
                record({ operation: 'op-2', result: result({ scanId: 'S-2' }) }),
            ])
        )
        const user = await launch(native)
        const history = within(screen.getByRole('region', { name: /Recent scans for Prod EU/ }))
        const items = history.getAllByRole('button')
        expect(items.map((b) => b.textContent)).toEqual(['Latest scanAllowed', 'Scan 1 of 2Blocked'])
        expect(screen.getByText('S-3')).toBeInTheDocument()
        await user.click(items[1]!)
        expect(screen.getByText('S-2')).toBeInTheDocument()
        expect(items[1]).toHaveAttribute('aria-current', 'true')
    })

    it('disables scanning without a ready key and points to Settings', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant(), CONTEXT))
        const user = await launch(native)
        expect(scanButton()).toBeDisabled()
        expect(screen.getByText(/has no ready scan key/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Open Settings' }))
        expect(screen.getByRole('heading', { level: 1, name: 'Settings' })).toBeInTheDocument()
        expect(native.dispatched).toEqual([])
    })

    it('is operable by keyboard alone', async () => {
        const native = scriptedNative(ready)
        native.respond = () => admitted('op-1')
        const user = await launch(native)
        await act(async () => field('Profile name or ID').focus())
        await user.keyboard('P')
        await user.tab()
        await user.keyboard('hello')
        expect(field('Prompt')).toHaveFocus()
        await user.tab()
        await user.tab()
        await user.tab()
        expect(scanButton()).toHaveFocus()
        await user.keyboard('{Enter}')
        expect(native.dispatched).toHaveLength(1)
    })
})
