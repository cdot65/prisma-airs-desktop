import type {
    ContextRef,
    ManagementFailure,
    ManagementRequest,
    ManagementResource,
    ManagementResult,
} from '@prisma-airs/desktop-contracts'
import { RefusedError } from '@prisma-airs/desktop-contracts'
import { type Dispatch, outcomeMessage } from '../tenants/dispatch'

/** Waits for the single native result of an admitted management read. */
export type ObserveManagement = (operation: string) => Promise<ManagementResult>

/** What one management read produced, as the presentation shows it. */
export type ReadOutcome =
    | { kind: 'result'; result: ManagementResult }
    /** Not admitted (refused, unconfirmed or undelivered): nothing was read. */
    | { kind: 'notAdmitted'; message: string }

/**
 * Dispatches one management read for `context` and waits for its single
 * observation. A read whose context retires while it runs settles as
 * `cancelled`; it is never shown as data.
 */
export async function readManagement(
    dispatch: Dispatch,
    observe: ObserveManagement,
    context: ContextRef,
    request: ManagementRequest
): Promise<ReadOutcome> {
    const outcome = await dispatch({ kind: 'management', context, request })
    if (outcome.kind !== 'admitted' || outcome.operation === null) {
        return { kind: 'notAdmitted', message: outcomeMessage(outcome) ?? 'The read was not started.' }
    }
    try {
        return { kind: 'result', result: await observe(outcome.operation) }
    } catch (error) {
        if (error instanceof RefusedError) return { kind: 'notAdmitted', message: outcomeMessage({ kind: 'refused', refusal: error.refusal }) ?? '' }
        return { kind: 'notAdmitted', message: 'The desktop did not return the result of this read.' }
    }
}

/** Sanitized operator text for each management failure (no server text). */
export const MANAGEMENT_FAILURE_TEXT: Record<ManagementFailure, string> = {
    unauthorized: 'The management client was not accepted. Check the client ID and secret in Settings.',
    denied: 'This management client is not allowed to read this. Ask your administrator for the needed role.',
    notFound: 'The service did not find this resource.',
    rateLimited: 'The service is limiting requests. Wait a moment, then refresh.',
    unavailable: 'The management service is unavailable. Try again later.',
    rejected: 'The service rejected the request.',
    timeout: 'The read took too long and was stopped.',
    cancelled: 'The read was stopped because the tenant or its management credential changed.',
    network: 'The management service could not be reached.',
    destination: 'The service tried to send the read somewhere else. Nothing was followed.',
    invalidResponse: 'The service returned a reply the desktop could not use.',
    credentialLocked: 'The Keychain is locked. Unlock it and try again.',
    credentialDenied: 'macOS did not allow access to the management secret.',
    credentialMissing: 'The management secret is missing. Enter it again in Settings.',
    configuration: 'The management connection could not be set up for this tenant.',
}

export interface ResourceInfo {
    id: ManagementResource
    label: string
    group: 'AI Runtime Security' | 'Data Loss Prevention'
    /** Fields tried in order for a row's display name. */
    nameFields: readonly string[]
    /** The service's identity field (detail reads look items up by it). */
    idField: string
    /** Whether a detail read can address an item of this resource. */
    detail: boolean
}

export const RESOURCES: readonly ResourceInfo[] = [
    { id: 'profiles', label: 'Security profiles', group: 'AI Runtime Security', nameFields: ['profile_name'], idField: 'profile_id', detail: true },
    { id: 'topics', label: 'Custom topics', group: 'AI Runtime Security', nameFields: ['topic_name'], idField: 'topic_id', detail: true },
    { id: 'applications', label: 'Applications', group: 'AI Runtime Security', nameFields: ['app_name', 'customer_app_name'], idField: 'customer_appId', detail: true },
    { id: 'apiKeys', label: 'API keys', group: 'AI Runtime Security', nameFields: ['api_key_name'], idField: 'api_key_id', detail: true },
    { id: 'deploymentProfiles', label: 'Deployment profiles', group: 'AI Runtime Security', nameFields: ['dp_name'], idField: 'dp_name', detail: true },
    { id: 'dlpCatalog', label: 'DLP profiles (AIRS)', group: 'Data Loss Prevention', nameFields: ['name'], idField: 'id', detail: true },
    { id: 'dataProfiles', label: 'Data profiles', group: 'Data Loss Prevention', nameFields: ['name'], idField: 'id', detail: true },
    { id: 'dataPatterns', label: 'Data patterns', group: 'Data Loss Prevention', nameFields: ['name'], idField: 'id', detail: true },
    { id: 'filteringProfiles', label: 'Data filtering profiles', group: 'Data Loss Prevention', nameFields: ['name'], idField: 'id', detail: true },
    { id: 'dictionaries', label: 'Dictionaries', group: 'Data Loss Prevention', nameFields: ['name'], idField: 'id', detail: true },
]

/** A scalar rendered as plain text; nested values as compact JSON text. */
export function fieldText(value: unknown): string {
    if (value === null || value === undefined) return '—'
    if (typeof value === 'string') return value
    if (typeof value === 'number' || typeof value === 'boolean') return String(value)
    return JSON.stringify(value)
}

export function itemName(info: ResourceInfo, item: Record<string, unknown>): string {
    for (const field of info.nameFields) {
        const value = item[field]
        if (typeof value === 'string' && value.length > 0) return value
    }
    return fieldText(item[info.idField])
}

export function itemId(info: ResourceInfo, item: Record<string, unknown>): string | null {
    const value = item[info.idField]
    if (typeof value === 'string' && value.length > 0) return value
    if (typeof value === 'number') return String(value)
    return null
}
