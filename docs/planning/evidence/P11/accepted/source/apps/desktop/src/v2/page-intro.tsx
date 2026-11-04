import type { ReactNode } from 'react'

/** A page's description and controls; its title belongs to the shell header. */
export function PageIntro({ description, actions, status, className = '' }: { description: ReactNode; actions?: ReactNode; status?: ReactNode; className?: string }) {
    return (
        <div data-slot='page-intro' className={`flex min-w-0 flex-wrap items-center ${className}`} style={{ gap: '10px 16px' }}>
            <div data-slot='page-description' className='min-w-0 text-[14px]' style={{ flex: '1 1 20rem', maxWidth: '52rem', color: 'var(--v2-soft)', overflowWrap: 'anywhere' }}>
                {description}
            </div>
            {actions && (
                <div data-slot='page-actions' className='ml-auto flex min-w-0 flex-wrap items-center' style={{ maxWidth: '100%', gap: 8 }}>
                    {actions}
                </div>
            )}
            {status && (
                <div data-slot='page-intro-status' className='w-full min-w-0'>
                    {status}
                </div>
            )}
        </div>
    )
}
