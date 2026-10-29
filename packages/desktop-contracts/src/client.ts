/**
 * Typed presentation facade over the two native commands.
 *
 * - Admission calls are serialized: the next delivery is sent only after the
 *   previous admission result arrives, and the sequence advances only then.
 * - A lost admission result is recovered by resending the same bytes with the
 *   same sequence. If it stays lost, the outcome is unknown: the facade
 *   rotates to a new delivery session and never reuses that sequence for a
 *   different intent. It never re-sends business work under a new sequence.
 * - Native responses are validated at runtime; generated declarations alone
 *   do not validate anything.
 */
import type { AcademyCallResult } from './generated/AcademyCallResult'
import type { DispatchResponse } from './generated/DispatchResponse'
import type { Intent } from './generated/Intent'
import type { ManagementResult } from './generated/ManagementResult'
import type { MenuCommand } from './generated/MenuCommand'
import type { ObservationUpdate } from './generated/ObservationUpdate'
import type { Refusal } from './generated/Refusal'
import type { ShellSnapshot } from './generated/ShellSnapshot'
import type { WorkbenchResult } from './generated/WorkbenchResult'

export const PROTOCOL = 1

/** The native boundary. Production binds it to Tauri `invoke`/`Channel`. */
export interface DesktopTransport {
    dispatch(bytes: Uint8Array): Promise<unknown>
    observe(bytes: Uint8Array): Promise<unknown>
    openChannel(onMessage: (update: unknown) => void): {
        token: string
        close(): void
    }
}

/** The admission result for a delivery could not be established. */
export class DeliveryOutcomeUnknown extends Error {
    constructor(readonly intentKind: string) {
        super(
            `The desktop did not confirm whether "${intentKind}" was admitted. ` +
                'Check the current state before trying again.'
        )
        this.name = 'DeliveryOutcomeUnknown'
    }
}

/** The native host returned something outside the wire contract. */
export class ContractViolation extends Error {
    constructor(what: string) {
        super(`Unexpected native response: ${what}`)
        this.name = 'ContractViolation'
    }
}

export class RefusedError extends Error {
    constructor(readonly refusal: Refusal) {
        super(`Refused: ${refusal}`)
        this.name = 'RefusedError'
    }
}

const REFUSALS: readonly Refusal[] = [
    'staleContext',
    'capabilityMissing',
    'invalidInput',
    'reviewInvalid',
    'reviewConsumed',
    'busy',
    'lockedStore',
    'deniedStore',
    'alreadyProcessed',
    'sequenceGap',
    'shuttingDown',
    'decisionPending',
]

const DECIMAL_U64 = /^(0|[1-9][0-9]{0,19})$/
const U64_MAX = 18446744073709551615n

const FEATURE_IDS: readonly string[] = [
    'home',
    'inbox',
    'runtime',
    'academy',
    'administration',
    'sessions',
    'library',
    'redTeam',
    'apiClient',
    'gateway',
    'harness',
    'import',
    'settings',
]
const THEMES: readonly string[] = ['system', 'light', 'dark']
const PRODUCT_IDS: readonly string[] = ['runtimeSecurity', 'gateway', 'redTeam', 'modelSkill']
const ENVIRONMENTS: readonly string[] = ['production', 'staging', 'other']
const SETUP_MODES: readonly string[] = ['firstRun', 'add']
const VERIFY_LINES: readonly string[] = ['scanKey', 'managementClient', 'gateway', 'redTeam', 'modelSkill', 'ais', 'aes']
const VERIFY_STATES: readonly string[] = ['pending', 'pass', 'warning', 'fail', 'stored', 'skipped', 'unavailable']
const UI_VARIANTS: readonly string[] = ['legacy', 'v2']
const ONBOARDING_STATES: readonly string[] = ['needed', 'inProgress', 'done']
const STORAGE: readonly string[] = ['ready', 'corrupt', 'unavailable', 'resetting']
const SCAN_STATES: readonly string[] = ['missing', 'storing', 'ready', 'failed']
const MODES: readonly string[] = ['keychain', 'session']
const VAULT_ERRORS: readonly string[] = ['locked', 'denied', 'unavailable', 'notFound', 'corrupt']
const REGIONS: readonly string[] = ['unitedStates', 'europe', 'india', 'singapore']
const IMPORT_FORMATS: readonly string[] = ['profileDocument', 'airsCliRegistry']
const IMPORT_ERRORS: readonly string[] = ['tooLarge', 'unsupported', 'empty']
const CANDIDATE_PROBLEMS: readonly string[] = ['invalidLabel', 'invalidTsgId', 'invalidDestination']
const TENANT_ID = /^tn-[0-9a-f]{16}$/
const CONTEXT_REF = /^cx-[A-Za-z0-9-]{1,61}$/
const REVIEW_ID = /^rv-[A-Za-z0-9-]{1,61}$/
const OPERATION_ID = /^op-[0-9]{1,20}$/
const LEARNER_REF = /^lx-[A-Za-z0-9-]{1,61}$/
/** A bare HTTPS origin (Academy web, for public links only). */
const ACADEMY_ORIGIN = /^https:\/\/[A-Za-z0-9.-]{1,253}(:\d{1,5})?$/
const ACADEMY_FAILURES: readonly string[] = [
    'notConfigured',
    'invalidConfig',
    'network',
    'invalidResponse',
    'denied',
    'expired',
    'sessionExpired',
    'scopeMissing',
    'accountChanged',
    'authFailed',
]
const CALL_FAILURES: readonly string[] = [
    'network',
    'timeout',
    'invalidResponse',
    'sessionExpired',
    'credentialUnavailable',
    'cancelled',
    'hostUnavailable',
    'unsupportedImage',
    'destinationRefused',
    'uploadFailed',
    'offlineUnavailable',
    'notDownloadable',
    'offlineFull',
]
const SCAN_PROGRESS: readonly string[] = ['queued', 'running', 'settled']
const OUTCOMES: readonly string[] = ['notSent', 'rejected', 'completed', 'unknown']
const STOP_REASONS: readonly string[] = ['cancelled', 'contextRetired', 'deadline', 'shuttingDown']
const VERDICTS: readonly string[] = ['allow', 'block', 'alert', 'unknown']
const SCAN_FAILURES: readonly string[] = [
    'unauthorized',
    'forbidden',
    'conflict',
    'rateLimited',
    'badRequest',
    'unprocessable',
    'serverError',
    'httpOther',
    'timeout',
    'cancelled',
    'network',
    'redirect',
    'malformedResponse',
    'tooLarge',
    'invalidRequest',
    'credentialLocked',
    'credentialDenied',
    'credentialMissing',
    'other',
]

const MANAGEMENT_RESOURCES: readonly string[] = [
    'sessions',
    'profiles',
    'topics',
    'applications',
    'apiKeys',
    'deploymentProfiles',
    'dlpCatalog',
    'dataProfiles',
    'dataPatterns',
    'filteringProfiles',
    'dictionaries',
]
const MANAGEMENT_FAILURES: readonly string[] = [
    'unauthorized',
    'denied',
    'notFound',
    'rateLimited',
    'unavailable',
    'rejected',
    'timeout',
    'cancelled',
    'network',
    'destination',
    'invalidResponse',
    'credentialLocked',
    'credentialDenied',
    'credentialMissing',
    'configuration',
]
const DETAIL_FOUND: readonly string[] = ['found', 'absent', 'incomplete']
const CHANGE_KINDS: readonly string[] = [
    'profiles',
    'topics',
    'applications',
    'apiKeys',
    'dataPatterns',
    'dataProfiles',
    'filteringProfiles',
    'dictionaries',
    'gatewayWorkspaces',
    'gatewayProviders',
    'gatewayRoutes',
    'gatewayGuardrails',
    'gatewayServiceKeys',
    'gatewayUserKeys',
    'gatewayIntegrations',
]
const CHANGE_ACTIONS: readonly string[] = ['create', 'update', 'delete', 'version', 'rotate', 'models']
const CHANGE_PROBLEMS: readonly string[] = [
    'unsupported',
    'target',
    'body',
    'contract',
    'description',
    'rotation',
    'keyFields',
    'applicationExists',
    'keyExists',
    'inventoryLimit',
    'deploymentProfile',
    'sourceMismatch',
    'notFound',
    'dataProfile',
    'dictionaryFile',
    'fileName',
    'stagedFile',
    'previewOnly',
    'credentialPolicy',
    'protectedConfiguration',
    'workspaceMismatch',
    'configuration',
    'scopeExists',
    'scopeMissing',
    'referenced',
]
const STAGED_STATES: readonly string[] = ['choosing', 'ready', 'failed']
const STAGED_FAILURES: readonly string[] = ['tooLarge', 'notText', 'empty', 'unreadable']
const STAGED_TOKEN = /^sf-[0-9a-f]{16}$/
const REVIEW_STATES: readonly string[] = ['preparing', 'ready', 'confirming', 'expired', 'failed']
const READBACKS: readonly string[] = ['found', 'absent', 'incomplete', 'failed', 'differs', 'stopped']
const STEP_STATES: readonly string[] = ['notStarted', 'notSent', 'rejected', 'unknown', 'completed', 'existing', 'deleted']
const RECONCILED: readonly string[] = ['found', 'absent', 'incomplete', 'failed', 'ambiguous']
const SCOPE_CLEANUPS: readonly string[] = ['notNeeded', 'retained', 'deleted', 'failed', 'unknown']
export const GATEWAY_DRAFT_PROBLEMS: readonly string[] = ['unsupported', 'tooLarge', 'credential', 'incomplete']
const GATEWAY_LOCAL_ID = /^g[drpif]-[0-9a-f]{24}$/
/** A file chosen to upload is named by a token, never by its path. */
const FILE_TOKEN = /^gf-[0-9a-f]{24}$/
const SECRET_STATES: readonly string[] = ['none', 'held', 'forgotten', 'shown']
/** Largest page the native side reads (`management::MAX_PAGE`). */
const MANAGEMENT_MAX_PAGE = 100

const oneOf = (allowed: readonly string[], value: unknown): boolean =>
    typeof value === 'string' && allowed.includes(value)
const optional = (check: (v: unknown) => boolean, value: unknown): boolean =>
    value === null || check(value)
const isString = (value: unknown): boolean => typeof value === 'string'
const isCount = (value: unknown): boolean => Number.isSafeInteger(value) && (value as number) >= 0
const RESET_STAGES: readonly string[] = ['credentials', 'document']

function isDestination(value: unknown): boolean {
    if (!isRecord(value)) return false
    if (value.kind === 'region') return oneOf(REGIONS, value.region)
    return value.kind === 'custom' && typeof value.origin === 'string' && value.origin.startsWith('https://')
}

function isManagementView(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(SCAN_STATES, value.state) &&
        optional((m) => oneOf(MODES, m), value.mode) &&
        optional((f) => oneOf(VAULT_ERRORS, f), value.failure) &&
        optional(isString, value.clientId) &&
        optional((o) => typeof o === 'string' && o.startsWith('https://'), value.tokenOrigin)
    )
}

/** The workspace inference key's state and binding (C18; never the key). */
function isInferenceKeyView(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(SCAN_STATES, value.state) &&
        optional((m) => oneOf(MODES, m), value.mode) &&
        optional((f) => oneOf(VAULT_ERRORS, f), value.failure) &&
        optional(
            (t) =>
                isRecord(t) &&
                isListedWorkspace(t.workspace) &&
                typeof t.endpoint === 'string' &&
                t.endpoint.startsWith('https://') &&
                !t.endpoint.includes('?') &&
                !t.endpoint.includes('#'),
            value.target
        ) &&
        // A ready key is always bound to a workspace and endpoint.
        (value.state !== 'ready' || value.target !== null)
    )
}

function isTenant(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        TENANT_ID.test(value.id) &&
        // C29: the environment chosen at setup.
        oneOf(ENVIRONMENTS, value.environment) &&
        isString(value.label) &&
        optional(isString, value.tsgId) &&
        isDestination(value.destination) &&
        isString(value.origin) &&
        isRecord(value.scan) &&
        oneOf(SCAN_STATES, value.scan.state) &&
        optional((m) => oneOf(MODES, m), value.scan.mode) &&
        optional((f) => oneOf(VAULT_ERRORS, f), value.scan.failure) &&
        isManagementView(value.management) &&
        isInferenceKeyView(value.inference) &&
        // C29: the products a tenant reads; Runtime Security is always first.
        Array.isArray(value.products) &&
        value.products.length >= 1 &&
        value.products[0] === 'runtimeSecurity' &&
        value.products.every((p: unknown) => typeof p === 'string' && PRODUCT_IDS.includes(p)) &&
        new Set(value.products as string[]).size === value.products.length &&
        // C29: verify warnings recorded at completion (Inbox items in C30).
        Array.isArray(value.setupWarnings) &&
        value.setupWarnings.every(isSetupWarning)
    )
}

function isProductList(value: unknown): boolean {
    return (
        Array.isArray(value) &&
        value.length >= 1 &&
        value[0] === 'runtimeSecurity' &&
        value.every((p: unknown) => typeof p === 'string' && PRODUCT_IDS.includes(p)) &&
        new Set(value as string[]).size === value.length
    )
}

/** The setup in progress (C29): a core-owned draft, never listed as a tenant. */
function isSetupView(value: unknown): boolean {
    if (
        !isRecord(value) ||
        !oneOf(SETUP_MODES, value.mode) ||
        typeof value.context !== 'string' ||
        !CONTEXT_REF.test(value.context) ||
        !isTenant(value.tenant) ||
        !isProductList(value.products) ||
        typeof value.scanRequired !== 'boolean' ||
        !optional((w) => isRecord(w) && typeof w.reference === 'string' && w.reference.length > 0 && isListedWorkspace(w.workspace) && typeof w.restored === 'boolean', value.workspace) ||
        !Array.isArray(value.packs) ||
        !value.packs.every((p: unknown) => isRecord(p) && oneOf(PRODUCT_IDS, p.product) && typeof p.count === 'number' && Number.isInteger(p.count) && p.count >= 0) ||
        typeof value.completable !== 'boolean'
    ) {
        return false
    }
    const verify = value.verify
    if (verify === null) return true
    return (
        isRecord(verify) &&
        typeof verify.generation === 'number' &&
        Number.isInteger(verify.generation) &&
        verify.generation >= 1 &&
        Array.isArray(verify.steps) &&
        verify.steps.every((s: unknown) => isRecord(s) && oneOf(VERIFY_LINES, s.line) && (s.product === null || oneOf(PRODUCT_IDS, s.product)) && oneOf(VERIFY_STATES, s.state) && typeof s.reason === 'string') &&
        typeof verify.done === 'boolean' &&
        typeof verify.failed === 'boolean' &&
        typeof verify.blocking === 'boolean' &&
        typeof verify.stale === 'boolean' &&
        Array.isArray(verify.warnings) &&
        verify.warnings.every((w: unknown) => isRecord(w) && oneOf(PRODUCT_IDS, w.product) && typeof w.reason === 'string' && w.reason.length > 0)
    )
}

const SETUP_WARNING_ID = /^vw-[0-9]{1,20}$/
function isSetupWarning(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        SETUP_WARNING_ID.test(value.id) &&
        oneOf(PRODUCT_IDS, value.product) &&
        typeof value.reason === 'string' &&
        value.reason.length > 0 &&
        typeof value.recordedSecs === 'number' &&
        Number.isInteger(value.recordedSecs) &&
        value.recordedSecs >= 0
    )
}

function isImportView(value: unknown): boolean {
    if (!isRecord(value)) return false
    switch (value.state) {
        case 'idle':
        case 'choosing':
            return true
        case 'failed':
            return oneOf(IMPORT_ERRORS, value.error)
        case 'review':
            return (
                typeof value.review === 'string' &&
                REVIEW_ID.test(value.review) &&
                oneOf(IMPORT_FORMATS, value.format) &&
                isString(value.fileName) &&
                isRecord(value.discarded) &&
                isCount(value.discarded.secretFields) &&
                isCount(value.discarded.fileReferences) &&
                isCount(value.discarded.otherFields) &&
                Array.isArray(value.candidates) &&
                value.candidates.every(
                    (c) =>
                        isRecord(c) &&
                        isCount(c.index) &&
                        isString(c.label) &&
                        optional(isString, c.tsgId) &&
                        optional(isDestination, c.destination) &&
                        typeof c.defaultDestination === 'boolean' &&
                        typeof c.existingLabel === 'boolean' &&
                        optional((p) => oneOf(CANDIDATE_PROBLEMS, p), c.problem)
                )
            )
        default:
            return false
    }
}

const isOperation = (value: unknown): boolean => typeof value === 'string' && OPERATION_ID.test(value)
const isDetection = (d: unknown): boolean => isRecord(d) && isString(d.detector) && typeof d.detected === 'boolean'

function isScanResult(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.scanId) &&
        isString(value.reportId) &&
        optional(isString, value.trId) &&
        optional(isString, value.profileName) &&
        optional(isString, value.profileId) &&
        isString(value.category) &&
        isString(value.action) &&
        oneOf(VERDICTS, value.verdict) &&
        typeof value.timeout === 'boolean' &&
        typeof value.serviceError === 'boolean' &&
        Array.isArray(value.contentErrors) &&
        value.contentErrors.every(
            (e) =>
                isRecord(e) && optional(isString, e.contentType) && optional(isString, e.feature) && optional(isString, e.status)
        ) &&
        Array.isArray(value.promptDetected) &&
        value.promptDetected.every(isDetection) &&
        Array.isArray(value.responseDetected) &&
        value.responseDetected.every(isDetection) &&
        // C31: topic results keyed apart from the detectors, per direction.
        isTopicVerdict(value.topics) &&
        isTopicVerdict(value.responseTopics)
    )
}

function isTopicVerdict(value: unknown): boolean {
    return (
        isRecord(value) &&
        optional((v) => typeof v === 'boolean', value.violation) &&
        Array.isArray(value.blocked) &&
        value.blocked.every(isString) &&
        Array.isArray(value.allowed) &&
        value.allowed.every(isString)
    )
}

const ADMIN_PRODUCTS: readonly string[] = ['runtimeSecurity', 'gateway', 'redTeam', 'modelSkill', 'agenticIdentity', 'agenticEndpoint']
const CHANGE_KINDS_ALL: readonly string[] = [
    'profiles',
    'topics',
    'applications',
    'apiKeys',
    'dataPatterns',
    'dataProfiles',
    'filteringProfiles',
    'dictionaries',
    'gatewayWorkspaces',
    'gatewayProviders',
    'gatewayRoutes',
    'gatewayGuardrails',
    'gatewayServiceKeys',
    'gatewayUserKeys',
    'gatewayIntegrations',
]
const CHANGE_ACTIONS_ALL: readonly string[] = ['create', 'update', 'delete', 'version', 'rotate', 'models']
const TAB_DISPOSITIONS: readonly string[] = ['read', 'change', 'unavailable', 'removed']
/** The Administration inventory (C31): products, tabs, dispositions and the admitted actions per tab. */
function isAdminInventoryView(value: unknown): boolean {
    if (!isRecord(value) || !Array.isArray(value.products)) return false
    const isPair = (p: unknown) => isRecord(p) && oneOf(CHANGE_KINDS_ALL, p.kind) && oneOf(CHANGE_ACTIONS_ALL, p.action)
    return value.products.every(
        (p: unknown) =>
            isRecord(p) &&
            oneOf(ADMIN_PRODUCTS, p.id) &&
            isString(p.label) &&
            isRecord(p.availability) &&
            (p.availability.state === 'available' || (p.availability.state === 'unavailable' && isString(p.availability.reason))) &&
            typeof p.readOnly === 'boolean' &&
            Array.isArray(p.tabs) &&
            // An unavailable product has no tabs and so no action (V2-T19).
            (p.availability.state === 'available' || p.tabs.length === 0) &&
            p.tabs.every(
                (t: unknown) =>
                    isRecord(t) &&
                    isString(t.id) &&
                    isString(t.label) &&
                    isRecord(t.disposition) &&
                    oneOf(TAB_DISPOSITIONS, t.disposition.kind) &&
                    (t.disposition.kind === 'read' || t.disposition.kind === 'change' || isString(t.disposition.reason)) &&
                    Array.isArray(t.reads) &&
                    t.reads.every(isString) &&
                    Array.isArray(t.actions) &&
                    t.actions.every((a: unknown) => isPair(a) && isString((a as Record<string, unknown>).label) && typeof (a as Record<string, unknown>).row === 'boolean') &&
                    // Only a change tab offers actions; a read-only product never does.
                    (t.disposition.kind === 'change' || t.actions.length === 0) &&
                    (!p.readOnly || t.actions.length === 0) &&
                    Array.isArray(t.denied) &&
                    t.denied.every((d: unknown) => isPair(d) && isString((d as Record<string, unknown>).reason)) &&
                    optional(isString, t.resource)
            )
    )
}

const ROTATION_STAGES: readonly string[] = ['issuanceUnconfirmed', 'issued', 'resumed', 'acknowledged', 'removalUnconfirmed', 'complete']
/** One key rotation (C31, I7): its two labelled timestamps and stage. */
function isRotationView(value: unknown): boolean {
    const isKey = (k: unknown) => isRecord(k) && isString(k.id) && typeof k.name === 'string'
    return (
        isRecord(value) &&
        isString(value.application) &&
        isKey(value.oldKey) &&
        optional(isKey, value.newKey) &&
        oneOf(ROTATION_STAGES, value.stage) &&
        optional(isString, value.issuedBy) &&
        isCount(value.issuedAt) &&
        optional(isCount, value.deploymentAcknowledgedAt) &&
        optional(isCount, value.oldKeyRemovedAt) &&
        optional(isString, value.removalRejected) &&
        typeof value.unsaved === 'boolean' &&
        // Complete means read back gone, with its time; otherwise that time is unset.
        ((value.stage === 'complete') === (value.oldKeyRemovedAt !== null)) &&
        // The held key is offered only while it may be revealed: issued, or
        // issued but not yet confirmed by readback (C31, I6).
        (value.stage === 'issued' || value.stage === 'issuanceUnconfirmed' || value.issuedBy === null) &&
        // Nothing is claimed about a key an unconfirmed issuance has not read back.
        (value.stage !== 'issuanceUnconfirmed' || value.newKey === null)
    )
}

function isScanRecord(value: unknown): boolean {
    if (
        !isRecord(value) ||
        !isOperation(value.operation) ||
        !oneOf(SCAN_PROGRESS, value.progress) ||
        !optional((o) => oneOf(OUTCOMES, o), value.outcome) ||
        !optional((s) => oneOf(STOP_REASONS, s), value.stop) ||
        !optional(isScanResult, value.result) ||
        !optional(
            (f) => isRecord(f) && oneOf(SCAN_FAILURES, f.kind) && optional((s) => Number.isInteger(s) && (s as number) >= 100 && (s as number) <= 599, f.status),
            value.failure
        ) ||
        typeof value.discarded !== 'boolean'
    ) {
        return false
    }
    // Only a completed scan carries a result, and a settled one has an outcome.
    if (value.result !== null && value.outcome !== 'completed') return false
    return (value.progress === 'settled') === (value.outcome !== null)
}

const OFFLINE_STATES: readonly string[] = ['signedOut', 'opening', 'empty', 'ready', 'unavailable']
const DOWNLOAD_STATUSES: readonly string[] = ['current', 'outdated', 'expired', 'withdrawn']
/** A UTC instant as the core writes it (`2026-10-25T12:00:00.000Z`). */
const INSTANT = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/
const isInstant = (value: unknown): boolean => typeof value === 'string' && INSTANT.test(value)
const OFFLINE_ID = /^[A-Za-z0-9_-]{1,128}$/
const isOfflineId = (value: unknown): boolean => typeof value === 'string' && OFFLINE_ID.test(value)

function isOfflineView(value: unknown): boolean {
    if (!isRecord(value) || !oneOf(OFFLINE_STATES, value.state) || typeof value.reachable !== 'boolean') return false
    if (!Array.isArray(value.downloads) || !Array.isArray(value.localDrafts)) return false
    // Signed out or not yet open, nothing of the account is shown.
    const open = value.state === 'ready'
    if (!open && (value.downloads.length > 0 || value.localDrafts.length > 0)) return false
    const courses = new Set<unknown>()
    const downloads = value.downloads.every((d) => {
        if (
            !isRecord(d) ||
            !isOfflineId(d.courseId) ||
            courses.has(d.courseId) ||
            typeof d.title !== 'string' ||
            !isString(d.revision) ||
            !isInstant(d.downloadedAt) ||
            !isInstant(d.leaseExpiresAt) ||
            !oneOf(DOWNLOAD_STATUSES, d.status) ||
            !isCount(d.units) ||
            !optional(isOfflineId, d.firstUnit)
        ) {
            return false
        }
        courses.add(d.courseId)
        return true
    })
    const attempts = new Set<unknown>()
    const drafts = value.localDrafts.every((d) => {
        if (!isRecord(d) || !isOfflineId(d.attemptId) || attempts.has(d.attemptId) || !isCount(d.baseVersion) || !isInstant(d.savedAt)) {
            return false
        }
        attempts.add(d.attemptId)
        return true
    })
    return downloads && drafts
}

/** An Academy account as `desktop.me` names it: its user ID, a UUID (as live
 * Academy does, C25), or a Clerk `user_` identifier (core `is_account_id`). */
const ACADEMY_ACCOUNT = /^(?:[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}|user_[A-Za-z0-9_]{1,123})$/

function isAcademyView(value: unknown): boolean {
    if (!isRecord(value)) return false
    switch (value.state) {
        case 'signedOut':
        case 'starting':
        case 'completing':
            return true
        case 'awaitingApproval':
            return (
                isString(value.userCode) &&
                typeof value.verificationUri === 'string' &&
                value.verificationUri.startsWith('https://') &&
                isCount(value.expiresIn)
            )
        case 'signedIn':
            return (
                isString(value.displayName) &&
                typeof value.accountId === 'string' &&
                ACADEMY_ACCOUNT.test(value.accountId) &&
                typeof value.learner === 'string' &&
                LEARNER_REF.test(value.learner)
            )
        case 'failed':
            return (
                oneOf(ACADEMY_FAILURES, value.failure) &&
                optional((v) => oneOf(VAULT_ERRORS, v), value.vault) &&
                optional(isString, value.displayName)
            )
        default:
            return false
    }
}

/** Validates a learner call result (its tRPC payloads are parsed later). */
export function parseAcademyCallResult(value: unknown): AcademyCallResult {
    if (isRecord(value)) {
        if (value.kind === 'data' && typeof value.data === 'string') return { kind: 'data', data: value.data }
        if (
            value.kind === 'error' &&
            typeof value.error === 'string' &&
            Number.isInteger(value.httpStatus) &&
            oneOf(OUTCOMES, value.outcome)
        ) {
            return value as unknown as AcademyCallResult
        }
        if (value.kind === 'failed' && oneOf(CALL_FAILURES, value.failure) && oneOf(OUTCOMES, value.outcome)) {
            return value as unknown as AcademyCallResult
        }
    }
    throw new ContractViolation('academy call result')
}

/** A JSON array of objects (management items are rendered as data only). */
function isItemArray(value: unknown): boolean {
    if (typeof value !== 'string') return false
    try {
        const items: unknown = JSON.parse(value)
        return Array.isArray(items) && items.every(isRecord)
    } catch {
        return false
    }
}

function isManagementPage(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(MANAGEMENT_RESOURCES, value.resource) &&
        isCount(value.offset) &&
        isCount(value.limit) &&
        (value.limit as number) >= 1 &&
        (value.limit as number) <= MANAGEMENT_MAX_PAGE &&
        isCount(value.count) &&
        (value.count as number) <= (value.limit as number) &&
        isItemArray(value.items) &&
        (JSON.parse(value.items as string) as unknown[]).length === value.count &&
        typeof value.complete === 'boolean' &&
        optional(isCount, value.nextOffset) &&
        // Complete pages have no continuation; incomplete ones advance.
        (value.complete
            ? value.nextOffset === null
            : typeof value.nextOffset === 'number' && value.nextOffset > (value.offset as number)) &&
        optional(isCount, value.total)
    )
}

function isManagementDetail(value: unknown): boolean {
    if (
        !isRecord(value) ||
        !oneOf(MANAGEMENT_RESOURCES, value.resource) ||
        typeof value.id !== 'string' ||
        !oneOf(DETAIL_FOUND, value.found)
    ) {
        return false
    }
    if (value.found !== 'found') return value.item === null
    if (typeof value.item !== 'string') return false
    try {
        return isRecord(JSON.parse(value.item))
    } catch {
        return false
    }
}

const WORKSPACE_REF = /^gw-[A-Za-z0-9-]{1,61}$/
const GATEWAY_ID = /^[A-Za-z0-9_-]{1,128}$/
const GATEWAY_RESOURCES: readonly string[] = [
    'workspaces',
    'providers',
    'integrations',
    'routes',
    'guardrails',
    'serviceKeys',
    'userKeys',
]
const SCOPE_BINDINGS: readonly string[] = ['bound', 'unbound', 'absent', 'denied', 'failed']
const PLANE_ACCESS: readonly string[] = ['granted', 'denied', 'failed']
const GATEWAY_MAX_ROWS = 200

const isGatewayId = (value: unknown): boolean => typeof value === 'string' && GATEWAY_ID.test(value)

function isListedWorkspace(value: unknown): boolean {
    return (
        isRecord(value) &&
        isGatewayId(value.id) &&
        typeof value.name === 'string' &&
        typeof value.slug === 'string' &&
        typeof value.scopeName === 'string'
    )
}

/** A Gateway inventory: at most 200 projected rows, its count and total. */
function isGatewayList(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(GATEWAY_RESOURCES, value.resource) &&
        optional(isGatewayId, value.workspace) &&
        // Scoped inventories name their workspace; tenant-wide ones none.
        (value.workspace === null) === (value.resource === 'workspaces' || value.resource === 'integrations') &&
        isCount(value.count) &&
        (value.count as number) <= GATEWAY_MAX_ROWS &&
        isItemArray(value.rows) &&
        (JSON.parse(value.rows as string) as unknown[]).length === value.count &&
        optional(isCount, value.total) &&
        (value.total === null || (value.total as number) >= (value.count as number)) &&
        typeof value.complete === 'boolean'
    )
}

function isGatewayDetail(value: unknown): boolean {
    if (!isRecord(value) || !oneOf(GATEWAY_RESOURCES, value.resource) || !isGatewayId(value.id) || !oneOf(DETAIL_FOUND, value.found)) {
        return false
    }
    if (value.found !== 'found') return value.item === null
    return isJsonObjectText(value.item)
}

function isGatewayModels(value: unknown): boolean {
    return (
        isRecord(value) &&
        isGatewayId(value.integration) &&
        isCount(value.count) &&
        isItemArray(value.models) &&
        (JSON.parse(value.models as string) as unknown[]).length === value.count &&
        optional(isCount, value.total) &&
        typeof value.complete === 'boolean' &&
        typeof value.allowAll === 'boolean'
    )
}

function isGatewayPrerequisites(value: unknown): boolean {
    if (!isRecord(value) || !isGatewayId(value.workspace) || typeof value.scopeName !== 'string') return false
    // Each source holds its data or its failure, never both.
    const sources = ['routes', 'providers', 'integrations', 'serviceKeys'].every(
        (k) => optional(isGatewayList, value[k]) && optionalFailure(value[`${k}Failure`]) && (value[k] === null) !== (value[`${k}Failure`] === null)
    )
    return (
        sources &&
        isCount(value.inferenceKeys) &&
        oneOf(SCOPE_BINDINGS, value.scope) &&
        oneOf(PLANE_ACCESS, value.dataPlane) &&
        oneOf(PLANE_ACCESS, value.adminPlane) &&
        oneOf(PLANE_ACCESS, value.iamPlane)
    )
}

function isGatewayResult(value: unknown): boolean {
    if (!isRecord(value)) return false
    switch (value.kind) {
        case 'list':
            return isGatewayList(value.list)
        case 'detail':
            return isGatewayDetail(value.detail)
        case 'models':
            return isGatewayModels(value.models)
        case 'prerequisites':
            return isGatewayPrerequisites(value.prerequisites)
        case 'observed':
            return isObserveResult(value.observed)
    }
    return false
}

/** A finite, non-negative measure. */
const isMeasure = (value: unknown): boolean => typeof value === 'number' && Number.isFinite(value) && value >= 0
const optionalMeasure = (value: unknown): boolean => value === null || isMeasure(value)
const GATEWAY_WINDOWS: readonly number[] = [1, 7, 30]
const TRACE_ID = /^[A-Za-z0-9_.:-]{1,128}$/

function isSeries(value: unknown): boolean {
    return (
        isRecord(value) &&
        Array.isArray(value.points) &&
        value.points.length <= 2048 &&
        value.points.every((p) => isRecord(p) && isString(p.x) && isMeasure(p.y)) &&
        optionalMeasure(value.total) &&
        optionalMeasure(value.errors) &&
        typeof value.partial === 'boolean' &&
        // A partial window never claims a total.
        (!value.partial || value.total === null)
    )
}

function isUsage(value: unknown): boolean {
    if (!isRecord(value) || !isGatewayId(value.workspace) || !GATEWAY_WINDOWS.includes(value.days as number)) return false
    const series = ['requests', 'cost', 'tokens', 'latency'].every((k) =>
        optional(isSeries, value[k]) && optionalFailure(value[`${k}Failure`]) && (value[k] === null) !== (value[`${k}Failure`] === null)
    )
    const models = value.models
    const modelsOk =
        models === null ||
        (isRecord(models) &&
            Array.isArray(models.rows) &&
            models.rows.length <= 100 &&
            models.rows.every(
                (m) =>
                    isRecord(m) &&
                    isString(m.model) &&
                    optional(isString, m.provider) &&
                    isMeasure(m.requests) &&
                    optionalMeasure(m.cost) &&
                    optionalMeasure(m.totalTokens) &&
                    optionalMeasure(m.successRate)
            ) &&
            optionalMeasure(models.total) &&
            typeof models.complete === 'boolean' &&
            typeof models.partial === 'boolean')
    return series && modelsOk && optionalFailure(value.modelsFailure) && (models === null) !== (value.modelsFailure === null)
}

function isLogs(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.workspace) &&
        GATEWAY_WINDOWS.includes(value.days as number) &&
        isCount(value.page) &&
        (value.page as number) < 40 &&
        optional((t) => typeof t === 'string' && TRACE_ID.test(t), value.trace) &&
        optional((c) => isCount(c) && (c as number) >= 100 && (c as number) <= 599, value.status) &&
        Array.isArray(value.rows) &&
        value.rows.length <= 25 &&
        value.rows.every(
            (r) =>
                isRecord(r) &&
                isString(r.id) &&
                isString(r.createdAt) &&
                typeof r.model === 'string' &&
                optional(isString, r.provider) &&
                isCount(r.status) &&
                optionalMeasure(r.units) &&
                optionalMeasure(r.cost) &&
                optional((t) => typeof t === 'string' && TRACE_ID.test(t), r.traceId) &&
                optional(isString, r.cacheStatus) &&
                optional(isString, r.apiKeyId) &&
                // A filtered page carries only what was asked for.
                (value.trace === null || r.traceId === value.trace) &&
                (value.status === null || r.status === value.status)
        ) &&
        optionalCount(value.total) &&
        typeof value.more === 'boolean' &&
        typeof value.capped === 'boolean' &&
        // A capped page is the last one read: nothing more is offered.
        !(value.more && value.capped) &&
        typeof value.partial === 'boolean'
    )
}

function isPolicyList(value: unknown): boolean {
    return (
        isRecord(value) &&
        Array.isArray(value.policies) &&
        value.policies.length <= 200 &&
        value.policies.every(
            (p) =>
                isRecord(p) &&
                isString(p.id) &&
                optional(isString, p.name) &&
                isString(p.status) &&
                optional(isString, p.limitType) &&
                Array.isArray(p.groupBy) &&
                p.groupBy.every(isString) &&
                optionalMeasure(p.creditLimit) &&
                optionalMeasure(p.alertThreshold) &&
                optional(isString, p.periodicReset) &&
                optional(isString, p.nextResetAt) &&
                optionalMeasure(p.value) &&
                optional(isString, p.unit) &&
                optional(isString, p.target)
        ) &&
        optionalCount(value.total) &&
        typeof value.complete === 'boolean'
    )
}

function isLimits(value: unknown): boolean {
    return (
        isRecord(value) &&
        isGatewayId(value.workspace) &&
        ['usage', 'rate'].every(
            (k) => optional(isPolicyList, value[k]) && optionalFailure(value[`${k}Failure`]) && (value[k] === null) !== (value[`${k}Failure`] === null)
        )
    )
}

function isObserveResult(value: unknown): boolean {
    if (!isRecord(value)) return false
    switch (value.kind) {
        case 'usage':
            return isUsage(value.usage)
        case 'logs':
            return isLogs(value.logs)
        case 'limits':
            return isLimits(value.limits)
    }
    return false
}

const PLAYGROUND_STATES: readonly string[] = ['running', 'completed', 'incomplete', 'truncated', 'stopped', 'failed']
export const INFERENCE_FAILURES: readonly string[] = [
    'blocked',
    'unauthorized',
    'denied',
    'notFound',
    'rejected',
    'rateLimited',
    'unavailable',
    'timeout',
    'network',
    'destination',
    'invalidResponse',
    'modelFailed',
    'credentialLocked',
    'credentialDenied',
    'credentialMissing',
    'configuration',
]
const RUN_ENDS: readonly string[] = ['completed', 'incomplete', 'truncated', 'stopped', 'failed']
export const INFERENCE_APIS: readonly string[] = ['chat', 'responses']

function isTokenUsage(value: unknown): boolean {
    return isRecord(value) && optionalCount(value.prompt) && optionalCount(value.completion) && optionalCount(value.total)
}

/** The Playground's run (C18): bounded output, its trace and how it ended. */
function isPlaygroundView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.operation === 'string' &&
        OPERATION_ID.test(value.operation) &&
        isListedWorkspace(value.workspace) &&
        oneOf(INFERENCE_APIS, value.api) &&
        isString(value.model) &&
        optional(isGatewayId, value.route) &&
        optional(isGatewayId, value.file) &&
        // Only a Responses run attaches a file.
        (value.file === null || value.api === 'responses') &&
        typeof value.trace === 'string' &&
        TRACE_ID.test(value.trace) &&
        oneOf(PLAYGROUND_STATES, value.state) &&
        typeof value.output === 'string' &&
        value.output.length <= 64 * 1024 &&
        // A running run has no outcome yet; an ended one always does.
        (value.state === 'running' ? value.outcome === null : oneOf(OUTCOMES, value.outcome)) &&
        optional((f) => oneOf(INFERENCE_FAILURES, f), value.failure) &&
        typeof value.sent === 'boolean' &&
        typeof value.answered === 'boolean' &&
        optional(isString, value.servedModel) &&
        optional(isString, value.responseId) &&
        optional(isString, value.finish) &&
        optional(isTokenUsage, value.usage)
    )
}

const FILE_CHANGES: readonly string[] = ['upload', 'delete']
/** Latest file creation time, in seconds, a date can render (8.64e15 ms). */
const MAX_CREATED_AT = 8_640_000_000_000

/** One Gateway file's metadata (C18, D045): never its content. */
function isGatewayFile(value: unknown): boolean {
    return (
        isRecord(value) &&
        isGatewayId(value.id) &&
        isString(value.filename) &&
        isCount(value.bytes) &&
        // Seconds; null when the Gateway named a time no date can render.
        optional((c) => isCount(c) && (c as number) <= MAX_CREATED_AT, value.createdAt) &&
        isString(value.purpose) &&
        isString(value.status) &&
        !('content' in value)
    )
}

/** An upload or delete: going (no outcome yet) or how it settled. */
function isFileAction(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(FILE_CHANGES, value.action) &&
        optional(isString, value.filename) &&
        optional(isGatewayId, value.file) &&
        optional(isCount, value.bytes) &&
        typeof value.trace === 'string' &&
        TRACE_ID.test(value.trace) &&
        optional((o) => oneOf(OUTCOMES, o), value.outcome) &&
        optional((f) => oneOf(INFERENCE_FAILURES, f), value.failure) &&
        typeof value.sent === 'boolean'
    )
}

/** The chosen workspace's files (C18): metadata and a chosen file's name and size only. */
function isGatewayFiles(value: unknown): boolean {
    return (
        isRecord(value) &&
        isListedWorkspace(value.workspace) &&
        optional((f) => Array.isArray(f) && f.length <= 200 && f.every(isGatewayFile), value.files) &&
        typeof value.more === 'boolean' &&
        typeof value.reading === 'boolean' &&
        optional((f) => oneOf(INFERENCE_FAILURES, f), value.readFailure) &&
        optional(
            (c) =>
                isRecord(c) &&
                typeof c.token === 'string' &&
                FILE_TOKEN.test(c.token) &&
                isString(c.name) &&
                isCount(c.bytes) &&
                (c.bytes as number) <= 4 * 1024 * 1024 &&
                Object.keys(c).length === 3,
            value.chosen
        ) &&
        typeof value.choosing === 'boolean' &&
        optional((f) => oneOf(STAGED_FAILURES, f), value.chooseFailure) &&
        optional(isFileAction, value.action)
    )
}

function isGatewayView(value: unknown): boolean {
    return (
        isRecord(value) &&
        optional((w) => Array.isArray(w) && w.every(isListedWorkspace), value.workspaces) &&
        typeof value.complete === 'boolean' &&
        optional(
            (s) =>
                isRecord(s) &&
                typeof s.reference === 'string' &&
                WORKSPACE_REF.test(s.reference) &&
                isListedWorkspace(s.workspace) &&
                typeof s.restored === 'boolean',
            value.selected
        ) &&
        optional(isGatewayId, value.remembered) &&
        typeof value.rememberedMissing === 'boolean' &&
        (!value.rememberedMissing || value.remembered !== null) &&
        isGatewayLocal(value.local) &&
        optional(isPlaygroundView, value.playground) &&
        optional(isGatewayFiles, value.files)
    )
}

export const HARNESS_PHASES: readonly string[] = [
    'starting',
    'ready',
    'answering',
    'interrupting',
    'renewing',
    'confirmWrite',
    'ending',
    'ended',
    'failed',
]
export const HARNESS_FAILURES: readonly string[] = [
    'engineUnavailable',
    'credentialMissing',
    'credentialLocked',
    'credentialDenied',
    'startFailed',
    'bootstrapRejected',
    'timeout',
    'protocol',
    'bounds',
    'exited',
    'refused',
    'turnFailed',
    'cleanupFailed',
    'restoreFailed',
    'signInNeeded',
    'mcpTooLarge',
]
export const SIGN_IN_FAILURES: readonly string[] = [
    'invalidConfig',
    'network',
    'invalidResponse',
    'denied',
    'timedOut',
    'cancelled',
    'browserUnavailable',
    'callbackUnavailable',
    'invalidToken',
    'opaqueToken',
    'wrongWorkspace',
    'differentAccount',
    'sessionExpired',
    'storeFailed',
]
export const HISTORY_FAILURES: readonly string[] = ['locked', 'denied', 'unavailable', 'full', 'tooLarge', 'unreadable', 'missing']
const CREDENTIAL_SOURCES: readonly string[] = ['workspaceKey', 'companySignIn']
const SIGN_IN_STEPS: readonly string[] = ['contacting', 'browser', 'completing']
const NOT_CONTINUABLE: readonly string[] = [
    'running',
    'otherCredential',
    'otherProject',
    'otherMcp',
    'engineUnavailable',
    'workspaceUnavailable',
    'notRestorable',
    'imported',
]
/** Saved conversations per workspace and a title's length (C20). */
const MAX_SAVED_CONVERSATIONS = 16
const MAX_TITLE_CHARS = 80
const HARNESS_STOPS: readonly string[] = ['ended', 'sleep', 'quit', 'deadline']
const TURN_ENDS: readonly string[] = ['completed', 'interrupted', 'truncated', 'failed', 'refused']
const ENTRY_STATES: readonly string[] = ['streaming', 'complete', 'interrupted', 'truncated', 'failed', 'declined']
const HARNESS_ROLES: readonly string[] = ['operator', 'assistant', 'command', 'fileChange', 'tool', 'mcp']
/** Workspace MCP servers (C22): the engine's bounds and the desktop's states. */
const MAX_MCP_SERVERS = 8
const MCP_NAME = /^[a-z][a-z0-9_]{0,31}$/
const MCP_SERVER_ID = /^[A-Za-z0-9-]{1,64}$/
const MCP_STARTUPS: readonly string[] = ['starting', 'ready', 'failed', 'cancelled']
const MCP_LIST_STATES: readonly string[] = ['loading', 'ready', 'failed']
export const MCP_SIGN_IN_FAILURES: readonly string[] = [
    'notOffered',
    'denied',
    'notFound',
    'unavailable',
    'invalidMetadata',
    'registrationNotOffered',
    'network',
    'invalidResponse',
    'timedOut',
    'cancelled',
    'browserUnavailable',
    'callbackUnavailable',
    'sessionExpired',
    'storeFailed',
]
/** What the Harness may propose (C21): Runtime and DLP changes only. */
const PROPOSAL_KINDS: readonly string[] = CHANGE_KINDS.filter((k) => !k.startsWith('gateway'))
const PROPOSAL_STATES: readonly string[] = ['staged', 'inReview', 'closed', 'dismissed']
/** Pending approvals, staged proposals and an approval's texts (C21). */
const MAX_APPROVALS = 8
const MAX_PROPOSALS = 8
/** An approval's command or arguments are shown whole, up to 8 KiB (C21 F3). */
const MAX_APPROVAL_BYTES = 8 * 1024
const APPROVAL_ID = /^[0-9a-f]{24}$/
const PROPOSAL_ID = /^P-[1-8]$/
const CLEANUPS: readonly string[] = ['graceful', 'forced', 'failed']
/** The Harness's bounds (C19): entries, a conversation's text and one answer. */
const MAX_HARNESS_ENTRIES = 200
const MAX_TRANSCRIPT_BYTES = 1024 * 1024
const MAX_ANSWER_BYTES = 256 * 1024
const GIT_COMMIT = /^[0-9a-f]{40}$/

function utf8Bytes(text: string): number {
    return new TextEncoder().encode(text).length
}

function isHarnessEntry(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        value.id.length > 0 &&
        value.id.length <= 300 &&
        oneOf(HARNESS_ROLES, value.role) &&
        typeof value.text === 'string' &&
        utf8Bytes(value.text) <= (value.role === 'operator' ? 32 * 1024 : MAX_ANSWER_BYTES) &&
        oneOf(ENTRY_STATES, value.state) &&
        Object.keys(value).length === 4
    )
}

const IMPORT_CATEGORIES: readonly string[] = [
    'tenants',
    'endpoints',
    'settings',
    'cases',
    'reports',
    'academyDrafts',
    'apiClient',
    'conversations',
]
const IMPORT_PROBLEMS: readonly string[] = [
    'missing',
    'sourceLocked',
    'accessDenied',
    'keychainLocked',
    'corrupt',
    'unsupportedVersion',
    'tooLarge',
    'pathRejected',
    'sourceRetired',
    'noStoredWork',
    'accountMismatch',
    'signInRequired',
    'endpointNotMappable',
    'noV2Equivalent',
    'destinationFull',
    'sourceChanged',
    'noTarget',
    'notRecorded',
]
const IMPORT_SOURCES: readonly string[] = ['notFound', 'locked', 'unavailable', 'ready']
const IMPORT_ACTIVITIES: readonly string[] = ['inspecting', 'planning', 'importing', 'confirming', 'rollingBack']
const RECEIPT_STATES: readonly string[] = ['applying', 'applied', 'rollingBack', 'rolledBack']
const RECEIPT_ITEM_STATES: readonly string[] = [
    'planned',
    'writing',
    'applied',
    'failed',
    'reverting',
    'reverted',
    'keptChanged',
    'gone',
]
const RECEIPT_ID = /^imp-[0-9a-f]{16}$/
/** Bounds of the import view (C23): v1 tenants, plan items and receipts. */
const MAX_IMPORT_TENANTS = 64
const MAX_IMPORT_ITEMS = 256
const MAX_RECEIPTS = 8

const isU32 = (v: unknown): boolean => typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= 0xffffffff

function isItemStatus(value: unknown): boolean {
    if (!isRecord(value)) return false
    if (value.kind === 'blocked') return oneOf(IMPORT_PROBLEMS, value.problem) && Object.keys(value).length === 2
    return oneOf(['ready', 'alreadyPresent', 'conflict'], value.kind) && Object.keys(value).length === 1
}

function isPlanItem(value: unknown): boolean {
    return (
        isRecord(value) &&
        isU32(value.index) &&
        oneOf(IMPORT_CATEGORIES, value.category) &&
        bounded(1024)(value.label) &&
        optional(bounded(1024), value.target) &&
        isItemStatus(value.status) &&
        isRecord(value.counts) &&
        ['new', 'present', 'conflict', 'skipped'].every((k) => isU32((value.counts as Record<string, unknown>)[k])) &&
        Object.keys(value.counts).length === 4 &&
        optional((p) => oneOf(IMPORT_PROBLEMS, p), value.partial)
    )
}

function isReceiptView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        RECEIPT_ID.test(value.id) &&
        isCount(value.created) &&
        oneOf(RECEIPT_STATES, value.state) &&
        typeof value.interrupted === 'boolean' &&
        Array.isArray(value.items) &&
        value.items.length <= MAX_IMPORT_ITEMS &&
        value.items.every(
            (i) =>
                isRecord(i) &&
                oneOf(IMPORT_CATEGORIES, i.category) &&
                bounded(1024)(i.label) &&
                oneOf(RECEIPT_ITEM_STATES, i.state) &&
                isU32(i.written) &&
                isU32(i.kept)
        ) &&
        typeof value.canResume === 'boolean' &&
        typeof value.canRollBack === 'boolean'
    )
}

/** Local redacted diagnostics (C24): a count, the retention, an export and a clearing. */
function isDiagnosticsView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.available === 'boolean' &&
        isU32(value.events) &&
        value.retentionDays === 14 &&
        optional((e) => oneOf(['saving', 'saved', 'cancelled', 'failed'], e), value.export) &&
        optional((c) => oneOf(['cleared', 'failed'], c), value.clear) &&
        Object.keys(value).length === 5
    )
}

/** The reviewed v1 import (C23): what v1 has, the review and receipts. */
function isV1ImportView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.available === 'boolean' &&
        optional((v) => oneOf(IMPORT_SOURCES, v), value.source) &&
        Array.isArray(value.tenants) &&
        value.tenants.length <= MAX_IMPORT_TENANTS &&
        value.tenants.every(
            (t) =>
                isRecord(t) &&
                isU32(t.index) &&
                bounded(1024)(t.label) &&
                optional((v) => typeof v === 'string' && /^[0-9]{1,20}$/.test(v), t.tsgId) &&
                typeof t.sessionOnly === 'boolean' &&
                Array.isArray(t.matches) &&
                t.matches.every((m) => typeof m === 'string' && TENANT_ID.test(m))
        ) &&
        Array.isArray(value.categories) &&
        value.categories.every(
            (c) =>
                isRecord(c) &&
                oneOf(IMPORT_CATEGORIES, c.category) &&
                optional(isU32, c.found) &&
                optional((p) => oneOf(IMPORT_PROBLEMS, p), c.problem)
        ) &&
        Array.isArray(value.notMapped) &&
        value.notMapped.every(bounded(64)) &&
        optional(
            (p) =>
                isRecord(p) &&
                typeof p.review === 'string' &&
                REVIEW_ID.test(p.review) &&
                Array.isArray(p.items) &&
                p.items.length <= MAX_IMPORT_ITEMS &&
                p.items.every(isPlanItem),
            value.plan
        ) &&
        optional((a) => oneOf(IMPORT_ACTIVITIES, a), value.activity) &&
        Array.isArray(value.receipts) &&
        value.receipts.length <= MAX_RECEIPTS &&
        value.receipts.every(isReceiptView) &&
        isStrings(value.damagedReceipts, (id) => typeof id === 'string' && RECEIPT_ID.test(id)) &&
        (value.damagedReceipts as unknown[]).length <= MAX_RECEIPTS &&
        typeof value.receiptsUnreadable === 'boolean' &&
        optional((p) => oneOf(IMPORT_PROBLEMS, p), value.problem)
    )
}

/** The native Harness (C19): the bundled engine and the bound conversation. */
function isHarnessView(value: unknown): boolean {
    if (!isRecord(value) || !isRecord(value.engine)) return false
    const e = value.engine
    const engineOk =
        typeof e.available === 'boolean' &&
        e.protocol === 'airs-desktop/1' &&
        isString(e.runtimeVersion) &&
        typeof e.engineCommit === 'string' &&
        GIT_COMMIT.test(e.engineCommit)
    return (
        engineOk &&
        optional(isConversationView, value.conversation) &&
        optional(isHarnessSignInView, value.signIn) &&
        optional(isHarnessHistoryView, value.history) &&
        optional(isHarnessProjectView, value.project) &&
        optional(isHarnessMcpView, value.mcp) &&
        optional((p) => typeof p === 'string' && p.length > 0 && p.length <= 4096, value.projectRefused)
    )
}

/** The project the Harness works in, and whether it may change it (C21). */
function isHarnessProjectView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.path === 'string' &&
        value.path.startsWith('/') &&
        value.path.length <= 1024 &&
        typeof value.write === 'boolean' &&
        Object.keys(value).length === 2
    )
}

function bounded(max: number) {
    return (v: unknown) => typeof v === 'string' && utf8Bytes(v) <= max + 3
}

/** An engine request for the operator's approval (C21). */
function isHarnessApproval(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        APPROVAL_ID.test(value.id) &&
        oneOf(['command', 'fileChange', 'mcpTool'], value.kind) &&
        // Shown whole: never cut (C21 F3).
        optional((v) => typeof v === 'string' && utf8Bytes(v) <= MAX_APPROVAL_BYTES, value.command) &&
        optional(bounded(1024), value.cwd) &&
        optional(bounded(1024), value.reason) &&
        Array.isArray(value.files) &&
        value.files.length <= 64 &&
        value.files.every((f) => typeof f === 'string' && f.length > 0 && utf8Bytes(f) <= 1024) &&
        // Only a file change names files, and it names every one whole
        // (C21 review R2-F3).
        (value.kind === 'fileChange' ? value.files.length > 0 : value.files.length === 0) &&
        optional((v) => typeof v === 'string' && MCP_NAME.test(v), value.server) &&
        optional(bounded(128), value.tool) &&
        optional((v) => typeof v === 'string' && utf8Bytes(v) <= MAX_APPROVAL_BYTES, value.arguments) &&
        // An MCP tool approval names its server and tool, and only it does.
        (value.kind === 'mcpTool') === (value.server !== null && value.tool !== null) &&
        (value.kind === 'mcpTool' || value.arguments === null) &&
        (value.kind !== 'mcpTool' || (value.command === null && value.cwd === null))
    )
}

/** One workspace MCP server's startup in the running engine (C22). */
function isMcpServerStatus(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.name === 'string' &&
        MCP_NAME.test(value.name) &&
        oneOf(MCP_STARTUPS, value.status) &&
        typeof value.signedOut === 'boolean' &&
        Object.keys(value).length === 3
    )
}

/** One catalog server, as the desktop reads and holds it (C22). */
function isMcpServerView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        value.id.length > 0 &&
        value.id.length <= 256 &&
        isString(value.name) &&
        typeof value.namespace === 'string' &&
        MCP_NAME.test(value.namespace) &&
        typeof value.url === 'string' &&
        value.url.startsWith('https://') &&
        typeof value.status === 'string' &&
        typeof value.supported === 'boolean' &&
        typeof value.enabled === 'boolean' &&
        typeof value.signedIn === 'boolean' &&
        typeof value.stale === 'boolean' &&
        optional((v) => oneOf(MCP_SIGN_IN_FAILURES, v), value.expired) &&
        optional((v) => oneOf(SIGN_IN_STEPS, v), value.step) &&
        optional((v) => oneOf(MCP_SIGN_IN_FAILURES, v), value.failure) &&
        // Only a server the engine can take is on, and a sign-in is current
        // or stale, never both.
        (value.supported || !value.enabled) &&
        !(value.signedIn && value.stale) &&
        // A server ID the engine cannot take is shown, never granted.
        (MCP_SERVER_ID.test(value.id) || !value.supported)
    )
}

/** The chosen workspace's MCP servers (C22). */
function isHarnessMcpView(value: unknown): boolean {
    if (!isRecord(value) || !Array.isArray(value.servers)) return false
    const servers = value.servers
    return (
        typeof value.workspace === 'string' &&
        oneOf(MCP_LIST_STATES, value.state) &&
        optional((f) => oneOf(MANAGEMENT_FAILURES, f), value.failure) &&
        (value.state !== 'failed' || value.failure !== null) &&
        typeof value.complete === 'boolean' &&
        servers.length <= 5 * 100 &&
        servers.every(isMcpServerView) &&
        new Set(servers.map((s) => (s as { id: string }).id)).size === servers.length &&
        new Set(servers.map((s) => (s as { namespace: string }).namespace)).size === servers.length &&
        servers.filter((s) => (s as { enabled: boolean }).enabled).length <= MAX_MCP_SERVERS
    )
}

/** A change the Harness proposed for the operator's own review (C21). */
function isProposalView(value: unknown): boolean {
    if (!isRecord(value) || !isRecord(value.draft)) return false
    const d = value.draft
    return (
        typeof value.id === 'string' &&
        PROPOSAL_ID.test(value.id) &&
        typeof value.summary === 'string' &&
        value.summary.trim().length > 0 &&
        [...value.summary].length <= 512 &&
        oneOf(PROPOSAL_KINDS, d.kind) &&
        oneOf(['create', 'update', 'delete'], d.action) &&
        typeof d.target === 'string' &&
        d.target.length <= 256 &&
        isJsonObjectText(d.body) &&
        d.workspace === undefined &&
        oneOf(PROPOSAL_STATES, value.state) &&
        optional((r) => typeof r === 'string' && REVIEW_ID.test(r), value.review) &&
        // Only a proposal opened into a review names it.
        (value.state === 'inReview' || value.state === 'closed') === (value.review !== null)
    )
}

/** The company sign-in for the Harness Gateway credential (C20). */
function isHarnessSignInView(value: unknown): boolean {
    return (
        isRecord(value) &&
        isListedWorkspace(value.workspace) &&
        typeof value.endpoint === 'string' &&
        value.endpoint.startsWith('https://') &&
        typeof value.issuer === 'string' &&
        value.issuer.startsWith('https://') &&
        isString(value.clientId) &&
        isString(value.audience) &&
        typeof value.selected === 'boolean' &&
        optional(isString, value.account) &&
        optional((v) => oneOf(SIGN_IN_STEPS, v), value.step) &&
        optional((v) => oneOf(SIGN_IN_FAILURES, v), value.failure)
    )
}

function isSavedConversation(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        UUID.test(value.id) &&
        typeof value.title === 'string' &&
        [...value.title].length <= MAX_TITLE_CHARS &&
        isCount(value.updated) &&
        isCount(value.entries) &&
        (value.entries as number) <= MAX_HARNESS_ENTRIES &&
        oneOf(CREDENTIAL_SOURCES, value.source)
    )
}

function isOpenedConversation(value: unknown): boolean {
    if (!isRecord(value)) return false
    const entries = value.entries
    return (
        typeof value.id === 'string' &&
        UUID.test(value.id) &&
        typeof value.title === 'string' &&
        [...value.title].length <= MAX_TITLE_CHARS &&
        isCount(value.updated) &&
        Array.isArray(entries) &&
        entries.length <= MAX_HARNESS_ENTRIES &&
        entries.every(isHarnessEntry) &&
        oneOf(CREDENTIAL_SOURCES, value.source) &&
        optional((v) => oneOf(NOT_CONTINUABLE, v), value.notContinuable)
    )
}

/** The chosen workspace's protected history (C20). */
function isHarnessHistoryView(value: unknown): boolean {
    if (!isRecord(value)) return false
    const conversations = value.conversations
    const unreadable = value.unreadable
    return (
        typeof value.local === 'boolean' &&
        oneOf(['loading', 'ready', 'failed'], value.state) &&
        Array.isArray(conversations) &&
        Array.isArray(unreadable) &&
        conversations.length + unreadable.length <= MAX_SAVED_CONVERSATIONS &&
        conversations.every(isSavedConversation) &&
        unreadable.every((id) => typeof id === 'string' && UUID.test(id)) &&
        optional((v) => oneOf(HISTORY_FAILURES, v), value.failure) &&
        // A failed list names why.
        (value.state !== 'failed' || value.failure !== null) &&
        typeof value.full === 'boolean' &&
        optional(isOpenedConversation, value.opened) &&
        typeof value.busy === 'boolean' &&
        optional((v) => oneOf(HISTORY_FAILURES, v), value.problem) &&
        optional((e) => isExportView(e) && (e as Record<string, unknown>).kind === 'harnessConversation', value.export)
    )
}

function isConversationView(value: unknown): boolean {
    if (!isRecord(value)) return false
    const entries = value.entries
    return (
        typeof value.id === 'string' &&
        OPERATION_ID.test(value.id) &&
        isListedWorkspace(value.workspace) &&
        typeof value.endpoint === 'string' &&
        value.endpoint.startsWith('https://') &&
        oneOf(HARNESS_PHASES, value.phase) &&
        optional((f) => oneOf(HARNESS_FAILURES, f), value.failure) &&
        // A failed conversation names its failure.
        (value.phase !== 'failed' || value.failure !== null) &&
        optional((s) => oneOf(HARNESS_STOPS, s), value.stop) &&
        Array.isArray(entries) &&
        entries.length <= MAX_HARNESS_ENTRIES &&
        entries.every(isHarnessEntry) &&
        entries.reduce((n: number, x) => n + utf8Bytes((x as { text: string }).text), 0) <= MAX_TRANSCRIPT_BYTES &&
        optional((t) => oneOf(TURN_ENDS, t), value.lastTurn) &&
        optional((u) => isRecord(u) && isCount(u.input) && isCount(u.output) && isCount(u.total), value.usage) &&
        optional((c) => oneOf(CLEANUPS, c), value.cleanup) &&
        // An ended conversation says how its engine was stopped.
        (value.phase !== 'ended' || value.cleanup !== null) &&
        optional(isString, value.runtimeVersion) &&
        oneOf(CREDENTIAL_SOURCES, value.source) &&
        typeof value.historyId === 'string' &&
        UUID.test(value.historyId) &&
        optional(isCount, value.savedAt) &&
        optional((v) => oneOf(HISTORY_FAILURES, v), value.saveFailure) &&
        optional((v) => typeof v === 'string' && utf8Bytes(v) <= 32 * 1024, value.unsent) &&
        optional((v) => oneOf(SIGN_IN_FAILURES, v), value.signInFailure) &&
        optional((p) => typeof p === 'string' && p.startsWith('/'), value.project) &&
        typeof value.write === 'boolean' &&
        // Changes are only ever allowed in a project.
        (!value.write || value.project !== null) &&
        Array.isArray(value.approvals) &&
        value.approvals.length <= MAX_APPROVALS &&
        value.approvals.every(isHarnessApproval) &&
        // Commands need a project, and file changes the right to change it.
        value.approvals.every(
            (a) => {
                const kind = (a as Record<string, unknown>).kind
                // MCP tools need no folder; commands and file changes do.
                return kind === 'mcpTool' || (value.project !== null && (value.write || kind === 'command'))
            },
        ) &&
        Array.isArray(value.proposals) &&
        value.proposals.length <= MAX_PROPOSALS &&
        value.proposals.every(isProposalView) &&
        new Set(value.proposals.map((p) => (p as { id: string }).id)).size === value.proposals.length &&
        Array.isArray(value.mcp) &&
        value.mcp.length <= MAX_MCP_SERVERS &&
        value.mcp.every(isMcpServerStatus) &&
        new Set(value.mcp.map((m) => (m as { name: string }).name)).size === value.mcp.length &&
        // The write confirmation is only asked when changes are allowed.
        (value.phase !== 'confirmWrite' || value.write === true)
    )
}

/** The change a kept Gateway draft or record describes (C17). */
function isGatewayChange(value: Record<string, unknown>): boolean {
    return (
        typeof value.id === 'string' &&
        GATEWAY_LOCAL_ID.test(value.id) &&
        optional(isListedWorkspace, value.workspace) &&
        oneOf(CHANGE_KINDS, value.kind) &&
        typeof value.kind === 'string' &&
        value.kind.startsWith('gateway') &&
        oneOf(CHANGE_ACTIONS, value.action) &&
        isString(value.target) &&
        isString(value.body)
    )
}

/** The tenant's Gateway drafts and uncertain changes on this Mac (C17). */
function isGatewayLocal(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(REPORTS_STATES, value.state) &&
        // A failure is reported exactly when the file could not be opened.
        (value.state === 'failed' ? oneOf(REPORT_STORE_FAILURES, value.failure) : value.failure === null) &&
        Array.isArray(value.drafts) &&
        value.drafts.every((d) => isRecord(d) && isGatewayChange(d) && isString(d.savedAt)) &&
        Array.isArray(value.records) &&
        value.records.every(
            (r) =>
                isRecord(r) &&
                isGatewayChange(r) &&
                optional(isString, r.name) &&
                oneOf(OUTCOMES, r.outcome) &&
                optional((b) => oneOf(READBACKS, b), r.readback) &&
                (r.provisioning === null ||
                    (r.kind === 'gatewayWorkspaces' && r.action === 'create' && isProvisioning(r.provisioning))) &&
                isString(r.settledAt)
        ) &&
        Array.isArray(value.playground) &&
        value.playground.every(
            (d) =>
                isRecord(d) &&
                typeof d.id === 'string' &&
                GATEWAY_LOCAL_ID.test(d.id) &&
                isListedWorkspace(d.workspace) &&
                oneOf(INFERENCE_APIS, d.api) &&
                typeof d.model === 'string' &&
                optional(isGatewayId, d.route) &&
                optional((x) => typeof x === 'string', d.system) &&
                typeof d.prompt === 'string' &&
                optional(isGatewayId, d.file) &&
                isString(d.savedAt)
        ) &&
        Array.isArray(value.runs) &&
        value.runs.every(
            (r) =>
                isRecord(r) &&
                typeof r.id === 'string' &&
                GATEWAY_LOCAL_ID.test(r.id) &&
                isListedWorkspace(r.workspace) &&
                oneOf(INFERENCE_APIS, r.api) &&
                isString(r.model) &&
                optional(isGatewayId, r.route) &&
                typeof r.trace === 'string' &&
                TRACE_ID.test(r.trace) &&
                oneOf(RUN_ENDS, r.end) &&
                oneOf(OUTCOMES, r.outcome) &&
                optional((f) => oneOf(INFERENCE_FAILURES, f), r.failure) &&
                typeof r.answered === 'boolean' &&
                isCount(r.outputBytes) &&
                optional(isTokenUsage, r.usage) &&
                isString(r.settledAt) &&
                // Kept output never: only its size.
                !('output' in r)
        ) &&
        Array.isArray(value.files) &&
        value.files.every(
            (r) =>
                isRecord(r) &&
                typeof r.id === 'string' &&
                GATEWAY_LOCAL_ID.test(r.id) &&
                isListedWorkspace(r.workspace) &&
                oneOf(FILE_CHANGES, r.action) &&
                optional(isString, r.filename) &&
                optional(isGatewayId, r.file) &&
                optional(isCount, r.bytes) &&
                typeof r.trace === 'string' &&
                TRACE_ID.test(r.trace) &&
                oneOf(OUTCOMES, r.outcome) &&
                optional((f) => oneOf(INFERENCE_FAILURES, f), r.failure) &&
                isString(r.settledAt) &&
                // A file's content is never kept.
                !('content' in r)
        ) &&
        optional((p) => oneOf(GATEWAY_DRAFT_PROBLEMS, p), value.problem) &&
        typeof value.unsaved === 'boolean'
    )
}

function isJsonObjectText(value: unknown): boolean {
    if (typeof value !== 'string') return false
    try {
        return isRecord(JSON.parse(value))
    } catch {
        return false
    }
}

function isChangeFailure(value: unknown): boolean {
    if (!isRecord(value)) return false
    if (['conflict', 'expired', 'capacity', 'workspaceChanged'].includes(value.kind as string)) return true
    if (value.kind === 'problem') return oneOf(CHANGE_PROBLEMS, value.problem)
    return value.kind === 'service' && oneOf(MANAGEMENT_FAILURES, value.failure)
}

function isChangeReview(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.review === 'string' &&
        REVIEW_ID.test(value.review) &&
        oneOf(CHANGE_KINDS, value.kind) &&
        oneOf(CHANGE_ACTIONS, value.action) &&
        isString(value.target) &&
        oneOf(REVIEW_STATES, value.state) &&
        optional(isChangeFailure, value.failure) &&
        // A failure is reported exactly when the review failed or expired.
        (value.state === 'failed' || value.state === 'expired') === (value.failure !== null) &&
        optional(isJsonObjectText, value.before) &&
        optional(isJsonObjectText, value.after) &&
        optional(isJsonObjectText, value.sourceKey) &&
        isString(value.impact) &&
        optional(isString, value.confirmation) &&
        optional(isListedWorkspace, value.workspace) &&
        Array.isArray(value.references) &&
        value.references.every(isString) &&
        // Only a Gateway delete lists references, and only once it is ready.
        (value.references.length === 0 || (value.action === 'delete' && value.state !== 'failed'))
    )
}

/** A workspace provisioning's steps and their reconciliation (C17). */
function isProvisioning(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.scopeName) &&
        oneOf(STEP_STATES, value.scope) &&
        oneOf(STEP_STATES, value.workspace) &&
        oneOf(STEP_STATES, value.binding) &&
        oneOf(SCOPE_CLEANUPS, value.cleanup) &&
        optional(isString, value.workspaceId) &&
        optional(isString, value.workspaceSlug) &&
        oneOf(RECONCILED, value.scopeRead) &&
        optional((b) => typeof b === 'boolean', value.boundRead) &&
        oneOf(RECONCILED, value.workspaceRead)
    )
}

function isChangeResult(value: unknown): boolean {
    return (
        isRecord(value) &&
        isOperation(value.operation) &&
        oneOf(CHANGE_KINDS, value.kind) &&
        oneOf(CHANGE_ACTIONS, value.action) &&
        isString(value.target) &&
        optional(isString, value.name) &&
        typeof value.settled === 'boolean' &&
        // Only a settled change has an outcome.
        (value.settled ? oneOf(OUTCOMES, value.outcome) : value.outcome === null) &&
        optional(isChangeFailure, value.failure) &&
        optional(isJsonObjectText, value.response) &&
        optional((r) => oneOf(READBACKS, r), value.readback) &&
        optional(isJsonObjectText, value.current) &&
        oneOf(SECRET_STATES, value.secret) &&
        optional(isListedWorkspace, value.workspace) &&
        // Steps belong only to a workspace provisioning.
        (value.provisioning === null ||
            (value.kind === 'gatewayWorkspaces' && value.action === 'create' && isProvisioning(value.provisioning))) &&
        // C31: when the settlement was recorded; only a settled change has one.
        (value.settledAt === undefined || (value.settled === true && isCount(value.settledAt)))
    )
}

function isStagedFile(value: unknown): boolean {
    if (!isRecord(value) || !oneOf(STAGED_STATES, value.state)) return false
    const ready = value.state === 'ready'
    return (
        (ready
            ? typeof value.token === 'string' && STAGED_TOKEN.test(value.token) && isString(value.name) && isCount(value.bytes) && isCount(value.lines)
            : value.token === null && value.name === null && value.bytes === null && value.lines === null) &&
        (value.state === 'failed' ? oneOf(STAGED_FAILURES, value.failure) : value.failure === null)
    )
}

const SEARCH_KINDS: readonly string[] = ['sessionId', 'scanId', 'detector']
const REPORT_KINDS: readonly string[] = ['false_positive', 'false_negative']
const REPORT_VERDICTS: readonly string[] = ['malicious', 'benign', 'unknown']
export const REPORT_PROBLEMS: readonly string[] = ['reason', 'field', 'detectors', 'full', 'verdict', 'scan']
const REPORTS_STATES: readonly string[] = ['idle', 'loading', 'ready', 'failed']
const REPORT_STORE_FAILURES: readonly string[] = ['unavailable', 'unreadable']
const INVESTIGATION_STORAGE: readonly string[] = ['memory', 'protected', 'unavailable']
const EXPORT_KINDS: readonly string[] = ['reports', 'investigation', 'cases', 'harnessConversation']
const EXPORT_STATES: readonly string[] = ['saving', 'saved', 'cancelled', 'failed']

function isReport(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.scanId) &&
        isCount(value.scanSubReqId) &&
        isString(value.sessionId) &&
        isString(value.applicationId) &&
        isString(value.application) &&
        oneOf(REPORT_KINDS, value.kind) &&
        oneOf(REPORT_VERDICTS, value.verdict) &&
        isString(value.action) &&
        Array.isArray(value.detectors) &&
        value.detectors.every(isString) &&
        isString(value.profile) &&
        isString(value.category) &&
        isString(value.reason) &&
        isString(value.reportedAt)
    )
}

function isSessionsView(value: unknown): boolean {
    if (!isRecord(value) || !isRecord(value.reports)) return false
    const reports = value.reports
    return (
        oneOf(REPORTS_STATES, reports.state) &&
        // A failure is reported exactly when the reports failed to open.
        (reports.state === 'failed' ? oneOf(REPORT_STORE_FAILURES, reports.failure) : reports.failure === null) &&
        Array.isArray(reports.items) &&
        reports.items.every(isReport) &&
        optional((u) => isRecord(u) && isString(u.scanId) && isCount(u.subRequest), reports.undo) &&
        optional((p) => oneOf(REPORT_PROBLEMS, p), reports.problem) &&
        typeof reports.unsaved === 'boolean' &&
        optional(
            (i) =>
                isRecord(i) &&
                oneOf(INVESTIGATION_STORAGE, i.storage) &&
                isCount(i.sessions) &&
                isCount(i.transactions) &&
                isCount(i.contents),
            value.investigation
        ) &&
        optional(
            (e) =>
                isExportView(e) &&
                (e as Record<string, unknown>).kind !== 'cases' &&
                (e as Record<string, unknown>).kind !== 'harnessConversation',
            value.export
        )
    )
}

function isSessionSearch(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(SEARCH_KINDS, value.kind) &&
        isString(value.query) &&
        isItemArray(value.sessions) &&
        isCount(value.scanned) &&
        typeof value.complete === 'boolean' &&
        typeof value.matchLimit === 'boolean' &&
        optional(isJsonObjectText, value.scan) &&
        typeof value.cached === 'boolean'
    )
}

const CASE_PRODUCTS: readonly string[] = ['ai-gateway', 'ai-red-teaming', 'model-security', 'runtime-security', 'skill-scanning']
export const CASE_PROBLEMS: readonly string[] = [
    'fields',
    'payload',
    'foreignTenant',
    'full',
    'tooLarge',
    'duplicate',
    'format',
    'catalog',
    'allPresent',
    'missing',
    'notSaved',
]
const LIBRARY_STATES: readonly string[] = ['idle', 'loading', 'ready', 'failed']
const LIBRARY_FAILURES: readonly string[] = ['locked', 'denied', 'unavailable', 'unreadable']
const TONES: readonly string[] = ['ok', 'warn', 'bad']
const INBOX_DESTINATIONS: readonly string[] = ['apiKeys', 'sessions', 'gateway']
const FACES: readonly string[] = ['items', 'firstLaunch', 'allClear', 'gap']
const HOLD_KINDS: readonly string[] = ['acknowledge', 'snooze']

function isExportView(e: unknown): boolean {
    return (
        isRecord(e) &&
        oneOf(EXPORT_KINDS, e.kind) &&
        oneOf(EXPORT_STATES, e.state) &&
        // Only a saved export names its file.
        (e.state === 'saved' ? isString(e.name) : e.name === null)
    )
}

function isCaseView(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.id) &&
        oneOf(CASE_PRODUCTS, value.product) &&
        isString(value.name) &&
        isString(value.profile) &&
        isString(value.prompt) &&
        isString(value.response) &&
        isString(value.appName) &&
        isString(value.expected) &&
        optional(isString, value.payload) &&
        // Only specifications carry a payload (v1).
        (value.product !== 'runtime-security' || value.payload === null)
    )
}

function isLibraryNotice(value: unknown): boolean {
    if (!isRecord(value)) return false
    if (value.kind === 'saved') return isString(value.name)
    if (value.kind === 'imported' || value.kind === 'examplesAdded') return isCount(value.added)
    if (value.kind === 'copied') return isString(value.tenant)
    return value.kind === 'catalogUnavailable' || value.kind === 'fileUnreadable'
}

function isLibraryView(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(LIBRARY_STATES, value.state) &&
        // A failure is reported exactly when the library failed to open.
        (value.state === 'failed' ? oneOf(LIBRARY_FAILURES, value.failure) : value.failure === null) &&
        Array.isArray(value.cases) &&
        value.cases.length <= 32 &&
        value.cases.every(isCaseView) &&
        optional((p) => oneOf(CASE_PROBLEMS, p), value.problem) &&
        optional((f) => oneOf(LIBRARY_FAILURES, f), value.unsaved) &&
        optional(isLibraryNotice, value.notice) &&
        typeof value.choosing === 'boolean' &&
        typeof value.fetching === 'boolean' &&
        optional((e) => isExportView(e) && (e as Record<string, unknown>).kind === 'cases', value.export)
    )
}

const optionalCount = (value: unknown): boolean => value === null || isCount(value)

function isInboxItem(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.id) &&
        isString(value.title) &&
        isString(value.detail) &&
        isString(value.source) &&
        typeof value.action === 'boolean' &&
        oneOf(TONES, value.tone) &&
        oneOf(INBOX_DESTINATIONS, value.destination) &&
        isString(value.actionLabel) &&
        Array.isArray(value.facts) &&
        value.facts.every((f) => isRecord(f) && isString(f.label) && isString(f.value)) &&
        optional((n) => isCount(n) && (n as number) <= 1000, value.perMille)
    )
}

function isRuntimeOverview(value: unknown): boolean {
    if (!isRecord(value) || !isRecord(value.summary)) return false
    const summary = value.summary
    const chart = value.chart
    const chartOk =
        chart === null ||
        (isRecord(chart) &&
            isCount(chart.bucketSeconds) &&
            (chart.bucketSeconds as number) >= 1 &&
            Array.isArray(chart.buckets) &&
            chart.buckets.length <= 2048 &&
            chart.buckets.every(
                (b) =>
                    isRecord(b) &&
                    isString(b.time) &&
                    isCount(b.total) &&
                    isCount(b.violating) &&
                    optional(
                        (d) => Array.isArray(d) && d.every((c) => isRecord(c) && isString(c.detector) && isCount(c.count)),
                        b.detections
                    )
            ))
    return (
        isCount(value.readAt) &&
        chartOk &&
        // A source has data or a failure, never both.
        (chart === null) === (value.chartFailure !== null) &&
        optional((f) => oneOf(MANAGEMENT_FAILURES, f), value.chartFailure) &&
        optionalCount(summary.sessions) &&
        optionalCount(summary.violating) &&
        optionalCount(summary.benign) &&
        optionalCount(summary.detectionEvents) &&
        typeof value.keysComplete === 'boolean' &&
        optional((f) => oneOf(MANAGEMENT_FAILURES, f), value.keysFailure) &&
        Array.isArray(value.items) &&
        value.items.every(isInboxItem) &&
        Array.isArray(value.gaps) &&
        value.gaps.every(isString) &&
        optional(isGatewayOverview, value.gateway)
    )
}

/** The chosen Gateway workspace's last day on Home (C18). */
function isGatewayOverview(value: unknown): boolean {
    return (
        isRecord(value) &&
        isGatewayId(value.workspace) &&
        isString(value.workspaceName) &&
        optionalMeasure(value.requests) &&
        optionalMeasure(value.errors) &&
        optionalMeasure(value.cost) &&
        typeof value.partial === 'boolean' &&
        optionalFailure(value.usageFailure) &&
        optionalCount(value.policies) &&
        optionalFailure(value.limitsFailure)
    )
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/
const isUuid = (value: unknown): boolean => typeof value === 'string' && UUID.test(value)
const isToken = (value: unknown): boolean => typeof value === 'string' && value.length > 0 && Array.from(value).length <= 64
const optionalToken = (value: unknown): boolean => value === null || isToken(value)
const optionalFailure = (value: unknown): boolean => optional((f) => oneOf(MANAGEMENT_FAILURES, f), value)

/** A source has its data or its failure, never both and never neither. */
const sourced = (data: unknown, failure: unknown, valid: (v: unknown) => boolean): boolean =>
    optionalFailure(failure) && (data === null ? failure !== null : failure === null && valid(data))

function isBars(value: unknown): boolean {
    return (
        value === null ||
        (Array.isArray(value) && value.length <= 64 && value.every((b) => isRecord(b) && isToken(b.label) && isCount(b.count)))
    )
}

/**
 * A first page of at most 100 rows with its total, whether that total is a
 * capped lower bound, and completeness (never complete when capped).
 */
function isRowPage(value: unknown, row: (r: unknown) => boolean): boolean {
    return (
        isRecord(value) &&
        Array.isArray(value.rows) &&
        value.rows.length <= 100 &&
        value.rows.every(row) &&
        optionalCount(value.total) &&
        typeof value.capped === 'boolean' &&
        typeof value.complete === 'boolean' &&
        !(value.capped && value.complete)
    )
}

function isRedTeamStatistics(value: unknown): boolean {
    return (
        isRecord(value) &&
        isCount(value.scans) &&
        isCount(value.targets) &&
        isBars(value.status) &&
        isBars(value.risk) &&
        isBars(value.targetTypes) &&
        typeof value.noScans === 'boolean'
    )
}

const isRedTeamScan = (r: unknown): boolean =>
    isRecord(r) &&
    isUuid(r.id) &&
    optionalToken(r.status) &&
    optionalToken(r.jobType) &&
    optional(isUuid, r.targetId) &&
    optionalToken(r.targetType) &&
    optionalToken(r.createdAt)

function isRedTeamObservation(value: unknown): boolean {
    return (
        isRecord(value) &&
        isCount(value.readAt) &&
        [7, 15, 30].includes(value.window as number) &&
        sourced(value.statistics, value.statisticsFailure, isRedTeamStatistics) &&
        sourced(value.scans, value.scansFailure, (v) => isRowPage(v, isRedTeamScan)) &&
        // A read in which no source answered is a failed read, not this.
        (value.statistics !== null || value.scans !== null)
    )
}

const isModelScan = (r: unknown): boolean => isRecord(r) && isUuid(r.id) && optionalToken(r.outcome) && optionalToken(r.createdAt)

const isSkillScan = (r: unknown): boolean =>
    isRecord(r) &&
    typeof r.pseudonym === 'string' &&
    /^[0-9a-f]{8}$/.test(r.pseudonym) &&
    optionalToken(r.status) &&
    optionalToken(r.outcome) &&
    optionalCount(r.vulnerabilities) &&
    optionalToken(r.createdAt)

function isSupplyChainObservation(value: unknown): boolean {
    return (
        isRecord(value) &&
        isCount(value.readAt) &&
        sourced(value.models, value.modelsFailure, (v) => isRowPage(v, isModelScan)) &&
        sourced(
            value.skillStatistics,
            value.skillStatisticsFailure,
            (v) => isRecord(v) && isCount(v.uniqueSkills) && isCount(v.vulnerabilities)
        ) &&
        sourced(value.skills, value.skillsFailure, (v) => isRowPage(v, isSkillScan)) &&
        (value.models !== null || value.skillStatistics !== null || value.skills !== null)
    )
}

function isAttackPage(value: unknown): boolean {
    if (!isRecord(value) || !isUuid(value.scanId) || !isCount(value.page) || (value.page as number) > 9) return false
    const first = (value.page as number) * 100 + 1
    return (
        isRowPage(value, (a) => isRecord(a) && isCount(a.number) && optional((t) => typeof t === 'boolean', a.threat) &&
            optionalToken(a.severity) && optionalToken(a.attackType) && optionalToken(a.subCategory) &&
            optional((t) => typeof t === 'boolean', a.multiTurn)) &&
        // Rows are numbered in report order from this page's first position.
        (value.rows as Record<string, unknown>[]).every((a, i) => a.number === first + i)
    )
}

function isOverviewView(value: unknown): boolean {
    return (
        isRecord(value) &&
        optional(isRuntimeOverview, value.read) &&
        typeof value.refreshing === 'boolean' &&
        optional((f) => oneOf(MANAGEMENT_FAILURES, f), value.failure) &&
        optional(isCount, value.failedAt) &&
        typeof value.fresh === 'boolean' &&
        (value.read !== null || value.fresh === false) &&
        Array.isArray(value.attention) &&
        value.attention.every(isInboxItem) &&
        Array.isArray(value.held) &&
        value.held.every((h) => isRecord(h) && isInboxItem(h.item) && oneOf(HOLD_KINDS, h.kind) && optional(isCount, h.returnsAt) && (h.kind !== 'snooze' || h.returnsAt !== null)) &&
        oneOf(FACES, value.face) &&
        // All clear only from a fresh read with nothing open.
        (value.face !== 'allClear' || (value.fresh === true && value.attention.length === 0))
    )
}

const INBOX_KINDS: readonly string[] = ['runtime', 'gateway', 'setupWarning', 'rotation']
const READ_STATES: readonly string[] = ['pending', 'reading', 'done', 'failed']
const PRODUCT_READ_STATES: readonly string[] = ['pending', 'ok', 'incomplete', 'needsRole', 'failed', 'notRead']
/** The active tenant's Inbox and read status (C30): entries with their core-computed holds. */
function isInboxView(value: unknown): boolean {
    if (!isRecord(value) || !Array.isArray(value.items) || !isRecord(value.read)) return false
    const entries = value.items.every(
        (e: unknown) =>
            isRecord(e) &&
            isString(e.id) &&
            oneOf(INBOX_KINDS, e.kind) &&
            typeof e.needsAction === 'boolean' &&
            oneOf(TONES, e.severity) &&
            isString(e.title) &&
            typeof e.detail === 'string' &&
            typeof e.source === 'string' &&
            optional((p) => oneOf(PRODUCT_IDS, p), e.product) &&
            optional(
                (t) =>
                    isRecord(t) &&
                    FEATURE_IDS.includes(t.feature as string) &&
                    optional((p) => oneOf(PRODUCT_IDS, p), t.product) &&
                    optional((r) => oneOf(MANAGEMENT_RESOURCES, r), t.resource) &&
                    optional((i) => typeof i === 'string', t.item),
                e.target
            ) &&
            optional((a) => typeof a === 'string', e.actionLabel) &&
            Array.isArray(e.facts) &&
            e.facts.every((f: unknown) => isRecord(f) && typeof f.label === 'string' && typeof f.value === 'string') &&
            isCount(e.when) &&
            optional((h) => isRecord(h) && oneOf(HOLD_KINDS, h.kind) && optional(isCount, h.returnsAt) && (h.kind !== 'snooze' || h.returnsAt !== null), e.hold)
    )
    const read = value.read
    return (
        entries &&
        oneOf(READ_STATES, read.state) &&
        optional(isCount, read.lastReadAt) &&
        optional(isCount, read.failedAt) &&
        optional(isCount, read.settledAt) &&
        // A read that is done or failed settled at some time.
        (read.state === 'pending' || read.state === 'reading' || read.settledAt !== null) &&
        // A failed read names when it failed; never attempted means neither time.
        (read.state !== 'failed' || read.failedAt !== null) &&
        Array.isArray(read.products) &&
        read.products.every((p: unknown) => isRecord(p) && oneOf(PRODUCT_IDS, p.product) && oneOf(PRODUCT_READ_STATES, p.state) && isCount(p.sourcesRead) && isCount(p.sourcesTotal) && optional((d) => typeof d === 'string', p.detail)) &&
        Array.isArray(read.sourcesMissing) &&
        read.sourcesMissing.every((s: unknown) => typeof s === 'string') &&
        isCount(read.generation) &&
        optional((p) => oneOf(PRODUCT_IDS, p), read.rechecking) &&
        // Nothing read yet means no last-read time and no product result.
        (read.state !== 'pending' || read.lastReadAt === null) &&
        // The last snooze placed this session: the receipt the shell announces once.
        optional((r) => isRecord(r) && isString(r.id) && typeof r.title === 'string' && isCount(r.returnsAt) && isCount(r.sequence), value.lastSnooze)
    )
}

function isChangesView(value: unknown): boolean {
    return (
        isRecord(value) &&
        optional(isChangeReview, value.review) &&
        optional(isStagedFile, value.staged) &&
        Array.isArray(value.results) &&
        value.results.every(isChangeResult)
    )
}

/** Validates a management read result (items stay JSON text). */

// ------------------------------------------------------------- API Client

const PRIVATE_FAILURES: readonly string[] = ['locked', 'denied', 'unavailable', 'unreadable']
const CONFLICT_KINDS: readonly string[] = ['changed', 'deleted']
const ISSUE_KINDS: readonly string[] = ['link', 'invalid', 'duplicate', 'schema', 'name', 'limit']
const ENV_ORIGINS: readonly string[] = ['shared', 'local', 'both']
const LAYERS: readonly string[] = ['collection', 'localDefaults', 'shared', 'local', 'request']
const GIT_STATES: readonly string[] = ['unavailable', 'notRepository', 'ready', 'failed']
const FIELD_NAMES: readonly string[] = ['method', 'url', 'headers', 'query', 'body', 'bodyKind', 'settings']
const SAVE_REFUSALS: readonly string[] = ['conflict', 'incomplete', 'moved', 'sharedBody', 'interrupted', 'exists', 'io']
const PROJECT_PROBLEMS: readonly string[] = ['notCollection', 'badPath', 'exists', 'notInRepository', 'incomplete', 'missing', 'tooLarge', 'io']
const SCHEMA_PROBLEMS: readonly string[] = ['badPath', 'missing', 'notText', 'conflict', 'io']
const CURL_PROBLEMS: readonly string[] = [
    'tooLarge', 'notCurl', 'shell', 'unclosed', 'tooManyWords', 'unsupportedOption', 'missingArgument', 'url',
    'urlCredentials', 'queryCredentials', 'glob', 'template', 'bodyCredentials', 'json', 'header', 'external', 'combination',
]
const EXPORT_PROBLEMS: readonly string[] = ['tooLarge', 'unreadable', 'version', 'structure', 'limit']
const RESOURCE_PROBLEMS: readonly string[] = [
    'fields', 'pathParameters', 'settings', 'method', 'url', 'template', 'header', 'parameter', 'credential', 'body',
    'json', 'auth', 'authReview', 'authCombination', 'folder', 'path', 'environment',
]
const ADOPTION_PROBLEMS: readonly string[] = ['missing', 'unreadable']
const WORKBENCH_PROBLEMS: Readonly<Record<string, readonly string[] | null>> = {
    noCollection: null, missing: null, stale: null, unavailable: null, invalid: null, managed: null, limit: null, keychain: null, unprotected: null,
    project: PROJECT_PROBLEMS, private: PRIVATE_FAILURES, schema: SCHEMA_PROBLEMS, curl: CURL_PROBLEMS,
    export: EXPORT_PROBLEMS, adoption: ADOPTION_PROBLEMS,
}
/** A request document's identity (v1: letters, digits, `_` and `-`). */
const DOCUMENT_ID = /^[A-Za-z0-9_-]{1,128}$/
const REVISION = /^[0-9a-f]{32}$/
const DIGEST = /^[0-9a-f]{64}$/
/** Collection-relative paths: no absolute, parent or empty components. */
const isRelative = (value: unknown): boolean =>
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= 1024 &&
    !value.startsWith('/') &&
    value.split('/').every((part) => part !== '' && part !== '..' && part !== '.')
const isStrings = (value: unknown, check: (v: unknown) => boolean = isString): boolean =>
    Array.isArray(value) && value.every(check)

function isFields(value: unknown): boolean {
    return (
        isRecord(value) &&
        ['method', 'url', 'headers', 'query', 'body', 'bodyKind', 'settings'].every((k) => typeof value[k] === 'string')
    )
}

const isFieldProblem = (p: unknown): boolean => isRecord(p) && oneOf(FIELD_NAMES, p.field) && optional(isCount, p.line)

function isDocumentView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        DOCUMENT_ID.test(value.id) &&
        isRelative(value.relative) &&
        typeof value.revision === 'string' &&
        REVISION.test(value.revision) &&
        isFields(value.fields) &&
        isFields(value.saved) &&
        typeof value.new === 'boolean' &&
        typeof value.recovered === 'boolean' &&
        typeof value.bodyEditable === 'boolean' &&
        optional(isRelative, value.schema) &&
        optional((c) => oneOf(CONFLICT_KINDS, c), value.conflict) &&
        typeof value.interrupted === 'boolean' &&
        typeof value.metadataDirty === 'boolean'
    )
}

function isTabView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        DOCUMENT_ID.test(value.id) &&
        isRelative(value.relative) &&
        isString(value.method) &&
        ['draft', 'new', 'recovered', 'interrupted'].every((k) => typeof value[k] === 'boolean') &&
        optional((c) => oneOf(CONFLICT_KINDS, c), value.conflict)
    )
}

function isCollectionView(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.id) &&
        isString(value.name) &&
        isString(value.folder) &&
        typeof value.builtIn === 'boolean' &&
        typeof value.complete === 'boolean' &&
        Array.isArray(value.requests) &&
        value.requests.every(
            (r) => isRecord(r) && isRelative(r.relative) && typeof r.id === 'string' && DOCUMENT_ID.test(r.id) && isString(r.method) && isString(r.url)
        ) &&
        isStrings(value.folders, isRelative) &&
        isStrings(value.schemas, isRelative) &&
        Array.isArray(value.issues) &&
        value.issues.every((i) => isRecord(i) && isString(i.relative) && oneOf(ISSUE_KINDS, i.kind)) &&
        Array.isArray(value.environments) &&
        value.environments.every((e) => isRecord(e) && isString(e.name) && oneOf(ENV_ORIGINS, e.origin)) &&
        optional(isString, value.environment) &&
        Array.isArray(value.tabs) &&
        value.tabs.every(isTabView) &&
        optional((t) => typeof t === 'string' && DOCUMENT_ID.test(t), value.selected) &&
        Array.isArray(value.secrets) &&
        value.secrets.every((x) => isRecord(x) && isString(x.reference) && typeof x.stored === 'boolean' && typeof x.managed === 'boolean') &&
        optional((f) => oneOf(PRIVATE_FAILURES, f), value.unsaved) &&
        isCount(value.archives)
    )
}

const isResourceView = (r: unknown): boolean =>
    isRecord(r) &&
    isCount(r.index) &&
    isString(r.label) &&
    isRelative(r.relative) &&
    typeof r.environment === 'boolean' &&
    optional((p) => oneOf(RESOURCE_PROBLEMS, p), r.problem)

const isCandidateView = (c: unknown): boolean =>
    isRecord(c) && isCount(c.index) && isString(c.name) && isString(c.folder) && isStrings(c.environments) && isCount(c.skipped)

function isGitView(value: unknown): boolean {
    return (
        isRecord(value) &&
        oneOf(GIT_STATES, value.state) &&
        optional(isString, value.branch) &&
        optional(isString, value.head) &&
        optional(isString, value.upstream) &&
        optional(isCount, value.ahead) &&
        optional(isCount, value.behind) &&
        Array.isArray(value.changes) &&
        value.changes.length <= 500 &&
        value.changes.every((c) => isRecord(c) && isString(c.path) && isString(c.status)) &&
        typeof value.truncated === 'boolean'
    )
}

function isWorkbenchView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.available === 'boolean' &&
        typeof value.loaded === 'boolean' &&
        optional((f) => oneOf(PRIVATE_FAILURES, f), value.failure) &&
        Array.isArray(value.recent) &&
        value.recent.every((r) => isRecord(r) && isCount(r.index) && isString(r.name) && isString(r.folder)) &&
        optional(isCollectionView, value.collection) &&
        optional((i) => Array.isArray(i) && i.every(isResourceView), value.import) &&
        optional((a) => Array.isArray(a) && a.every(isCandidateView), value.adoption) &&
        typeof value.adoptionSettled === 'boolean' &&
        optional(isGitView, value.git) &&
        typeof value.busy === 'boolean' &&
        typeof value.unprotected === 'boolean' &&
        Array.isArray(value.running) &&
        value.running.length <= 4 &&
        value.running.every(isRunningView) &&
        optional(isSendReviewView, value.review) &&
        oneOf(NETWORKS, value.network)
    )
}

const isPair = (p: unknown): boolean => Array.isArray(p) && p.length === 2 && isString(p[0]) && isString(p[1])

function isPreview(value: unknown): boolean {
    return (
        isRecord(value) &&
        isString(value.method) &&
        isString(value.url) &&
        Array.isArray(value.headers) &&
        value.headers.every(isPair) &&
        Array.isArray(value.query) &&
        value.query.every(isPair) &&
        isString(value.bodyKind) &&
        isString(value.body) &&
        isCount(value.bodyBytes) &&
        Array.isArray(value.variables) &&
        value.variables.every(
            (v) =>
                isRecord(v) &&
                isString(v.name) &&
                oneOf(LAYERS, v.layer) &&
                optional(isString, v.secret) &&
                optional((s) => typeof s === 'boolean', v.stored)
        ) &&
        isStrings(value.unresolved) &&
        isStrings(value.missingSecrets) &&
        optional(isString, value.auth) &&
        isStrings(value.authReferences) &&
        optional(isFieldProblem, value.problem) &&
        typeof value.jsonWarning === 'boolean'
    )
}

function isWorkbenchProblem(value: unknown): boolean {
    if (!isRecord(value) || typeof value.kind !== 'string' || !(value.kind in WORKBENCH_PROBLEMS)) return false
    const details = WORKBENCH_PROBLEMS[value.kind]
    if (details === undefined) return false
    return details === null ? value.detail === undefined : oneOf(details, value.detail)
}

function isImportNote(value: unknown): boolean {
    if (!isRecord(value)) return false
    switch (value.kind) {
        case 'flagOmitted':
        case 'compressed':
        case 'emptyCookie':
            return true
        case 'authRemoved':
            return isString(value.reference)
        case 'headerReplaced':
            return isString(value.variable) && isString(value.reference)
    }
    return false
}

function isSaveRefusal(value: unknown): boolean {
    if (!isRecord(value)) return false
    if (value.kind === 'invalid') return isFieldProblem(value.problem)
    return oneOf(SAVE_REFUSALS, value.kind)
}

// ---- API Client execution (C15)

const RUN_ID = /^run-[0-9a-f]{16}$/
const RUN_STATES: readonly string[] = ['running', 'completed', 'failed', 'cancelled', 'interrupted']
const RUN_FAILURES: readonly string[] = ['timeout', 'connect', 'tls', 'request', 'token', 'tooLarge', 'storage', 'redirect', 'retired']
const CREDENTIAL_CLASSES: readonly string[] = ['anonymous', 'workbench', 'managed']
const NETWORKS: readonly string[] = ['live', 'loopbackOnly', 'disabled']
const SECRET_FAILURES: readonly string[] = ['missing', 'locked', 'denied', 'invalid']
const BODY_PROBLEMS: readonly string[] = ['missing', 'tooLarge', 'unsaved']
const EXPORT_FAILURES: readonly string[] = ['destination', 'body', 'io']
const SEND_REFUSALS: readonly string[] = [
    'url', 'network', 'mixedCredentials', 'managedPlacement', 'ambiguousCredential', 'noTenant', 'tokenDestination',
    'requestDestination', 'notHttps', 'unknownEndpoint', 'busy', 'stale', 'reviewMissing', 'reviewStale', 'reviewExpired',
    'reviewConsumed', 'declined', 'private',
]
const RENDER_PROBLEMS: readonly string[] = ['unclosed', 'secretInName', 'name', 'url', 'auth', 'multipartContentType']

function isRenderProblem(value: unknown): boolean {
    if (!isRecord(value)) return false
    if (value.kind === 'field') return isFieldProblem(value.problem)
    if (value.kind === 'unresolved' || value.kind === 'nested') return isString(value.name)
    return oneOf(RENDER_PROBLEMS, value.kind)
}

function isSendRefusal(value: unknown): boolean {
    if (!isRecord(value)) return false
    switch (value.kind) {
        case 'render':
            return isRenderProblem(value.problem)
        case 'unknownManagedReference':
            return isString(value.reference)
        case 'secret':
            return isString(value.reference) && oneOf(SECRET_FAILURES, value.failure)
        case 'body':
            return oneOf(BODY_PROBLEMS, value.problem)
    }
    return oneOf(SEND_REFUSALS, value.kind)
}

function isRunRecord(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        RUN_ID.test(value.id) &&
        typeof value.request === 'string' &&
        DOCUMENT_ID.test(value.request) &&
        isRelative(value.relative) &&
        isCount(value.at) &&
        isString(value.method) &&
        isString(value.url) &&
        oneOf(CREDENTIAL_CLASSES, value.class) &&
        optional(isString, value.operation) &&
        oneOf(RUN_STATES, value.state) &&
        optional((f) => oneOf(RUN_FAILURES, f), value.failure) &&
        typeof value.uncertain === 'boolean' &&
        optional((s) => isCount(s) && (s as number) < 1000, value.status) &&
        Array.isArray(value.headers) &&
        value.headers.every(isPair) &&
        optional(isString, value.redirect) &&
        isCount(value.elapsedMs) &&
        isCount(value.bytes) &&
        typeof value.stored === 'boolean'
    )
}

function isRunView(value: unknown): boolean {
    return (
        isRecord(value) &&
        isRunRecord(value.record) &&
        optional(isString, value.preview) &&
        isCount(value.previewBytes) &&
        typeof value.tokenDropped === 'boolean'
    )
}

function isRunningView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.run === 'string' &&
        RUN_ID.test(value.run) &&
        typeof value.request === 'string' &&
        DOCUMENT_ID.test(value.request) &&
        isString(value.method) &&
        isCount(value.bytes)
    )
}

function isSendReviewView(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.review === 'string' &&
        REVIEW_ID.test(value.review) &&
        typeof value.request === 'string' &&
        DOCUMENT_ID.test(value.request) &&
        isString(value.operation) &&
        isString(value.method) &&
        isString(value.url) &&
        Array.isArray(value.headers) &&
        value.headers.every(isPair) &&
        isString(value.body) &&
        isCount(value.bodyBytes) &&
        isStrings(value.files, isRelative) &&
        isString(value.destination) &&
        isCount(value.expires)
    )
}

export function parseWorkbenchResult(value: unknown): WorkbenchResult {
    if (isRecord(value)) {
        const valid = (() => {
            switch (value.kind) {
                case 'done':
                case 'cancelled':
                    return true
                case 'refused':
                    return isWorkbenchProblem(value.problem)
                case 'opened':
                    return isString(value.name)
                case 'document':
                    return isDocumentView(value.document) && Array.isArray(value.notes) && value.notes.every(isImportNote)
                case 'saved':
                case 'unchanged':
                    return isDocumentView(value.document)
                case 'saveRefused':
                    return isSaveRefusal(value.refusal)
                case 'protected':
                case 'closed':
                    return typeof value.id === 'string' && DOCUMENT_ID.test(value.id)
                case 'preview':
                    return isPreview(value.preview)
                case 'environment':
                    return (
                        isString(value.name) &&
                        typeof value.private === 'boolean' &&
                        isString(value.text) &&
                        optional((r) => typeof r === 'string' && DIGEST.test(r), value.revision)
                    )
                case 'schema':
                    return (
                        isRecord(value.schema) &&
                        isRelative(value.schema.relative) &&
                        isString(value.schema.text) &&
                        optional((r) => typeof r === 'string' && DIGEST.test(r), value.schema.revision)
                    )
                case 'import':
                    return isString(value.collection) && Array.isArray(value.resources) && value.resources.every(isResourceView)
                case 'created':
                    return isCount(value.requests) && isCount(value.environments) && isCount(value.skipped)
                case 'git':
                    return isGitView(value.git)
                case 'adoption':
                    return Array.isArray(value.candidates) && value.candidates.every(isCandidateView)
                case 'adopted':
                    return isCount(value.collections) && isCount(value.environments)
                case 'ran':
                    return isRunView(value.run)
                case 'reviewRequired':
                    return isSendReviewView(value.review)
                case 'sendRefused':
                    return isSendRefusal(value.refusal)
                case 'history':
                    return Array.isArray(value.runs) && value.runs.every(isRunRecord) && typeof value.more === 'boolean'
                case 'responseSaved':
                    return isCount(value.bytes) && isString(value.name) && !String(value.name).includes('/')
                case 'saveResponseFailed':
                    return oneOf(EXPORT_FAILURES, value.failure)
            }
            return false
        })()
        if (valid) return value as unknown as WorkbenchResult
    }
    throw new ContractViolation('workbench result')
}

export function parseManagementResult(value: unknown): ManagementResult {
    if (isRecord(value)) {
        if (value.kind === 'verified') return { kind: 'verified' }
        if (value.kind === 'page' && isManagementPage(value.page)) return value as unknown as ManagementResult
        if (value.kind === 'detail' && isManagementDetail(value.detail)) return value as unknown as ManagementResult
        if (
            value.kind === 'session' &&
            isRecord(value.session) &&
            isJsonObjectText(value.session.session) &&
            isCount(value.session.actions) &&
            typeof value.session.more === 'boolean'
        ) {
            return value as unknown as ManagementResult
        }
        if (value.kind === 'transaction' && isJsonObjectText(value.transaction)) return value as unknown as ManagementResult
        if (
            value.kind === 'content' &&
            isRecord(value.content) &&
            optional(isString, value.content.prompt) &&
            optional(isString, value.content.response)
        ) {
            return value as unknown as ManagementResult
        }
        if (value.kind === 'search' && isSessionSearch(value.search)) return value as unknown as ManagementResult
        if (value.kind === 'overview' && isRuntimeOverview(value.overview)) return value as unknown as ManagementResult
        if (value.kind === 'redTeam' && isRedTeamObservation(value.observation)) return value as unknown as ManagementResult
        if (value.kind === 'supplyChain' && isSupplyChainObservation(value.observation)) {
            return value as unknown as ManagementResult
        }
        if (value.kind === 'attacks' && isAttackPage(value.attacks)) return value as unknown as ManagementResult
        if (value.kind === 'gateway' && isGatewayResult(value.gateway)) return value as unknown as ManagementResult
        if (value.kind === 'failed' && oneOf(MANAGEMENT_FAILURES, value.failure) && oneOf(OUTCOMES, value.outcome)) {
            return value as unknown as ManagementResult
        }
    }
    throw new ContractViolation('management result')
}

function isDecimalU64(value: unknown): value is string {
    return typeof value === 'string' && DECIMAL_U64.test(value) && BigInt(value) <= U64_MAX
}

const encoder = new TextEncoder()
const encode = (value: unknown): Uint8Array =>
    encoder.encode(JSON.stringify(value))

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function asRefusal(value: unknown): Refusal {
    if (typeof value === 'string' && (REFUSALS as readonly string[]).includes(value)) {
        return value as Refusal
    }
    throw new ContractViolation('unknown refusal')
}

function asSequence(value: unknown): bigint {
    if (typeof value !== 'string' || !DECIMAL_U64.test(value)) {
        throw new ContractViolation('sequence is not a decimal u64 string')
    }
    const n = BigInt(value)
    if (n > U64_MAX) throw new ContractViolation('sequence exceeds u64')
    return n
}

export function parseDispatchResponse(value: unknown): DispatchResponse {
    if (!isRecord(value)) throw new ContractViolation('dispatch response')
    if (value.outcome === 'refusal') {
        return { outcome: 'refusal', refusal: asRefusal(value.refusal) }
    }
    if (
        value.outcome === 'receipt' &&
        isRecord(value.receipt) &&
        value.receipt.kind === 'admitted' &&
        typeof value.receipt.request === 'string' &&
        optional(isOperation, value.receipt.operation)
    ) {
        return {
            outcome: 'receipt',
            receipt: {
                kind: 'admitted',
                request: value.receipt.request,
                operation: value.receipt.operation as string | null,
            },
        }
    }
    throw new ContractViolation('dispatch response')
}

export function parseShellSnapshot(value: unknown): ShellSnapshot {
    if (
        !isRecord(value) ||
        !isDecimalU64(value.revision) ||
        !THEMES.includes(value.appearance as string) ||
        !UI_VARIANTS.includes(value.uiVariant as string) ||
        !ONBOARDING_STATES.includes(value.onboarding as string) ||
        typeof value.activeFeature !== 'string' ||
        !FEATURE_IDS.includes(value.activeFeature) ||
        typeof value.showCalls !== 'boolean' ||
        !optional(
            (g) =>
                isRecord(g) &&
                typeof g.to === 'string' &&
                TENANT_ID.test(g.to) &&
                isRecord(g.pending) &&
                typeof g.pending.conversation === 'string' &&
                g.pending.conversation.length > 0 &&
                typeof g.pending.decision === 'string' &&
                g.pending.decision.length > 0 &&
                typeof g.completion === 'boolean',
            value.switchBlocked
        ) ||
        // C29: the tenant setup in progress, with its draft and verification.
        !optional(isSetupView, value.setup) ||
        !Array.isArray(value.features) ||
        typeof value.fixtureBuild !== 'boolean' ||
        !oneOf(STORAGE, value.storage) ||
        !Array.isArray(value.tenants) ||
        !value.tenants.every(isTenant) ||
        !optional(
            (a) => isRecord(a) && typeof a.tenant === 'string' && TENANT_ID.test(a.tenant) && typeof a.context === 'string' && CONTEXT_REF.test(a.context),
            value.active
        ) ||
        !optional(
            (r) =>
                isRecord(r) &&
                typeof r.review === 'string' &&
                REVIEW_ID.test(r.review) &&
                typeof r.tenant === 'string' &&
                TENANT_ID.test(r.tenant) &&
                isString(r.fromOrigin) &&
                isString(r.toOrigin) &&
                isDestination(r.to) &&
                typeof r.removesScanCredential === 'boolean',
            value.endpointReview
        ) ||
        !isImportView(value.import) ||
        !optional(
            (f) => isRecord(f) && oneOf(RESET_STAGES, f.stage) && oneOf(VAULT_ERRORS, f.reason),
            value.resetFailure
        ) ||
        typeof value.durabilityUnconfirmed !== 'boolean' ||
        !Array.isArray(value.scans) ||
        !value.scans.every(isScanRecord) ||
        !isAcademyView(value.academy) ||
        !optional((l) => typeof l === 'string' && LEARNER_REF.test(l), value.learner) ||
        typeof value.academyOrigin !== 'string' ||
        !ACADEMY_ORIGIN.test(value.academyOrigin) ||
        !isOfflineView(value.academyOffline) ||
        !isChangesView(value.changes) ||
        !isSessionsView(value.sessions) ||
        !isLibraryView(value.library) ||
        !isOverviewView(value.overview) ||
        !optional(isAdminInventoryView, value.administration) ||
        !Array.isArray(value.rotations) ||
        !value.rotations.every(isRotationView) ||
        !isInboxView(value.inbox) ||
        !isWorkbenchView(value.workbench) ||
        !isGatewayView(value.gateway) ||
        !isHarnessView(value.harness) ||
        !isV1ImportView(value.v1Import) ||
        !isDiagnosticsView(value.diagnostics) ||
        (value.academy as Record<string, unknown>).state !== 'signedIn' && (value.academyOffline as Record<string, unknown>).state !== 'signedOut'
    ) {
        throw new ContractViolation('shell snapshot')
    }
    const seen = new Set<string>()
    for (const feature of value.features) {
        if (
            !isRecord(feature) ||
            typeof feature.id !== 'string' ||
            !FEATURE_IDS.includes(feature.id) ||
            seen.has(feature.id) ||
            typeof feature.label !== 'string' ||
            !isRecord(feature.availability) ||
            !(feature.shortcut === null || (typeof feature.shortcut === 'number' && feature.shortcut >= 1 && feature.shortcut <= 9)) ||
            typeof feature.sidebar !== 'boolean'
        ) {
            throw new ContractViolation('feature entry')
        }
        seen.add(feature.id)
        const availability = feature.availability
        const valid =
            availability.state === 'available' ||
            (availability.state === 'unavailable' &&
                typeof availability.reason === 'string' &&
                typeof availability.plannedChunk === 'string')
        if (!valid) throw new ContractViolation('feature availability')
    }
    if (!seen.has(value.activeFeature as string)) throw new ContractViolation('active feature')
    return value as unknown as ShellSnapshot
}

function parseMenuCommand(value: unknown): MenuCommand {
    if (!isRecord(value)) throw new ContractViolation('menu command')
    switch (value.kind) {
        case 'commandPalette':
            return { kind: 'commandPalette' }
        case 'saveDocument':
            return { kind: 'saveDocument' }
        case 'tenantSwitcher':
            return { kind: 'tenantSwitcher' }
        case 'navigate':
            if (typeof value.feature === 'string' && FEATURE_IDS.includes(value.feature)) {
                return value as unknown as MenuCommand
            }
            break
        case 'setAppearance':
            if (THEMES.includes(value.theme as string)) {
                return value as unknown as MenuCommand
            }
            break
    }
    throw new ContractViolation('menu command')
}

export function parseObservationUpdate(value: unknown): ObservationUpdate {
    if (!isRecord(value)) throw new ContractViolation('observation update')
    switch (value.kind) {
        case 'shell':
            return { kind: 'shell', snapshot: parseShellSnapshot(value.snapshot) }
        case 'menu':
            return { kind: 'menu', command: parseMenuCommand(value.command) }
        case 'resync':
            return { kind: 'resync' }
    }
    throw new ContractViolation('observation update')
}

export interface ShellObserver {
    onSnapshot(snapshot: ShellSnapshot): void
    onMenu(command: MenuCommand): void
    onError?(error: unknown): void
}

export interface DesktopClientOptions {
    /** Resends of identical bytes after a lost admission result. */
    lostResultResends?: number
}

interface Session {
    nonce: string
    next: bigint
}

export class DesktopClient {
    readonly #transport: DesktopTransport
    readonly #resends: number
    #session: Session | null = null
    #queue: Promise<unknown> = Promise.resolve()

    constructor(transport: DesktopTransport, options: DesktopClientOptions = {}) {
        this.#transport = transport
        this.#resends = options.lostResultResends ?? 1
    }

    /** Delivers one intent; resolves with the native receipt or refusal. */
    dispatch(intent: Intent): Promise<DispatchResponse> {
        const run = this.#queue.then(() => this.#deliver(intent))
        this.#queue = run.catch(() => undefined)
        return run
    }

    /**
     * Waits for the single result of an admitted learner call. Not an
     * admission: it may be called concurrently with dispatches.
     */
    async observeAcademyCall(operation: string): Promise<AcademyCallResult> {
        if (!OPERATION_ID.test(operation)) throw new ContractViolation('operation reference')
        let response: unknown
        try {
            response = await this.#transport.observe(
                encode({ protocol: PROTOCOL, scope: { kind: 'academyCall', operation } })
            )
        } catch (error) {
            // The native command rejects with a refusal name.
            if (typeof error === 'string' && (REFUSALS as readonly string[]).includes(error)) {
                throw new RefusedError(error as Refusal)
            }
            throw error
        }
        if (isRecord(response) && response.kind === 'academyCall') return parseAcademyCallResult(response.result)
        if (isRecord(response) && response.kind === 'refusal') throw new RefusedError(asRefusal(response.refusal))
        throw new ContractViolation('academy call observation')
    }

    /**
     * Waits for the single result of an admitted management read. Not an
     * admission: it may be called concurrently with dispatches.
     */
    async observeManagement(operation: string): Promise<ManagementResult> {
        if (!OPERATION_ID.test(operation)) throw new ContractViolation('operation reference')
        let response: unknown
        try {
            response = await this.#transport.observe(
                encode({ protocol: PROTOCOL, scope: { kind: 'management', operation } })
            )
        } catch (error) {
            if (typeof error === 'string' && (REFUSALS as readonly string[]).includes(error)) {
                throw new RefusedError(error as Refusal)
            }
            throw error
        }
        if (isRecord(response) && response.kind === 'management') return parseManagementResult(response.result)
        if (isRecord(response) && response.kind === 'refusal') throw new RefusedError(asRefusal(response.refusal))
        throw new ContractViolation('management observation')
    }

    /**
     * Waits for the single result of an admitted API Client request. Not an
     * admission: it may be called concurrently with dispatches.
     */
    async observeWorkbench(operation: string): Promise<WorkbenchResult> {
        if (!OPERATION_ID.test(operation)) throw new ContractViolation('operation reference')
        let response: unknown
        try {
            response = await this.#transport.observe(
                encode({ protocol: PROTOCOL, scope: { kind: 'workbench', operation } })
            )
        } catch (error) {
            if (typeof error === 'string' && (REFUSALS as readonly string[]).includes(error)) {
                throw new RefusedError(error as Refusal)
            }
            throw error
        }
        if (isRecord(response) && response.kind === 'workbench') return parseWorkbenchResult(response.result)
        if (isRecord(response) && response.kind === 'refusal') throw new RefusedError(asRefusal(response.refusal))
        throw new ContractViolation('workbench observation')
    }

    async #register(): Promise<Session> {
        const response = await this.#transport.observe(
            encode({ protocol: PROTOCOL, scope: { kind: 'deliverySession' } })
        )
        if (isRecord(response) && response.kind === 'deliverySession' && !isDecimalU64(response.nextSequence)) {
            throw new ContractViolation('delivery session sequence')
        }
        if (isRecord(response) && response.kind === 'refusal') {
            throw new RefusedError(asRefusal(response.refusal))
        }
        if (
            !isRecord(response) ||
            response.kind !== 'deliverySession' ||
            typeof response.session !== 'string' ||
            response.session.length === 0
        ) {
            throw new ContractViolation('delivery session')
        }
        return { nonce: response.session, next: asSequence(response.nextSequence) }
    }

    async #deliver(intent: Intent): Promise<DispatchResponse> {
        if (this.#session === null || this.#session.next > U64_MAX) {
            this.#session = await this.#register()
        }
        const session = this.#session
        const bytes = encode({
            protocol: PROTOCOL,
            session: session.nonce,
            sequence: session.next.toString(),
            intent,
        })
        for (let attempt = 0; attempt <= this.#resends; attempt += 1) {
            let raw: unknown
            try {
                raw = await this.#transport.dispatch(bytes)
            } catch {
                continue
            }
            // A result arrived: the sequence is consumed whatever it says.
            session.next += 1n
            return parseDispatchResponse(raw)
        }
        // Ambiguous delivery: never reuse this sequence for other input.
        this.#session = null
        throw new DeliveryOutcomeUnknown(intent.kind)
    }

    /**
     * Observes the shell. On native resync (overflow or channel end) it
     * observes again and delivers a fresh snapshot; stale revisions are
     * ignored. Every replaced, failed or stopped observation is closed on
     * both sides: the JavaScript callback is removed and the native task is
     * told to stop. Stopping observation never cancels native work.
     */
    async observeShell(observer: ShellObserver): Promise<() => void> {
        let stopped = false
        let latest = -1n
        let channel: { token: string; close(): void } | null = null

        const retire = (current: { token: string; close(): void } | null) => {
            if (!current) return
            current.close()
            void this.#transport
                .observe(encode({ protocol: PROTOCOL, scope: { kind: 'closeShell', updates: current.token } }))
                .catch(() => undefined)
        }

        const deliver = (snapshot: ShellSnapshot) => {
            const revision = BigInt(snapshot.revision)
            if (revision <= latest) return
            latest = revision
            observer.onSnapshot(snapshot)
        }

        const open = async (): Promise<void> => {
            retire(channel)
            channel = null
            const current = this.#transport.openChannel((raw) => {
                if (stopped || channel !== current) return
                try {
                    const update = parseObservationUpdate(raw)
                    if (update.kind === 'shell') deliver(update.snapshot)
                    else if (update.kind === 'menu') observer.onMenu(update.command)
                    else void open().catch((e: unknown) => observer.onError?.(e))
                } catch (error) {
                    observer.onError?.(error)
                }
            })
            channel = current
            let response: unknown
            try {
                response = await this.#transport.observe(
                    encode({ protocol: PROTOCOL, scope: { kind: 'shell', updates: current.token } })
                )
            } catch (error) {
                retire(current)
                if (channel === current) channel = null
                throw error
            }
            if (!isRecord(response) || response.kind !== 'shell') {
                retire(current)
                if (channel === current) channel = null
                if (isRecord(response) && response.kind === 'refusal') {
                    throw new RefusedError(asRefusal(response.refusal))
                }
                throw new ContractViolation('shell observation')
            }
            let snapshot: ShellSnapshot
            try {
                snapshot = parseShellSnapshot(response.snapshot)
            } catch (error) {
                retire(current)
                if (channel === current) channel = null
                throw error
            }
            if (!stopped) deliver(snapshot)
        }

        await open()
        return () => {
            stopped = true
            retire(channel)
            channel = null
        }
    }
}
