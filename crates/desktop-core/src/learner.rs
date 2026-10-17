//! The Academy learner procedure allowlist and learner-facing views.
//!
//! The allowlist is pinned to the reviewed Academy revision (891f65ee, base
//! 7f3a28c4): `LEARNER_PROCEDURES` in `src/server/services/desktop-auth.ts`,
//! with each procedure's kind read from its router. Academy's server enforces
//! the same set for desktop tokens; the desktop refuses anything else before a
//! request is made, including manager, assessor and administrative
//! procedures (the learner `admin.feedback.submit` is listed explicitly).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::academy::AcademyFailure;
use crate::context::LearnerRef;
use crate::vault::VaultError;

/// A tRPC procedure kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProcedureKind {
    Query,
    Mutation,
}

use ProcedureKind::{Mutation, Query};

/// Every learner procedure the desktop may call, with its kind.
pub const LEARNER_PROCEDURES: &[(&str, ProcedureKind)] = &[
    ("admin.feedback.submit", Mutation),
    ("caches.list", Query),
    ("caches.open", Mutation),
    ("certificate.get", Query),
    ("certificate.mine", Query),
    ("certifications.list", Query),
    ("certifications.lockedTiers", Query),
    ("courses.list", Query),
    ("dashboard.summary", Query),
    ("desktop.access", Query),
    ("desktop.attempt.checkStep", Mutation),
    ("desktop.attempt.critique", Mutation),
    ("desktop.attempt.get", Query),
    ("desktop.attempt.hint", Mutation),
    ("desktop.attempt.list", Query),
    ("desktop.attempt.start", Mutation),
    ("desktop.attempt.submit", Mutation),
    ("desktop.bundle", Query),
    ("desktop.draft.resolve", Mutation),
    ("desktop.draft.save", Mutation),
    ("desktop.handbook", Query),
    ("desktop.manifest", Query),
    ("desktop.me", Query),
    ("desktop.organization.select", Mutation),
    ("desktop.organizations", Query),
    ("desktop.unitBundle", Query),
    ("enrollment.affectedByCourseRemoval", Query),
    ("enrollment.removeByCourse", Mutation),
    ("exam.acknowledgeAward", Mutation),
    ("exam.answer", Mutation),
    ("exam.eligibility", Query),
    ("exam.execTerminalItem", Mutation),
    ("exam.getActive", Query),
    ("exam.result", Query),
    ("exam.start", Mutation),
    ("exam.submissionState", Query),
    ("exam.submit", Mutation),
    ("gamification.dossier", Query),
    ("gamification.leaderboard", Query),
    ("gamification.setDailyGoal", Mutation),
    ("gamification.setTimezone", Mutation),
    ("gamification.summary", Query),
    ("identityPractical.assign", Mutation),
    ("identityPractical.claimCertificate", Mutation),
    ("identityPractical.status", Query),
    ("identityPractical.submit", Mutation),
    ("labs.get", Query),
    ("labs.list", Query),
    ("labs.unitFor", Query),
    ("library.list", Query),
    ("meta.contentVersion", Query),
    ("notifications.list", Query),
    ("notifications.markAllRead", Mutation),
    ("notifications.markRead", Mutation),
    ("notifications.unreadCount", Query),
    ("onboarding.complete", Mutation),
    ("onboarding.status", Query),
    ("orientation.complete", Mutation),
    ("orientation.get", Query),
    ("orientation.markHintSeen", Mutation),
    ("paths.forCourse", Query),
    ("paths.get", Query),
    ("paths.list", Query),
    ("progress.checkInline", Mutation),
    ("progress.courseSummary", Query),
    ("progress.get", Query),
    ("progress.resetCourse", Mutation),
    ("seasons.current", Query),
    ("settings.avatarUploadUrl", Mutation),
    ("settings.confirmAvatar", Mutation),
    ("settings.get", Query),
    ("settings.removeAvatar", Mutation),
    ("settings.update", Mutation),
    ("shift.answer", Mutation),
    ("shift.briefing", Query),
    ("shift.daily", Query),
    ("shift.dailyBoard", Query),
    ("shift.dailyGhost", Query),
    ("shift.debrief", Query),
    ("shift.decide", Mutation),
    ("shift.eligibility", Query),
    ("shift.getActive", Query),
    ("shift.history", Query),
    ("shift.serve", Mutation),
    ("shift.start", Mutation),
    ("shift.yesterday", Query),
    ("shop.buy", Mutation),
    ("shop.equip", Mutation),
    ("shop.list", Query),
    ("tutor.critique", Mutation),
    ("units.certOverview", Query),
    ("units.checkKcAnswer", Mutation),
    ("units.complete", Mutation),
    ("units.get", Query),
    ("units.listByModule", Query),
    ("units.listByPath", Query),
    ("units.moduleMeta", Query),
    ("units.startKnowledgeCheck", Mutation),
    ("units.submitKnowledgeCheck", Mutation),
];

/// The kind of an allowed learner procedure.
pub fn procedure_kind(procedure: &str) -> Option<ProcedureKind> {
    LEARNER_PROCEDURES
        .binary_search_by(|(name, _)| name.cmp(&procedure))
        .ok()
        .map(|i| LEARNER_PROCEDURES[i].1)
}

/// The Academy account state the presentation may show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AcademyView {
    SignedOut,
    /// Fetching configuration and starting device authorization.
    Starting,
    /// Waiting for the learner to approve in the browser.
    #[serde(rename_all = "camelCase")]
    AwaitingApproval {
        user_code: String,
        verification_uri: String,
        /// Seconds until the code expires, as of this snapshot.
        expires_in: u32,
    },
    /// Approved; identifying the account and saving the session.
    Completing,
    #[serde(rename_all = "camelCase")]
    SignedIn {
        display_name: String,
        learner: LearnerRef,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        failure: AcademyFailure,
        vault: Option<VaultError>,
        /// A saved account whose session needs signing in again.
        display_name: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowlist_is_sorted_unique_and_learner_only() {
        assert!(LEARNER_PROCEDURES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(LEARNER_PROCEDURES.len(), 99);
        assert_eq!(procedure_kind("desktop.me"), Some(Query));
        assert_eq!(procedure_kind("desktop.attempt.submit"), Some(Mutation));
        assert_eq!(procedure_kind("admin.feedback.submit"), Some(Mutation));
        assert_eq!(procedure_kind("shift.debrief"), Some(Query));
        for denied in [
            "admin.feedback.list",
            "admin.users.list",
            "exam.grade",
            "desktop",
            "",
            "units.get.x",
        ] {
            assert_eq!(procedure_kind(denied), None, "{denied}");
        }
    }
}
