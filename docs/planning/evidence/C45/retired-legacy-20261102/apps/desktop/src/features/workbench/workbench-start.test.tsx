/**
 * The API Client's no-collection state (C14, D057): a refused Open or New
 * collection is shown in a callout that stays until the next action that
 * succeeds (a cancelled sheet keeps it), and a folder that is not a
 * collection offers New collection… from the callout.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import { DesktopClient, type Intent, type ShellSnapshot, type WorkbenchRequest, type WorkbenchResult } from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative } from '../../test/native'

const NOT_COLLECTION =
    "That folder isn't empty and has no API Client collection (collection.toml). Choose an empty folder or a collection folder."

const start: ShellSnapshot = { ...baseSnapshot, workbench: { ...baseSnapshot.workbench, loaded: true } }

/** Records API Client requests and answers them in order from `answers`. */
function workbench(native: Native, answers: WorkbenchResult[]): WorkbenchRequest[] {
    const requests: WorkbenchRequest[] = []
    const pending = new Map<string, WorkbenchRequest>()
    const respond = native.respond
    native.respond = (intent: Intent) => {
        if (intent.kind !== 'workbench') return respond(intent)
        requests.push(intent.request)
        const operation = `op-${requests.length}`
        pending.set(operation, intent.request)
        return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${requests.length}`, operation } }
    }
    native.observeWorkbench = async (operation) => {
        if (!pending.has(operation)) return { kind: 'refusal', refusal: 'invalidInput' }
        return { kind: 'workbench', result: answers.shift() ?? { kind: 'done' } }
    }
    return requests
}

async function open(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^API Client/ }))
    await screen.findByText('No collection is open.')
    return user
}

const callout = () => document.querySelector<HTMLElement>('[data-slot="workbench-message"]')

describe('API Client without a collection', () => {
    it('keeps a not-a-collection refusal visible and offers New collection…', async () => {
        const native = scriptedNative(start)
        const requests = workbench(native, [
            { kind: 'refused', problem: { kind: 'project', detail: 'notCollection' } },
            { kind: 'cancelled' },
            { kind: 'refused', problem: { kind: 'project', detail: 'exists' } },
        ])
        const user = await open(native)
        await user.click(screen.getByRole('button', { name: 'Open collection…' }))
        const alert = await screen.findByRole('alert')
        expect(alert).toHaveTextContent(NOT_COLLECTION)
        expect(alert).toBe(callout())
        // A cancelled sheet leaves the refusal in place.
        await user.click(within(alert).getByRole('button', { name: 'New collection…' }))
        await waitFor(() => expect(requests.map((r) => r.kind)).toEqual(['openFolder', 'createCollection']))
        expect(callout()).toHaveTextContent(NOT_COLLECTION)
        // Another refusal replaces it, without the offer.
        await user.click(within(callout()!).getByRole('button', { name: 'New collection…' }))
        await waitFor(() => expect(callout()).toHaveTextContent('Something already exists there; nothing was replaced.'))
        expect(within(callout()!).queryByRole('button', { name: 'New collection…' })).toBeNull()
        expect(requests.map((r) => r.kind)).toEqual(['openFolder', 'createCollection', 'createCollection'])
    })

    it('clears the refusal after an action that succeeds', async () => {
        const native = scriptedNative(start)
        workbench(native, [{ kind: 'refused', problem: { kind: 'project', detail: 'notCollection' } }, { kind: 'done' }])
        const user = await open(native)
        await user.click(screen.getByRole('button', { name: 'Open collection…' }))
        await waitFor(() => expect(callout()).toHaveTextContent(NOT_COLLECTION))
        await user.click(screen.getByRole('button', { name: 'Import v1 API Client profile…' }))
        await waitFor(() => expect(callout()).toBeNull())
    })
})
