/**
 * Red Team & Models through the real App, DesktopClient and shell: v1's
 * reads on opening each tab, window changes, sources that never pass for
 * zero, partial samples as lower bounds, attack results paged to ten pages,
 * superseded replies dropped, failures keeping the last read, and no
 * enabled control for an unavailable workflow.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    type AttackPage,
    DesktopClient,
    type Intent,
    type ManagementRequest,
    type ManagementResult,
    type RedTeamObservation,
    type ShellSnapshot,
    type SupplyChainObservation,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'
import { statusSum } from './red-team-view'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const SCAN = '11111111-2222-4333-8444-555555555555'
const TARGET = '66666666-7777-4888-8999-aaaaaaaaaaaa'
const now = () => Math.floor(Date.now() / 1000)

const ready = (context = CONTEXT): ShellSnapshot =>
    withTenant(
        baseSnapshot,
        tenant({
            management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'svc@1234567890.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' },
        }),
        context
    )

const redTeam = (overrides: Partial<RedTeamObservation> = {}): RedTeamObservation => ({
    readAt: now(),
    window: 30,
    statistics: {
        scans: 12,
        targets: 3,
        status: [
            { label: 'COMPLETED', count: 8 },
            { label: 'RUNNING', count: 2 },
            { label: 'QUEUED', count: 1 },
            { label: 'FAILED', count: 1 },
        ],
        risk: null,
        targetTypes: [{ label: 'APPLICATION', count: 3 }],
        noScans: false,
    },
    statisticsFailure: null,
    scans: {
        rows: [
            { id: SCAN, status: 'COMPLETED', jobType: 'STATIC', targetId: TARGET, targetType: 'APPLICATION', createdAt: '2026-10-18T10:00:00Z' },
            { id: TARGET, status: 'RUNNING', jobType: 'DYNAMIC', targetId: null, targetType: null, createdAt: null },
        ],
        total: 2,
        capped: false,
        complete: true,
    },
    scansFailure: null,
    ...overrides,
})

const models = (overrides: Partial<SupplyChainObservation> = {}): SupplyChainObservation => ({
    readAt: now(),
    models: { rows: [{ id: SCAN, outcome: 'BLOCKED', createdAt: null }], total: 140, capped: false, complete: false },
    modelsFailure: null,
    skillStatistics: { uniqueSkills: 4, vulnerabilities: 6 },
    skillStatisticsFailure: null,
    skills: { rows: [{ pseudonym: 'a1b2c3d4', status: 'COMPLETED', outcome: 'ALLOWED', vulnerabilities: 2, createdAt: null }], total: null, capped: false, complete: false },
    skillsFailure: null,
    ...overrides,
})

const attackPage = (page: number, n: number, total: number | null, complete: boolean, capped = false): AttackPage => ({
    scanId: SCAN,
    page,
    rows: Array.from({ length: n }, (_, i) => ({
        number: page * 100 + i + 1,
        threat: i % 2 === 0 ? true : i % 3 === 0 ? null : false,
        severity: 'HIGH',
        attackType: 'NORMAL',
        subCategory: 'Jailbreak',
        multiTurn: false,
    })),
    total,
    capped,
    complete,
})

type Answer = (request: ManagementRequest) => ManagementResult | Promise<ManagementResult>

function management(native: Native, answer: Answer): ManagementRequest[] {
    const reads: ManagementRequest[] = []
    const pending = new Map<string, ManagementRequest>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind !== 'management') return respond(intent)
        reads.push(intent.request)
        const operation = `op-${reads.length}`
        pending.set(operation, intent.request)
        return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${reads.length}`, operation } }
    }
    native.observeManagement = async (operation) => {
        const request = pending.get(operation)
        if (!request) return { kind: 'refusal', refusal: 'invalidInput' }
        return { kind: 'management', result: await answer(request) }
    }
    return reads
}

const standard: Answer = (request) =>
    request.kind === 'redTeam'
        ? { kind: 'redTeam', observation: redTeam({ window: request.window }) }
        : request.kind === 'supplyChain'
          ? { kind: 'supplyChain', observation: models() }
          : request.kind === 'attacks'
            ? { kind: 'attacks', attacks: attackPage(request.page, 100, null, false) }
            : { kind: 'failed', failure: 'invalidResponse', outcome: 'completed' }

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Red Team/ }))
    await screen.findByRole('heading', { level: 1, name: 'Red Teaming and model observation' })
    return user
}

/** The value shown for a summary tile. */
const value = (name: string) => {
    const term = screen.getByText(name, { selector: 'dt' })
    return term.nextElementSibling?.textContent
}

describe('Red Team & Models', () => {
    it('reads Red Teaming on opening and shows v1’s tiles, lists and coverage', async () => {
        const native = scriptedNative(ready())
        const reads = management(native, standard)
        await open(native)
        await waitFor(() => expect(value('Scans')).toBe('12'))
        expect(reads).toEqual([{ kind: 'redTeam', window: 30 }])
        expect(value('Targets scanned')).toBe('3')
        expect(value('Active scans')).toBe('3')
        expect(value('Failed scans')).toBe('1')
        expect(screen.getByRole('heading', { name: 'Risk profile' }).nextElementSibling).toHaveTextContent('Not reported by the service.')
        expect(screen.getByText('Scans · 2 read · complete')).toBeInTheDocument()
        expect(screen.getAllByRole('row')).toHaveLength(3)
        // Only finished scans offer their attack results.
        expect(screen.getByRole('button', { name: 'Open attack results for scan 11111111' })).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Open attack results for scan 66666666' })).toBeNull()
        // No control starts an unavailable workflow.
        expect(screen.getByText(/Starting or re-running Red Teaming scans, verifying ASR/)).toBeInTheDocument()
        for (const name of [/new scan/i, /start scan/i, /verify asr/i, /new target/i, /create/i]) {
            expect(screen.queryByRole('button', { name })).toBeNull()
        }
    })

    it('reads a new window and filters the rows read locally', async () => {
        const native = scriptedNative(ready())
        const reads = management(native, standard)
        const user = await open(native)
        await waitFor(() => expect(value('Scans')).toBe('12'))
        await user.click(screen.getByRole('radio', { name: 'Last 7 days' }))
        await waitFor(() => expect(reads.at(-1)).toEqual({ kind: 'redTeam', window: 7 }))
        expect(await screen.findByText(/last 7 days/)).toBeInTheDocument()
        await user.selectOptions(screen.getByRole('combobox', { name: 'Status' }), 'Running')
        expect(screen.getAllByRole('row')).toHaveLength(2)
        expect(reads.filter((r) => r.kind === 'redTeam')).toHaveLength(2)
    })

    it('states failed and unreported sources instead of zeros and keeps the last read', async () => {
        const native = scriptedNative(ready())
        let fail = false
        management(native, (request) =>
            fail
                ? { kind: 'failed', failure: 'rateLimited', outcome: 'rejected' }
                : {
                      kind: 'redTeam',
                      observation: redTeam({
                          window: request.kind === 'redTeam' ? request.window : 30,
                          statistics: { scans: 5, targets: 1, status: null, risk: null, targetTypes: null, noScans: false },
                          scans: null,
                          scansFailure: 'denied',
                      }),
                  }
        )
        const user = await open(native)
        await waitFor(() => expect(value('Scans')).toBe('5'))
        expect(value('Active scans')).toBe('Unreported')
        expect(screen.getByText(/Scans · Unavailable: This management client is not allowed/)).toBeInTheDocument()
        expect(screen.getByText('The scan list could not be read.')).toBeInTheDocument()
        fail = true
        await user.click(screen.getByRole('button', { name: 'Refresh Red Teaming' }))
        expect(await screen.findByRole('alert')).toHaveTextContent('The last read failed: The service is limiting requests')
        expect(screen.getByRole('alert')).toHaveTextContent(/Showing what was read at/)
        expect(value('Scans')).toBe('5')
    })

    it('treats an empty tenant as no scans and an unreported breakdown as zero there', () => {
        expect(statusSum(null, 0, ['RUNNING'])).toBe(0)
        expect(statusSum(null, 3, ['RUNNING'])).toBeNull()
        expect(statusSum([{ label: 'RUNNING', count: 2 }, { label: 'PENDING', count: 1 }], 3, ['RUNNING', 'PENDING'])).toBe(3)
    })

    it('reads attack results to ten pages on opening and counts only reported successes', async () => {
        const native = scriptedNative(ready())
        const reads = management(native, standard)
        const user = await open(native)
        await user.click(await screen.findByRole('button', { name: 'Open attack results for scan 11111111' }))
        const region = await screen.findByRole('region', { name: 'Attack results · scan 11111111' })
        expect(within(region).getByRole('heading', { name: 'Attack results · scan 11111111' })).toHaveFocus()
        await waitFor(() => expect(within(region).getByText(/the 1,000-attack cap is reached/)).toBeInTheDocument())
        expect(reads.filter((r) => r.kind === 'attacks').map((r) => (r as { page: number }).page)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9])
        expect(within(region).getByText(/^500 of 1000 succeeded · read 100 per page · more may exist · the 1,000-attack cap is reached$/)).toBeInTheDocument()
        expect(within(region).getAllByText('Not rated').length).toBeGreaterThan(0)
        // Rows read are shown 100 at a time.
        expect(within(region).getAllByRole('row')).toHaveLength(101)
        expect(within(region).getByText('#001')).toBeInTheDocument()
        expect(within(region).getByText('Attacks 1–100 of 1,000 read')).toBeInTheDocument()
        await user.click(within(region).getByRole('button', { name: 'Next 100 attacks' }))
        expect(within(region).getByText('#101')).toBeInTheDocument()
        expect(within(region).queryByText('#001')).toBeNull()
        await user.click(within(region).getByRole('button', { name: 'Previous 100 attacks' }))
        expect(within(region).getByText('#001')).toBeInTheDocument()
        expect(within(region).getByText(/Attack prompts and the target's replies stay in the Red Teaming report/)).toBeInTheDocument()
    })

    it('stops reading attack results once the total is covered or a read fails', async () => {
        const native = scriptedNative(ready())
        let fail = false
        const reads = management(native, (request) =>
            request.kind === 'attacks'
                ? fail && request.page === 1
                    ? { kind: 'failed', failure: 'unavailable', outcome: 'completed' }
                    : { kind: 'attacks', attacks: attackPage(request.page, request.page === 2 ? 50 : 100, 250, request.page === 2) }
                : standard(request)
        )
        const user = await open(native)
        await user.click(await screen.findByRole('button', { name: 'Open attack results for scan 11111111' }))
        const region = await screen.findByRole('region', { name: 'Attack results · scan 11111111' })
        await waitFor(() => expect(within(region).getByText(/· all attacks read$/)).toBeInTheDocument())
        // Each page carries the rows received before it.
        expect(reads.filter((r) => r.kind === 'attacks').map((r) => (r as { received: number }).received)).toEqual([0, 100, 200])
        await user.click(within(region).getByRole('button', { name: 'Close attack results' }))
        fail = true
        await user.click(screen.getByRole('button', { name: 'Open attack results for scan 11111111' }))
        const again = await screen.findByRole('region', { name: 'Attack results · scan 11111111' })
        expect(await within(again).findByRole('alert')).toHaveTextContent('Attack results could not be read: The management service is unavailable')
        expect(within(again).getByText(/^50 of 100 succeeded · read 100 per page · showing 100 of 250$/)).toBeInTheDocument()
        expect(reads.filter((r) => r.kind === 'attacks')).toHaveLength(5)
    })

    it('stops on a short page and never calls an unread remainder "no attacks"', async () => {
        const native = scriptedNative(ready())
        let first = 0
        const reads = management(native, (request) =>
            request.kind === 'attacks' ? { kind: 'attacks', attacks: attackPage(request.page, first, 150, false) } : standard(request)
        )
        const user = await open(native)
        await user.click(await screen.findByRole('button', { name: 'Open attack results for scan 11111111' }))
        const region = await screen.findByRole('region', { name: 'Attack results · scan 11111111' })
        await waitFor(() => expect(within(region).getByText(/showing 0 of 150 · the service returned fewer than it reports$/)).toBeInTheDocument())
        expect(within(region).getByText(/the results could not be read in full/)).toBeInTheDocument()
        expect(within(region).queryByText(/reports no attacks/)).toBeNull()
        expect(within(region).queryByText(/all attacks read/)).toBeNull()
        expect(reads.filter((r) => r.kind === 'attacks')).toHaveLength(1)
        // A short page with rows stops the walk too.
        await user.click(within(region).getByRole('button', { name: 'Close attack results' }))
        first = 40
        await user.click(screen.getByRole('button', { name: 'Open attack results for scan 11111111' }))
        const again = await screen.findByRole('region', { name: 'Attack results · scan 11111111' })
        await waitFor(() => expect(within(again).getByText(/showing 40 of 150 · the service returned fewer than it reports$/)).toBeInTheDocument())
        expect(reads.filter((r) => r.kind === 'attacks')).toHaveLength(2)
    })

    it('shows a capped total as a lower bound', async () => {
        const native = scriptedNative(ready())
        management(native, (request) =>
            request.kind === 'attacks'
                ? { kind: 'attacks', attacks: attackPage(request.page, 20, 20, false, true) }
                : request.kind === 'supplyChain'
                  ? {
                        kind: 'supplyChain',
                        observation: models({ models: { rows: [{ id: SCAN, outcome: 'BLOCKED', createdAt: null }], total: 500, capped: true, complete: false } }),
                    }
                  : standard(request)
        )
        const user = await open(native)
        await user.click(await screen.findByRole('button', { name: 'Open attack results for scan 11111111' }))
        const region = await screen.findByRole('region', { name: 'Attack results · scan 11111111' })
        await waitFor(() => expect(within(region).getByText(/showing 20 of ≥ 20/)).toBeInTheDocument())
        await user.click(screen.getByRole('tab', { name: 'Models & skills' }))
        await waitFor(() => expect(value('Model scans (reported)')).toBe('≥ 500'))
        expect(screen.getByText('Model scans · Partial sample · 1 of ≥ 500')).toBeInTheDocument()
    })

    it('reads models and skills with partial samples as lower bounds and skills by pseudonym', async () => {
        const native = scriptedNative(ready())
        const reads = management(native, standard)
        const user = await open(native)
        await user.click(screen.getByRole('tab', { name: 'Models & skills' }))
        await waitFor(() => expect(value('Unique skills scanned (30 days)')).toBe('4'))
        expect(reads.at(-1)).toEqual({ kind: 'supplyChain' })
        expect(value('Model scans (reported)')).toBe('140')
        expect(value('Skill scans (observed)')).toBe('≥ 1')
        expect(value('Skill vulnerabilities (30 days)')).toBe('6')
        expect(screen.getByText('Model scans · Partial sample · 1 of 140')).toBeInTheDocument()
        expect(screen.getByText('Skill a1b2c3d4 (pseudonym for this read)')).toBeInTheDocument()
        expect(screen.getByText('2 vulnerabilities found')).toBeInTheDocument()
        expect(screen.getByText(/Starting model scans and managing security groups or model sources are not available here/)).toBeInTheDocument()
        // Returning to Red Teaming does not read it again.
        await user.click(screen.getByRole('tab', { name: 'Red Teaming' }))
        expect(reads.filter((r) => r.kind === 'redTeam')).toHaveLength(1)
    })

    it('drops a superseded window’s late reply', async () => {
        const native = scriptedNative(ready())
        let release: () => void = () => undefined
        management(native, async (request) => {
            if (request.kind === 'redTeam' && request.window === 30) {
                await new Promise<void>((resolve) => (release = resolve))
                return { kind: 'redTeam', observation: redTeam({ window: 30, statistics: { ...redTeam().statistics!, scans: 999 } }) }
            }
            return standard(request)
        })
        const user = await open(native)
        await user.click(screen.getByRole('radio', { name: 'Last 15 days' }))
        await waitFor(() => expect(value('Scans')).toBe('12'))
        release()
        await new Promise((resolve) => setTimeout(resolve, 0))
        expect(value('Scans')).toBe('12')
        expect(screen.getByText(/last 15 days/)).toBeInTheDocument()
    })

    it('asks for a management client before reading', async () => {
        const native = scriptedNative(withTenant(baseSnapshot, tenant(), CONTEXT))
        const reads = management(native, standard)
        await open(native)
        expect(screen.getByText(/Configure one in Settings to see them/)).toBeInTheDocument()
        expect(reads).toEqual([])
    })
})
