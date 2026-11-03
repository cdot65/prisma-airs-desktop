import type { ShellSnapshot, SwitchResolution } from '@prisma-airs/desktop-contracts'
import { GUARD_COPY } from './copy'
import { Modal } from './modal'

export interface SwitchGuardProps {
    /** The tenant the switch (or the setup completion) leads to. */
    target: string
    /** The switch is a setup draft's completion (the body says so). */
    completion: boolean
    binding: Pick<NonNullable<ShellSnapshot['switchBlocked']>, 'context' | 'review'>
    onStay: () => void
    onResolve: (resolution: SwitchResolution) => void
}

/**
 * The one tenant-switch guard (I1): rendered from the core's `switchBlocked`
 * whether the switch was asked for through the switcher, the palette, the
 * native menu or the completion of a setup draft. Stay dismisses the guard;
 * Archive and Discard re-send the same intent with the chosen resolution.
 * Initial focus is on Stay, the safe default: Enter on arrival never ends
 * the conversation (Archive stays the visually primary action).
 */
export function SwitchGuard({ target, completion, binding, onStay, onResolve }: SwitchGuardProps) {
    return (
        <Modal label={GUARD_COPY.title} onClose={onStay} width={520} data-slot='switch-guard'>
            <h2 className='text-[16px] font-semibold'>{GUARD_COPY.title}</h2>
            <p className='mt-2 text-[13px]' style={{ color: 'var(--v2-soft)' }}>
                {completion ? GUARD_COPY.bodyCompletion(target) : GUARD_COPY.body(target)}
            </p>
            <div className='mt-4 flex flex-wrap justify-end gap-2'>
                <button type='button' autoFocus onClick={onStay} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                    {GUARD_COPY.stay}
                </button>
                <button type='button' onClick={() => onResolve({ kind: 'discard', context: binding.context, review: binding.review })} className='h-8 rounded-[7px] border px-3 text-[13px]' style={{ borderColor: 'var(--v2-rule)' }}>
                    {GUARD_COPY.discard}
                </button>
                <button type='button' onClick={() => onResolve({ kind: 'archive', context: binding.context, review: binding.review })} className='h-8 rounded-[7px] px-3 text-[13px] font-semibold' style={{ background: 'var(--v2-acc)', color: 'var(--v2-acc-ink)' }}>
                    {GUARD_COPY.archive}
                </button>
            </div>
        </Modal>
    )
}
