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
    type LucideIcon,
} from 'lucide-react'
import type { FeatureId } from '@prisma-airs/desktop-contracts'

export const FEATURE_ICONS: Record<FeatureId, LucideIcon> = {
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
