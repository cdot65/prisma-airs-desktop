import type {
    Outcome,
    ProfileSelector,
    ScanDraft,
    ScanFailureKind,
    ScanRecordView,
    StopReason,
    Verdict,
} from '@prisma-airs/desktop-contracts'

/** Per-text ceiling (UTF-8 bytes), matching the native admission check. */
export const MAX_TEXT_BYTES = 2 * 1024 * 1024
/** Whole-request ceiling (escaped JSON bytes plus envelope allowance). */
export const MAX_REQUEST_BYTES = 8 * 1024 * 1024
const ENVELOPE_BYTES = 16 * 1024
const MAX_PROFILE_NAME = 100
const MAX_APP_NAME = 256
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

const CONTROL = /[\u0000-\u001f\u007f-\u009f]/
const encoder = new TextEncoder()
const bytes = (text: string): number => encoder.encode(text).length
const escapedBytes = (text: string): number => bytes(JSON.stringify(text))

export interface ScanInput {
    profileKind: 'name' | 'id'
    profile: string
    prompt: string
    response: string
    appName: string
}

/**
 * Builds the draft from input captured at submission, or explains why it
 * cannot be sent. The native side re-checks every limit at admission.
 */
export function draftFrom(input: ScanInput): { draft: ScanDraft } | { problem: string; field: keyof ScanInput } {
    const profile = input.profile.trim()
    if (profile.length === 0) return { problem: 'Choose the AI security profile for this scan.', field: 'profile' }
    let selector: ProfileSelector
    if (input.profileKind === 'name') {
        if (profile.length > MAX_PROFILE_NAME || CONTROL.test(profile)) {
            return { problem: `Profile names are at most ${MAX_PROFILE_NAME} characters, on one line.`, field: 'profile' }
        }
        selector = { kind: 'name', name: profile }
    } else {
        if (!UUID.test(profile)) return { problem: 'A profile ID is a UUID, for example 1b2c…-….', field: 'profile' }
        selector = { kind: 'id', id: profile.toLowerCase() }
    }
    if (input.prompt.trim().length === 0 && input.response.trim().length === 0) {
        return { problem: 'Enter a prompt, a response, or both.', field: 'prompt' }
    }
    if (bytes(input.prompt) > MAX_TEXT_BYTES) return { problem: 'The prompt is larger than 2 MiB.', field: 'prompt' }
    if (bytes(input.response) > MAX_TEXT_BYTES) return { problem: 'The response is larger than 2 MiB.', field: 'response' }
    const appName = input.appName.trim()
    if ([...appName].length > MAX_APP_NAME || CONTROL.test(appName)) {
        return { problem: `Application names are at most ${MAX_APP_NAME} characters, on one line.`, field: 'appName' }
    }
    const request = escapedBytes(input.prompt) + escapedBytes(input.response) + escapedBytes(profile) + escapedBytes(appName)
    if (request + ENVELOPE_BYTES > MAX_REQUEST_BYTES) {
        return { problem: 'This scan request would be larger than 8 MiB. Shorten the prompt or response.', field: 'prompt' }
    }
    return {
        draft: { profile: selector, prompt: input.prompt, response: input.response, appName: appName.length > 0 ? appName : null },
    }
}

export const VERDICT_TEXT: Record<Verdict, { label: string; tone: 'success' | 'danger' | 'warning' | 'default' }> = {
    allow: { label: 'Allowed', tone: 'success' },
    block: { label: 'Blocked', tone: 'danger' },
    alert: { label: 'Allowed with a threat detected', tone: 'warning' },
    unknown: { label: 'Not determined', tone: 'warning' },
}

export const FAILURE_TEXT: Record<ScanFailureKind, string> = {
    unauthorized: 'The tenant did not accept the scan key (HTTP 401). Check the key in Settings.',
    forbidden: 'The scan key is not allowed to run this scan (HTTP 403).',
    conflict: 'The service reported a conflict (HTTP 409).',
    rateLimited: 'The service is limiting requests (HTTP 429). Wait before scanning again.',
    badRequest: 'The service rejected the request (HTTP 400). Check the profile and input.',
    unprocessable: 'The service could not process the request (HTTP 422). Check the profile and input.',
    serverError: 'The service failed while handling the scan.',
    httpOther: 'The service returned an unexpected status.',
    timeout: 'No reply arrived in time.',
    cancelled: 'The scan was cancelled.',
    network: 'The connection failed before a reply arrived.',
    redirect: 'The service redirected the request; redirects are not followed.',
    malformedResponse: 'The reply could not be read as a scan result.',
    tooLarge: 'The request or reply exceeded the size limit.',
    invalidRequest: 'The scan request was not valid.',
    credentialLocked: 'The Keychain is locked. Unlock it and scan again.',
    credentialDenied: 'macOS did not allow access to the scan key.',
    credentialMissing: 'The scan key is missing. Enter it in Settings.',
    other: 'The scan did not complete.',
}

/** What is known about the remote effect, in operator terms. */
export function outcomeText(outcome: Outcome | null, stop: StopReason | null): string {
    switch (outcome) {
        case null:
            return ''
        case 'completed':
            return 'The service returned a result.'
        case 'rejected':
            return 'The service refused the request; it was not scanned.'
        case 'notSent':
            return stop === 'cancelled' ? 'Cancelled before it was sent. Nothing reached the service.' : 'Nothing was sent to the service.'
        // Unknown never claims the request was sent: a connect failure or a
        // cancellation may or may not have reached the service.
        case 'unknown':
            return stop === 'cancelled'
                ? 'Cancelled. The request may already have reached the service and been scanned; no result will be shown.'
                : 'The outcome is unknown. The request may or may not have reached the service, and it may have been scanned. It is not resubmitted automatically.'
    }
}

export function recordSummary(record: ScanRecordView): string {
    if (record.progress === 'queued') return 'Waiting to send…'
    if (record.progress === 'running') return 'Scanning…'
    if (record.discarded || record.stop === 'contextRetired') {
        return 'The tenant or scan key changed before the reply. Any reply was discarded and is not shown.'
    }
    if (record.result) return VERDICT_TEXT[record.result.verdict].label
    if (record.failure) return FAILURE_TEXT[record.failure.kind]
    return outcomeText(record.outcome, record.stop)
}
