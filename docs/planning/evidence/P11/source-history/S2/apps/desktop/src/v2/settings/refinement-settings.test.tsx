import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { SetupMode, SetupView } from '@prisma-airs/desktop-contracts'
import { SettingsView } from '../../shell/feature-view'
import { Wizard } from '../wizard/wizard'
import { draft, setupView } from '../wizard/fixtures'
import { fxHome, fxSwitcher } from '../fixtures'
import { TenantsSettings } from '../../modules/tenants/tenants-settings'

const dispatch = () => vi.fn().mockResolvedValue({ kind: 'admitted', operation: null })

describe('compact settings contracts', () => {
    it('changes palette independently from appearance', async () => {
        const user = userEvent.setup()
        const send = dispatch()
        const appearance = vi.fn()
        render(<SettingsView snapshot={{ ...fxHome(), colorTheme: 'prismaAirs', appearance: 'light' }} pending={false} onSetAppearance={appearance} dispatch={send} observeManagement={vi.fn()} />)
        await user.selectOptions(screen.getByRole('combobox', { name: 'Theme' }), 'gruvbox')
        expect(send).toHaveBeenCalledWith({ kind: 'setColorTheme', theme: 'gruvbox' })
        expect(appearance).not.toHaveBeenCalled()
        await user.selectOptions(screen.getByRole('combobox', { name: 'Appearance' }), 'dark')
        expect(appearance).toHaveBeenCalledWith('dark')
    })

    it('edits only the explicitly selected profile metadata', async () => {
        const user = userEvent.setup()
        const send = dispatch()
        const snapshot = fxSwitcher()
        render(<TenantsSettings snapshot={snapshot} dispatch={send} observeManagement={vi.fn()} />)
        const card = within(document.querySelector('[data-slot="update-tenant"]') as HTMLElement)
        await user.selectOptions(card.getByRole('combobox', { name: 'Tenant to update' }), snapshot.tenants[1]!.id)
        await user.click(card.getByRole('button', { name: 'Edit' }))
        await user.clear(card.getByRole('textbox', { name: 'Display name' }))
        await user.type(card.getByRole('textbox', { name: 'Display name' }), 'Renamed')
        await user.selectOptions(card.getByRole('combobox', { name: 'Environment' }), 'staging')
        await user.click(card.getByRole('button', { name: 'Save changes' }))
        expect(send).toHaveBeenCalledWith({ kind: 'editTenant', tenant: snapshot.tenants[1]!.id, label: 'Renamed', environment: 'staging' })
        expect(send.mock.calls).toHaveLength(1)
    })

    it('does not carry a delete confirmation to a different profile after removal', async () => {
        const user = userEvent.setup()
        const send = dispatch()
        const snapshot = fxSwitcher()
        const view = render(<TenantsSettings snapshot={snapshot} dispatch={send} observeManagement={vi.fn()} />)
        const card = () => within(document.querySelector('[data-slot="update-tenant"]') as HTMLElement)
        await user.click(card().getByRole('button', { name: 'Delete' }))
        expect(card().getByRole('button', { name: /Confirm deleting Northwind/ })).toBeInTheDocument()
        view.rerender(<TenantsSettings snapshot={{ ...snapshot, tenants: [snapshot.tenants[1]!] }} dispatch={send} observeManagement={vi.fn()} />)
        expect(card().queryByRole('button', { name: /Confirm deleting/ })).not.toBeInTheDocument()
        expect(card().getByRole('button', { name: 'Delete' })).toBeInTheDocument()
        expect(send).not.toHaveBeenCalled()
    })

    it('opens Add as an isolated wizard rather than creating a credential-free profile', async () => {
        const user = userEvent.setup()
        const send = dispatch()
        render(<TenantsSettings snapshot={fxSwitcher()} dispatch={send} observeManagement={vi.fn()} />)
        await user.click(screen.getByRole('button', { name: 'Add tenant…' }))
        expect(screen.getByRole('dialog', { name: 'Add a tenant' })).toBeInTheDocument()
        expect(screen.queryByRole('radio', { name: 'Custom HTTPS endpoint' })).not.toBeInTheDocument()
        expect(send).not.toHaveBeenCalled()
    })
})

describe.each<SetupMode>(['firstRun', 'add'])('required OAuth in %s setup', (mode) => {
    it('cannot continue with only a Scan Key and no management client', () => {
        const setup = { ...setupView({ mode, tenant: draft({ scan: { state: 'ready', mode: 'keychain', failure: null } }) }), scanRequired: false, managementRequired: true } as SetupView
        render(<Wizard snapshot={{ ...fxHome(), setup }} dispatch={dispatch()} observeManagement={vi.fn()} mode={mode} initial={{ step: 'tenant' }} onClose={vi.fn()} onNavigate={vi.fn()} />)
        expect(screen.getByRole('button', { name: 'Save the management client to continue' })).toBeDisabled()
    })

    it('can reach verification with a management client and no Scan Key', () => {
        const setup = { ...setupView({ mode, tenant: draft({ management: { state: 'ready', mode: 'keychain', failure: null, clientId: 'synthetic-client', tokenOrigin: 'https://auth.apps.paloaltonetworks.com' } }) }), scanRequired: false, managementRequired: true } as SetupView
        render(<Wizard snapshot={{ ...fxHome(), setup }} dispatch={dispatch()} observeManagement={vi.fn()} mode={mode} initial={{ step: 'tenant' }} onClose={vi.fn()} onNavigate={vi.fn()} />)
        expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled()
        expect(screen.getByText(/^OAuth authentication is required before/)).toBeInTheDocument()
    })

    it('clears an entered client secret when the draft context changes', async () => {
        const user = userEvent.setup()
        const setup = { ...setupView({ mode, tenant: draft() }), scanRequired: false, managementRequired: true } as SetupView
        const props = { dispatch: dispatch(), observeManagement: vi.fn(), mode, initial: { step: 'tenant' as const }, onClose: vi.fn(), onNavigate: vi.fn() }
        const view = render(<Wizard {...props} snapshot={{ ...fxHome(), setup }} />)
        const secret = screen.getByLabelText('Client secret') as HTMLInputElement
        await user.type(secret, 'synthetic-never-submitted-secret')
        view.rerender(<Wizard {...props} snapshot={{ ...fxHome(), setup: { ...setup, context: 'cx-another-draft-context' } }} />)
        expect(secret.value).toBe('')
        expect(props.dispatch).not.toHaveBeenCalled()
    })
})
