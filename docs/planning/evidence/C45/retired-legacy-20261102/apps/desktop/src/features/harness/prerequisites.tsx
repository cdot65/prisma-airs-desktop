import { useId } from 'react'
import { Button } from '@ai-security-academy/presentation'
import { CheckCircle2, Circle } from 'lucide-react'
import type { FeatureId } from '@prisma-airs/desktop-contracts'
import { SIGN_IN_NEEDS } from './sign-in-panel'

interface Props {
    /** A tenant is chosen. */
    tenant: boolean
    /** The tenant's management client is configured. */
    management: boolean
    /** The chosen Gateway workspace, by name. */
    workspace: string | null
    /** An inference key or company sign-in is set up for that workspace. */
    credential: boolean
    onNavigate: (id: FeatureId) => void
}

interface Step {
    id: string
    done: boolean
    label: string
    detail: string
    action: { label: string; to: FeatureId } | null
}

/**
 * What the Harness needs before a conversation, in order, each with its
 * current state and where to set it up.
 */
export function Prerequisites({ tenant, management, workspace, credential, onNavigate }: Props) {
    const ids = useId()
    const steps: Step[] = [
        {
            id: 'tenant',
            done: tenant,
            label: 'A tenant chosen',
            detail: 'Choose or add one in Settings.',
            action: tenant ? null : { label: 'Go to Settings', to: 'settings' },
        },
        {
            id: 'management',
            done: management,
            label: 'A management client for the tenant',
            detail: 'Configure it in Settings; AI Gateway reads the workspaces with it.',
            action: tenant && !management ? { label: 'Go to Settings', to: 'settings' } : null,
        },
        {
            id: 'workspace',
            done: workspace !== null,
            label: workspace ? `A Gateway workspace chosen: ${workspace}` : 'A Gateway workspace chosen',
            detail: 'Choose it in AI Gateway.',
            action: tenant && workspace === null ? { label: 'Go to AI Gateway', to: 'gateway' } : null,
        },
        {
            id: 'credential',
            done: credential,
            label: 'An inference key for that workspace, or company sign-in (SSO)',
            detail: `Store the key in the AI Gateway Playground, or set up company sign-in below once a workspace is chosen. ${SIGN_IN_NEEDS}`,
            action: null,
        },
    ]
    return (
        <section aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3' data-slot='harness-prerequisites'>
            <h2 id={`${ids}-heading`} className='text-[15px] font-semibold'>
                What you need first
            </h2>
            {/* role='list': WebKit drops list semantics from an unstyled list. */}
            <ol role='list' aria-labelledby={`${ids}-heading`} className='flex flex-col gap-3'>
                {steps.map((step) => {
                    const Icon = step.done ? CheckCircle2 : Circle
                    return (
                        <li key={step.id} className='flex gap-3' data-slot={`harness-prerequisite-${step.id}`} data-done={step.done}>
                            <Icon
                                aria-hidden='true'
                                className={step.done ? 'text-academy-accent mt-0.5 size-4 shrink-0' : 'text-academy-ink-faint mt-0.5 size-4 shrink-0'}
                            />
                            <div className='flex flex-col gap-1'>
                                <p className='text-[13px]'>
                                    <span className='sr-only'>{step.done ? 'Done: ' : 'Not yet: '}</span>
                                    {step.label}
                                </p>
                                {step.done ? null : <p className='text-academy-ink-faint text-[12px]'>{step.detail}</p>}
                                {step.action ? (
                                    <Button
                                        type='button'
                                        tier='secondary'
                                        size='sm'
                                        className='self-start'
                                        onClick={() => step.action && onNavigate(step.action.to)}
                                    >
                                        {step.action.label}
                                    </Button>
                                ) : null}
                            </div>
                        </li>
                    )
                })}
            </ol>
        </section>
    )
}
