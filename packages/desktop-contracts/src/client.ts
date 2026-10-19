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
]

const DECIMAL_U64 = /^(0|[1-9][0-9]{0,19})$/
const U64_MAX = 18446744073709551615n

const FEATURE_IDS: readonly string[] = [
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
]
const CHANGE_ACTIONS: readonly string[] = ['create', 'update', 'delete', 'version', 'rotate']
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
]
const STAGED_STATES: readonly string[] = ['choosing', 'ready', 'failed']
const STAGED_FAILURES: readonly string[] = ['tooLarge', 'notText', 'empty', 'unreadable']
const STAGED_TOKEN = /^sf-[0-9a-f]{16}$/
const REVIEW_STATES: readonly string[] = ['preparing', 'ready', 'confirming', 'expired', 'failed']
const READBACKS: readonly string[] = ['found', 'absent', 'incomplete', 'failed']
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

function isTenant(value: unknown): boolean {
    return (
        isRecord(value) &&
        typeof value.id === 'string' &&
        TENANT_ID.test(value.id) &&
        isString(value.label) &&
        optional(isString, value.tsgId) &&
        isDestination(value.destination) &&
        isString(value.origin) &&
        isRecord(value.scan) &&
        oneOf(SCAN_STATES, value.scan.state) &&
        optional((m) => oneOf(MODES, m), value.scan.mode) &&
        optional((f) => oneOf(VAULT_ERRORS, f), value.scan.failure) &&
        isManagementView(value.management)
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
        value.responseDetected.every(isDetection)
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
                /^user_[A-Za-z0-9_]{1,123}$/.test(value.accountId) &&
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
    if (value.kind === 'conflict' || value.kind === 'expired' || value.kind === 'capacity') return true
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
        optional(isString, value.confirmation)
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
        oneOf(SECRET_STATES, value.secret)
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
const EXPORT_KINDS: readonly string[] = ['reports', 'investigation', 'cases']
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
        optional((e) => isExportView(e) && (e as Record<string, unknown>).kind !== 'cases', value.export)
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
const INBOX_DESTINATIONS: readonly string[] = ['apiKeys', 'sessions']
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
        value.gaps.every(isString)
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
        value.held.every((h) => isRecord(h) && isInboxItem(h.item) && oneOf(HOLD_KINDS, h.kind)) &&
        oneOf(FACES, value.face) &&
        // All clear only from a fresh read with nothing open.
        (value.face !== 'allClear' || (value.fresh === true && value.attention.length === 0))
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
            !isRecord(feature.availability)
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
    return value as unknown as ShellSnapshot
}

function parseMenuCommand(value: unknown): MenuCommand {
    if (!isRecord(value)) throw new ContractViolation('menu command')
    switch (value.kind) {
        case 'commandPalette':
            return { kind: 'commandPalette' }
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
