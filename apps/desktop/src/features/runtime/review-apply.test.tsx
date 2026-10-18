/**
 * Production Review/Apply of Runtime changes through the real App,
 * DesktopClient and shell: drafts are captured from the editor when Review
 * and Apply are pressed (an edit made just before Apply is what is sent,
 * never a debounced copy), the editor is held while the native confirmation
 * is open, preview-only profile options are never part of a draft, and a
 * one-time key is revealed only through the native sheet.
 */
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'
import {
    DesktopClient,
    type ChangeResultView,
    type ChangeReviewView,
    type Intent,
    type ManagementResult,
    type ShellSnapshot,
} from '@prisma-airs/desktop-contracts'
import { App } from '../../app'
import { baseSnapshot, type Native, scriptedNative, tenant, withTenant } from '../../test/native'

const CONTEXT = 'cx-aaaaaaaaaaaaaaaaaaaaaaaa'
const TOPIC = { topic_id: '11111111-1111-4111-8111-111111111111', topic_name: 'Finance', description: 'Money talk', revision: 1 }
const ready = (changes: ShellSnapshot['changes'] = { review: null, results: [] }): ShellSnapshot => ({
    ...withTenant(
        baseSnapshot,
        tenant({
            management: {
                state: 'ready',
                mode: 'keychain',
                failure: null,
                clientId: 'svc@1234567890.iam.panserviceaccount.com',
                tokenOrigin: 'https://auth.apps.paloaltonetworks.com',
            },
        }),
        CONTEXT
    ),
    changes,
})

function review(overrides: Partial<ChangeReviewView> = {}): ChangeReviewView {
    return {
        review: 'rv-0011223344556677',
        kind: 'topics',
        action: 'update',
        target: TOPIC.topic_id,
        state: 'ready',
        failure: null,
        before: JSON.stringify(TOPIC),
        after: JSON.stringify({ ...TOPIC, description: 'Money and payments' }),
        sourceKey: null,
        impact: 'Applies only to this tenant. Existing unknown policy fields are preserved when included in the draft.',
        confirmation: 'Finance',
        ...overrides,
    }
}

/** Answers each management read from its request: one topic, no profiles. */
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
            const items = request.resource === 'topics' ? [TOPIC] : []
            return {
                kind: 'management',
                result: {
                    kind: 'page',
                    page: { resource: request.resource, offset: 0, limit: 25, items: JSON.stringify(items), count: items.length, nextOffset: null, complete: true, total: null },
                },
            }
        }
        return {
            kind: 'management',
            result: { kind: 'detail', detail: { resource: 'topics', id: TOPIC.topic_id, found: 'found', item: JSON.stringify(TOPIC) } },
        }
    }
}

async function openTopicEditor(native: Native) {
    const user = userEvent.setup()
    render(<App client={new DesktopClient(native.transport)} />)
    const nav = await screen.findByRole('navigation', { name: 'Features' })
    await user.click(within(nav).getByRole('button', { name: /^Administration/ }))
    await user.click(await screen.findByRole('button', { name: 'Custom topics' }))
    await user.click(await screen.findByRole('button', { name: 'Details for Finance' }))
    await user.click(await screen.findByRole('button', { name: 'Edit…' }))
    return user
}

const editor = () => screen.getByLabelText(/custom topic draft/) as HTMLTextAreaElement
const sentOf = (native: Native, kind: Intent['kind']) => native.dispatched.filter((i) => i.kind === kind)

describe('review and apply', () => {
    it('reviews exactly the editor text and applies the text present when Apply is pressed', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openTopicEditor(native)
        expect(JSON.parse(editor().value)).toEqual(TOPIC)
        const reviewed = JSON.stringify({ ...TOPIC, description: 'Money and payments' })
        await user.clear(editor())
        await user.click(editor())
        await user.paste(reviewed)
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        const [prepare] = sentOf(native, 'reviewChange')
        expect(prepare).toEqual({
            kind: 'reviewChange',
            context: CONTEXT,
            draft: { kind: 'topics', action: 'update', target: TOPIC.topic_id, body: reviewed },
        })
        await native.push(ready({ review: review(), results: [] }))
        expect(screen.getByText(/Applies only to this tenant/)).toBeInTheDocument()
        await user.type(screen.getByLabelText('Type Finance to confirm'), 'Finance')
        // An edit made right before Apply is what Apply sends (fresh capture);
        // Rust refuses it because it differs from the review.
        native.respond = (intent) =>
            intent.kind === 'requestApply'
                ? { outcome: 'refusal', refusal: 'reviewInvalid' }
                : { outcome: 'receipt', receipt: { kind: 'admitted', request: 'rq-x', operation: null } }
        await user.type(editor(), ' ')
        await user.click(screen.getByRole('button', { name: 'Apply…' }))
        const [apply] = sentOf(native, 'requestApply')
        expect(apply).toEqual({
            kind: 'requestApply',
            review: 'rv-0011223344556677',
            draft: { kind: 'topics', action: 'update', target: TOPIC.topic_id, body: `${reviewed} ` },
            confirmation: 'Finance',
        })
        expect(await screen.findByText(/changed since it was reviewed/)).toBeInTheDocument()
    })

    it('holds the editor while the native confirmation is open', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openTopicEditor(native)
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        await native.push(ready({ review: review({ state: 'confirming' }), results: [] }))
        expect(editor()).toHaveAttribute('readonly')
        expect(screen.getByLabelText('Type Finance to confirm')).toHaveAttribute('readonly')
        expect(screen.getByRole('button', { name: 'Apply…' })).toBeDisabled()
        expect(screen.getByRole('button', { name: 'Review again' })).toBeDisabled()
        expect(screen.getByText(/Confirm or cancel in the desktop’s dialog/)).toBeInTheDocument()
        // Typing is refused by the held editor.
        const before = editor().value
        await user.type(editor(), 'x')
        expect(editor().value).toBe(before)
        expect(sentOf(native, 'requestApply')).toEqual([])
    })

    it('never adds preview-only profile options to a draft', async () => {
        const native = scriptedNative(ready())
        const profile = { profile_id: '22222222-2222-4222-8222-222222222222', profile_name: 'Guardrails', policy: {} }
        let op = 0
        native.respond = (intent) => {
            if (intent.kind === 'management') op += 1
            return { outcome: 'receipt', receipt: { kind: 'admitted', request: `rq-${op}`, operation: intent.kind === 'management' ? `op-${op}` : null } }
        }
        native.observeManagement = () => ({
            kind: 'management',
            result:
                op === 1
                    ? { kind: 'page', page: { resource: 'profiles', offset: 0, limit: 25, items: JSON.stringify([profile]), count: 1, nextOffset: null, complete: true, total: null } }
                    : { kind: 'detail', detail: { resource: 'profiles', id: profile.profile_id, found: 'found', item: JSON.stringify(profile) } },
        })
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Administration/ }))
        await user.click(await screen.findByRole('button', { name: 'Details for Guardrails' }))
        await user.click(await screen.findByRole('button', { name: 'Edit…' }))
        expect(screen.getByText(/never sent/)).toBeInTheDocument()
        expect(screen.queryByRole('checkbox')).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        const [prepare] = sentOf(native, 'reviewChange')
        const body = JSON.parse((prepare as Extract<Intent, { kind: 'reviewChange' }>).draft.body) as Record<string, unknown>
        expect(body).toEqual(profile)
        expect(JSON.stringify(native.sent)).not.toMatch(/directional|injection_preview|customized/i)
    })

    it('reviews a delete without a draft and reveals a new key only through the native sheet', async () => {
        const native = scriptedNative(ready())
        service(native)
        const user = await openTopicEditor(native)
        await user.click(screen.getByRole('button', { name: 'Close' }))
        await user.click(screen.getByRole('button', { name: 'Delete…' }))
        expect(screen.queryByLabelText(/custom topic draft/)).toBeNull()
        await user.click(screen.getByRole('button', { name: 'Review change' }))
        expect(sentOf(native, 'reviewChange').at(-1)).toMatchObject({ draft: { action: 'delete', body: '{}' } })
        const result: ChangeResultView = {
            operation: 'op-9',
            kind: 'apiKeys',
            action: 'rotate',
            target: 'key-1',
            name: 'Support key',
            settled: true,
            outcome: 'completed',
            failure: null,
            response: JSON.stringify({ api_key_id: 'key-1', api_key_last8: 'abcd1234' }),
            readback: 'found',
            current: null,
            secret: 'held',
        }
        await native.push(ready({ review: null, results: [result] }))
        expect(screen.getByText(/held in memory only/)).toBeInTheDocument()
        await user.click(screen.getByRole('button', { name: 'Show new key…' }))
        await user.click(screen.getByRole('button', { name: 'Clear key' }))
        expect(sentOf(native, 'revealSecret')).toEqual([{ kind: 'revealSecret', operation: 'op-9' }])
        expect(sentOf(native, 'forgetSecret')).toEqual([{ kind: 'forgetSecret', operation: 'op-9' }])
        await native.push(ready({ review: null, results: [{ ...result, secret: 'forgotten' }] }))
        await waitFor(() => expect(screen.getByText('The new key was cleared from memory.')).toBeInTheDocument())
    })

    it('reports an uncertain outcome without claiming success', async () => {
        const native = scriptedNative(
            ready({
                review: null,
                results: [
                    {
                        operation: 'op-4',
                        kind: 'topics',
                        action: 'update',
                        target: TOPIC.topic_id,
                        name: 'Finance',
                        settled: true,
                        outcome: 'unknown',
                        failure: { kind: 'service', failure: 'network' },
                        response: null,
                        readback: null,
                        current: null,
                        secret: 'none',
                    },
                ],
            })
        )
        const user = userEvent.setup()
        render(<App client={new DesktopClient(native.transport)} />)
        await user.click(within(await screen.findByRole('navigation', { name: 'Features' })).getByRole('button', { name: /^Administration/ }))
        expect(await screen.findByText(/may or may not have been applied/)).toBeInTheDocument()
        expect(screen.queryByText(/^Applied\./)).toBeNull()
    })
})
