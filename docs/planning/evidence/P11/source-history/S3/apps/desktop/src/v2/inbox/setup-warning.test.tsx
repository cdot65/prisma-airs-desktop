import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it } from 'vitest'
import { DesktopClient, type ShellSnapshot } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { scriptedNative } from '../../test/native'
import { fxInbox } from '../fixtures'
import { resetShellStore, useShellStore } from '../store'
import { follow } from './inbox'

const warning = {
    id: 'vw-12', product: 'modelSkill' as const, reason: 'Model scans: forbidden; Skill scans: available.', recordedSecs: 1_790_000_000,
    check: { version: 1 as const, line: 'modelSkill' as const, tsgId: '1762204418', origin: 'https://api.scm.paloaltonetworks.com', workspace: null },
}
function warned(): ShellSnapshot {
    const base = fxInbox()
    const entry = { ...base.inbox.items.find((e) => e.kind === 'setupWarning')!, id: warning.id, product: 'modelSkill' as const, title: 'Supply chain setup warning', detail: warning.reason, target: { feature: 'administration' as const, product: 'modelSkill' as const, resource: null, item: null }, actionLabel: 'Open AI Supply Chain Security' }
    return { ...base, tenants: base.tenants.map((t, i) => i === 0 ? { ...t, setupWarnings: [warning], management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'public-client@1762204418.iam.panserviceaccount.com', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' } } : t), inbox: { ...base.inbox, items: [entry] } }
}
beforeEach(() => resetShellStore())

describe('setup warning presentation', () => {
    it('preserves the product through the Inbox action and opens that Administration section', async () => {
        const snapshot = warned()
        follow(snapshot.inbox.items[0]!, snapshot.active!.tenant, () => {})
        expect(useShellStore.getState().arrival).toMatchObject({ product: 'modelSkill', feature: 'administration' })
        const native = scriptedNative(snapshot)
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await screen.findByRole('heading', { name: 'Inbox', level: 1 })
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Supply chain setup warning/ }))
        await user.click(screen.getByRole('button', { name: 'Open AI Supply Chain Security' }))
        await native.push({ ...native.current, activeFeature: 'administration' })
        const product = document.querySelector('[data-product="modelSkill"]') as HTMLElement
        await waitFor(() => expect(within(product).getByRole('button', { name: /AI Supply Chain Security/ })).toHaveAttribute('aria-expanded', 'true'))
        expect(within(product).getByRole('tab', { name: 'Model scans' })).toBeInTheDocument()
    })

    it('rechecks only the selected warning and shows native pending and failed results without clearing it locally', async () => {
        const native = scriptedNative(warned())
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await screen.findByRole('heading', { name: 'Inbox', level: 1 })
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Supply chain setup warning/ }))
        await user.click(screen.getByRole('button', { name: 'Recheck setup' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'recheckSetupWarning', context: native.current.active!.context, warning: warning.id })
        const update = async (state: 'pending' | 'warning') => native.push({ ...native.current, tenants: native.current.tenants.map((t, i) => i === 0 ? { ...t, setupWarnings: [{ ...warning, recheck: { checkedSecs: 1_790_000_100, state, reason: state === 'pending' ? 'Checking original scope.' : 'Model scans still need a role.', equivalent: state !== 'pending', basis: state === 'pending' ? 'unknown' as const : 'recordedScope' as const } }] } : t) })
        await update('pending')
        expect(screen.getByRole('button', { name: 'Recheck setup' })).toBeDisabled()
        await user.click(screen.getByRole('button', { name: 'Recheck setup' }))
        expect(native.dispatched.filter((i) => i.kind === 'recheckSetupWarning')).toHaveLength(1)
        expect(screen.getByText('Checking original scope.')).toBeInTheDocument()
        expect(screen.queryByText(/original scope could not be matched/i)).toBeNull()
        await update('warning')
        expect(screen.getByText('Model scans still need a role.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Recheck setup' })).not.toBeDisabled()
        expect(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Supply chain setup warning/ })).toBeInTheDocument()
        const checked = { ...warning, recheck: { checkedSecs: 1_790_000_101, state: 'pass' as const, reason: 'Current product read passed.', equivalent: false, basis: 'unknown' as const } }
        await native.push({ ...native.current, tenants: native.current.tenants.map((t, i) => i === 0 ? { ...t, setupWarnings: [checked] } : t) })
        expect(screen.getByText(/original scope could not be matched/i)).toBeInTheDocument()
        expect(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Supply chain setup warning/ })).toBeInTheDocument()
        const resolved = { ...checked, recheck: { ...checked.recheck, equivalent: true, basis: 'recordedScope' as const, reason: 'Matching sources passed.' }, resolvedSecs: 1_790_000_101 }
        await native.push({ ...native.current, inbox: { ...native.current.inbox, items: [] }, tenants: native.current.tenants.map((t, i) => i === 0 ? { ...t, setupWarnings: [resolved] } : t) })
        await waitFor(() => expect(screen.queryByRole('list', { name: 'Inbox items' })).toBeNull())
        await native.push({ ...native.current, activeFeature: 'settings' })
        const history = await screen.findByRole('region', { name: 'Setup check history' })
        expect(within(history).getByText(warning.reason)).toBeInTheDocument()
        expect(within(history).getByText('Matching sources passed.')).toBeInTheDocument()
        expect(history.querySelector('[data-warning="vw-12"] [data-state="resolved"]')).not.toBeNull()
    })

    it('reports a refused recheck through the scoped action surface and retains the warning', async () => {
        const base = warned()
        const { check: _check, ...legacy } = warning
        const native = scriptedNative({ ...base, tenants: base.tenants.map((t, i) => i === 0 ? { ...t, setupWarnings: [legacy] } : t) })
        native.respond = () => ({ outcome: 'refusal', refusal: 'staleContext' })
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await screen.findByRole('heading', { name: 'Inbox', level: 1 })
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Supply chain setup warning/ }))
        await user.click(screen.getByRole('button', { name: 'Recheck setup' }))
        expect(await screen.findByText(/setup recheck for AI Supply Chain Security was refused \(staleContext\)/)).toBeInTheDocument()
        expect(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Supply chain setup warning/ })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Recheck setup' })).not.toBeDisabled()
    })

    it('allows a legacy Scan Key configuration recheck without management credentials or a TSG ID', async () => {
        const base = warned()
        const legacy = { id: 'vw-13', product: 'runtimeSecurity' as const, reason: 'No scan key was configured.', recordedSecs: warning.recordedSecs }
        const entry = { ...base.inbox.items[0]!, id: legacy.id, product: 'runtimeSecurity' as const, title: 'Scan key setup warning', detail: legacy.reason }
        const native = scriptedNative({ ...base, inbox: { ...base.inbox, items: [entry] }, tenants: base.tenants.map((t, i) => i === 0 ? { ...t, tsgId: null, management: { state: 'missing', mode: null, failure: null, clientId: null, tokenOrigin: null }, setupWarnings: [legacy] } : t) })
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await screen.findByRole('heading', { name: 'Inbox', level: 1 })
        await user.click(within(screen.getByRole('list', { name: 'Inbox items' })).getByRole('button', { name: /Scan key setup warning/ }))
        await user.click(screen.getByRole('button', { name: 'Recheck setup' }))
        expect(native.dispatched.at(-1)).toEqual({ kind: 'recheckSetupWarning', context: base.active!.context, warning: legacy.id })
        const stored = { ...legacy, recheck: { checkedSecs: 1_790_000_101, state: 'stored' as const, reason: 'Scan key configured. Its validity was not tested.', equivalent: true, basis: 'legacyTenantSources' as const }, resolvedSecs: 1_790_000_101 }
        await native.push({ ...native.current, activeFeature: 'settings', inbox: { ...native.current.inbox, items: [] }, tenants: native.current.tenants.map((t, i) => i === 0 ? { ...t, setupWarnings: [stored] } : t) })
        const history = await screen.findByRole('region', { name: 'Setup check history' })
        expect(history).toHaveTextContent('Latest check: Stored')
        expect(history).toHaveTextContent('Its validity was not tested.')
        expect(history).not.toHaveTextContent('Latest check: Passed')
    })
})
