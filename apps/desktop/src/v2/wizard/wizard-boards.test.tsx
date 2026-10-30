/**
 * Every R2Onboard board (boards.md) renders from its fixture with the
 * reviewed state visible: the wizard opened on the step, over the setup
 * draft the core would hold, through the real contract shape.
 */
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { Wizard } from './wizard'
import type { Dispatch } from '../../features/tenants/dispatch'
import { DRAFT_CONTEXT, fxAddTenant, fxAddTenantDrafted, fxOn, type WizardBoard } from './fixtures'

function mount(board: WizardBoard) {
    const dispatch = vi.fn<Dispatch>(async () => ({ kind: 'admitted', operation: null }))
    const onClose = vi.fn()
    render(<Wizard snapshot={board.snapshot} dispatch={dispatch} observeManagement={async () => ({ kind: 'verified' as const })} mode={board.mode} onClose={onClose} onNavigate={() => undefined} initial={board.initial} />)
    return { dispatch, onClose }
}
const current = () => screen.getByRole('navigation', { name: 'Setup steps' }).querySelector('[aria-current="step"]')?.textContent

describe('R2Onboard boards', () => {
    it('R2On01 welcome: three cards and the API-builder framing, no shell, no Back', () => {
        mount(fxOn.welcome())
        expect(screen.getByRole('heading', { level: 1, name: 'Welcome to Prisma AIRS Desktop' })).toBeInTheDocument()
        expect(within(screen.getByRole('main')).getAllByRole('listitem')).toHaveLength(3)
        expect(screen.queryByRole('navigation', { name: 'Destinations' })).toBeNull()
        expect(screen.queryByRole('button', { name: 'Back' })).toBeNull()
    })
    it('R2On02 tenant empty: nothing drafted, Continue names what is missing, no context line', () => {
        mount(fxOn.tenantEmpty())
        expect(current()).toBe('2Tenant')
        expect(screen.getByRole('button', { name: 'Create tenant' })).toBeEnabled()
        expect(screen.getByRole('button', { name: 'Create the tenant to continue' })).toBeDisabled()
        expect(screen.queryByLabelText('Scan API key', { selector: 'input' })).toBeNull()
        expect(document.querySelector('[data-slot="rail-context"]')).toBeNull()
    })
    it('R2On02b tenant filled: the context line, the editable profile, credential controls and Continue', () => {
        mount(fxOn.tenantFilled())
        expect(document.querySelector('[data-slot="rail-context"]')).toHaveTextContent('Northwind Health · Staging · TSG 1762204418')
        expect(screen.getByLabelText(/Display name/)).toHaveValue('Northwind Health')
        expect(within(screen.getByRole('radiogroup', { name: 'Environment' })).getByRole('radio', { name: 'Staging' })).toBeChecked()
        expect(screen.getByRole('button', { name: 'Save changes' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Replace scan key' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Remove scan key' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Save management client' })).toBeInTheDocument()
        expect(screen.getByRole('switch', { name: 'Set up an AI Gateway workspace now' })).not.toBeChecked()
        expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled()
    })
    it('R2On03 products: Runtime always on, AIS and AES not selectable, reads-with state per card', () => {
        mount(fxOn.products())
        const boxes = screen.getAllByRole('checkbox')
        expect(boxes.map((b) => b.getAttribute('aria-disabled') === 'true')).toEqual([true, false, false, false, true, true])
        expect(screen.getByRole('checkbox', { name: /AI Gateway/ })).toBeChecked()
        expect(screen.getByText('Always on')).toBeInTheDocument()
        expect(screen.getByText('Scan key saved')).toBeInTheDocument()
        expect(screen.getAllByText('Needs the management client')).toHaveLength(3)
        expect(screen.getByText(/Licence state is not readable through the public APIs/)).toBeInTheDocument()
    })
    it('R2On04 verify running: the list is busy and Continue waits', () => {
        mount(fxOn.verifyRunning())
        expect(screen.getByRole('list', { name: 'Checks' })).toHaveAttribute('aria-busy', 'true')
        expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled()
        expect(within(document.querySelector('[data-slot="verify-gateway"]') as HTMLElement).getByText('Pending')).toBeInTheDocument()
        expect(screen.getByText('Checking…')).toBeInTheDocument()
    })
    it('R2On04b verify warning: Continue with warnings is offered because the core says the run is completable', () => {
        mount(fxOn.verifyWarn())
        expect(screen.getByText('Checks finished with warnings.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue with warnings' })).toBeEnabled()
        expect(screen.getByRole('button', { name: 'Verify again' })).toBeEnabled()
    })
    it('R2On04c verify fail (required): Continue is disabled; Fix and verify again returns to Tenant with the profile and credential controls', async () => {
        const user = userEvent.setup()
        mount(fxOn.verifyFail())
        expect(screen.getByText('A required check failed.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled()
        expect(screen.queryByRole('button', { name: 'Continue with warnings' })).toBeNull()
        expect(screen.getByRole('button', { name: 'Verify again' })).toBeEnabled()
        expect(within(document.querySelector('[data-slot="verify-ais"]') as HTMLElement).getByText('Not available')).toBeInTheDocument()
        expect(within(document.querySelector('[data-slot="verify-aes"]') as HTMLElement).getByText('Not available')).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Fix and verify again' }))
        expect(screen.getByRole('heading', { level: 1, name: 'Connect your tenant' })).toHaveFocus()
        expect(screen.getByLabelText(/TSG ID/)).toHaveValue('1762204418')
        expect(screen.getByRole('button', { name: 'Replace management client' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Remove management client' })).toBeInTheDocument()
        expect(screen.getByText('Client ID: svc@1762204418.iam.example', { exact: false })).toBeInTheDocument()
    })
    it('R2On04c-opt optional failure: the operator may fix it or continue with it recorded as a warning', async () => {
        const user = userEvent.setup()
        const { dispatch } = mount(fxOn.verifyOptionalFail())
        expect(screen.getByText(/An optional check failed/)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Fix and verify again' })).toBeEnabled()
        expect(screen.getByRole('button', { name: 'Verify again' })).toBeEnabled()
        await user.click(screen.getByRole('button', { name: 'Continue with warnings' }))
        expect(screen.getByRole('heading', { level: 1, name: 'Load starter test cases' })).toBeInTheDocument()
        expect(dispatch).not.toHaveBeenCalled()
    })
    it('R2On04d verify stale: the run no longer counts and later steps are unreachable', () => {
        mount(fxOn.verifyStale())
        expect(screen.getByText('The inputs changed since this run. Verify again before continuing.')).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled()
        const rail = screen.getByRole('navigation', { name: 'Setup steps' })
        expect(within(rail).getByRole('button', { name: /Done$/ })).toBeDisabled()
    })
    it('R2On05 packs: one checkbox per chosen product with its count; Skip clears the choice', async () => {
        const user = userEvent.setup()
        mount(fxOn.packs())
        expect(screen.getByRole('checkbox', { name: 'AI Runtime Security · 6 cases' })).toBeChecked()
        await user.click(screen.getByRole('button', { name: 'Skip for now' }))
        expect(screen.getByRole('heading', { level: 1, name: 'Join the AI Security Academy' })).toBeInTheDocument()
    })
    it('R2On06 academy: the reviewed account component with sign-in or skip and the sync disclosure', () => {
        mount(fxOn.academy())
        expect(screen.getByRole('button', { name: 'Sign in to Academy' })).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Skip for now' })).toBeInTheDocument()
        expect(screen.getByText(/syncs your Academy progress only/)).toBeInTheDocument()
    })
    it('R2On07 done: the summary matches the core’s draft and Open Prisma AIRS sends one completion', async () => {
        const user = userEvent.setup()
        const { dispatch } = mount(fxOn.done())
        expect(screen.getByRole('heading', { level: 1, name: /is ready$/ })).toBeInTheDocument()
        expect(document.querySelector('[data-slot="summary"]')).toHaveTextContent('Products: AI Runtime Security')
        await user.click(screen.getByRole('button', { name: 'Open Prisma AIRS' }))
        expect(dispatch).toHaveBeenCalledWith({ kind: 'completeTenantSetup', context: DRAFT_CONTEXT, packs: ['runtimeSecurity'], resolution: null })
        expect(screen.getByRole('button', { name: 'Opening…' })).toBeDisabled()
    })
    it('R2AddTenant: no Welcome or Academy step, a 244px rail, Cancel on every step and no Back on the first', () => {
        mount(fxAddTenant())
        const rail = screen.getByRole('navigation', { name: 'Setup steps' })
        expect(within(rail).getAllByRole('button').map((b) => b.textContent)).toEqual(['1Tenant', '2Products', '3Verify', '4Starter casesOptional', '5Done', 'Light appearance'])
        expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument()
        expect(rail).toHaveStyle({ width: '244px' })
        expect(screen.getByText(/Cancel discards this draft and removes any credential it saved/)).toBeInTheDocument()
        expect(screen.queryByRole('button', { name: 'Back' })).toBeNull()
    })
    it('R2AddTenant drafted: the scan key is optional and Continue is open', () => {
        mount(fxAddTenantDrafted())
        expect(screen.getByText(/Optional here; without it scans stay unavailable/)).toBeInTheDocument()
        expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled()
    })
})
