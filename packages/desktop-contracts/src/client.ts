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
import type { DispatchResponse } from './generated/DispatchResponse'
import type { Intent } from './generated/Intent'
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
        optional((f) => oneOf(VAULT_ERRORS, f), value.scan.failure)
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
        typeof value.receipt.request === 'string'
    ) {
        return {
            outcome: 'receipt',
            receipt: { kind: 'admitted', request: value.receipt.request },
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
        typeof value.durabilityUnconfirmed !== 'boolean'
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
