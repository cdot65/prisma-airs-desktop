import {
    Boxes,
    BookOpen,
    Bot,
    FileSearch,
    FolderInput,
    GraduationCap,
    Network,
    Settings,
    ShieldCheck,
    Swords,
    Terminal,
    House,
    Inbox,
    type LucideIcon,
} from 'lucide-react'
import type { FeatureId } from '@prisma-airs/desktop-contracts'

export const FEATURE_ICONS: Record<FeatureId, LucideIcon> = {
    home: House,
    inbox: Inbox,
    runtime: ShieldCheck,
    academy: GraduationCap,
    administration: Boxes,
    sessions: FileSearch,
    library: BookOpen,
    redTeam: Swords,
    apiClient: Terminal,
    gateway: Network,
    harness: Bot,
    import: FolderInput,
    settings: Settings,
}
