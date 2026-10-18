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

/// Public catalog procedures (Academy `publicProcedure`) the desktop calls
/// with no credential: for guests, and for the two public-only module media
/// procedures that Academy's desktop-token allowlist does not include. All
/// are queries.
pub const PUBLIC_PROCEDURES: &[&str] = &[
    "courses.list",
    "library.list",
    "modules.diagramVideo",
    "modules.intro",
    "modules.listByCourse",
    "paths.forCourse",
    "paths.get",
    "paths.list",
    "units.get",
    "units.listByModule",
    "units.listByPath",
    "units.moduleMeta",
];

/// True for a public catalog procedure.
pub fn is_public(procedure: &str) -> bool {
    PUBLIC_PROCEDURES.binary_search(&procedure).is_ok()
}

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
        /// Academy's `user_…` account id (not a credential); learner pages
        /// key per-learner local state by it.
        account_id: String,
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
    fn the_public_set_is_sorted_and_contains_only_queries() {
        assert!(PUBLIC_PROCEDURES.windows(2).all(|w| w[0] < w[1]));
        for p in PUBLIC_PROCEDURES {
            assert!(procedure_kind(p).is_none_or(|k| k == Query), "{p}");
        }
        assert!(
            is_public("modules.intro")
                && !is_public("units.complete")
                && !is_public("progress.get")
        );
    }

    /// The TypeScript link suite and the shipped-procedure check read the
    /// same sets from this fixture.
    #[test]
    fn the_shared_procedure_fixture_matches_the_pinned_sets() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/desktop-contracts/fixtures/academy/learner-procedures.json");
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json");
        let learner: Vec<(String, String)> = LEARNER_PROCEDURES
            .iter()
            .map(|(name, kind)| {
                let kind = if *kind == Query { "query" } else { "mutation" };
                ((*name).to_owned(), kind.to_owned())
            })
            .collect();
        let listed: Vec<(String, String)> = fixture["learner"]
            .as_array()
            .expect("learner")
            .iter()
            .map(|e| {
                (
                    e["procedure"].as_str().expect("procedure").to_owned(),
                    e["kind"].as_str().expect("kind").to_owned(),
                )
            })
            .collect();
        assert_eq!(listed, learner);
        let public: Vec<&str> = fixture["public"]
            .as_array()
            .expect("public")
            .iter()
            .map(|p| p.as_str().expect("name"))
            .collect();
        assert_eq!(public, PUBLIC_PROCEDURES);
    }

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
