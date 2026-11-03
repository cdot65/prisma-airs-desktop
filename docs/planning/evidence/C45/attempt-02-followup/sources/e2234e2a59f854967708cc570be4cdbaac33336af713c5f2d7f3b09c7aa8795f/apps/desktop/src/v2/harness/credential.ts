import type { ShellSnapshot } from '@prisma-airs/desktop-contracts'

/**
 * What the Harness would send with, as the Harness screen itself decides
 * it (C32): a company sign-in counts only for the selected Gateway
 * workspace and only while chosen over the key; the workspace key counts
 * only when its target is the selected workspace. The shell's status row
 * and the tenant menu read this one derivation (C35 R1), never the sign-in
 * view alone.
 */
export interface HarnessCredential {
    /** The source the Harness would use for the selected workspace. */
    source: 'sso' | 'key' | 'none'
    /** The signed-in account, when the source is the sign-in and it is signed in. */
    account: string | null
    /** Usable now: an account without a failure, or a ready key. */
    ready: boolean
    /** The sign-in is chosen but failed (a renewal is needed before anything is sent). */
    failed: boolean
    /** A Gateway workspace is selected under the active context. */
    workspace: boolean
}

export function harnessCredential(snapshot: ShellSnapshot): HarnessCredential {
    const selected = snapshot.gateway.selected
    const tenant = snapshot.tenants.find((t) => t.id === snapshot.active?.tenant) ?? null
    const key = tenant?.inference
    const signIn = snapshot.harness.signIn
    const keyHere = key?.state === 'ready' && selected !== null && key.target?.workspace.id === selected.workspace.id
    const signInHere = signIn !== null && selected !== null && signIn.workspace.id === selected.workspace.id && signIn.selected
    if (signInHere) {
        return { source: 'sso', account: signIn.account, ready: signIn.account !== null && signIn.failure === null, failed: signIn.failure !== null, workspace: true }
    }
    if (keyHere) return { source: 'key', account: null, ready: true, failed: false, workspace: true }
    return { source: 'none', account: null, ready: false, failed: false, workspace: selected !== null }
}
