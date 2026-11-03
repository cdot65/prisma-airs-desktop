//! What v1 has (inspection: files only) and what the operator's selection
//! reads of it (planning: the selected protected items). Destination checks
//! (already present, conflict, full) are the authority's, against v2's
//! owners.

use serde::Serialize;
use sha2::{Digest, Sha256};
use ts_rs::TS;
use zeroize::Zeroizing;

use super::parse::{self, V1Snapshot, V1Tenant, V1Tenants};
use super::source::{self, V1Source};
use super::{Category, Problem};
use crate::tenants::{Destination, Region};

/// Inspection's view of v1's folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SourceState {
    /// No v1 folder on this Mac.
    NotFound,
    /// v1 is running: close it first.
    Locked,
    /// The folder or its tenants could not be read.
    Unavailable,
    Ready,
}

/// One v1 tenant as inspection shows it (no secret, no Keychain read).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct V1TenantView {
    pub index: u32,
    pub label: String,
    pub tsg_id: Option<String>,
    /// Session-only in v1: nothing of it was stored.
    pub session_only: bool,
}

/// A category as inspection sees it: what files show, or why not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CategoryView {
    pub category: Category,
    /// Items found in files; `None` when only a protected read can tell.
    pub found: Option<u32>,
    pub problem: Option<Problem>,
}

pub struct Inventory {
    pub state: SourceState,
    pub tenants: Option<V1Tenants>,
    pub categories: Vec<CategoryView>,
    /// v1's settings with no v2 equivalent (shown, never read).
    pub not_mapped: Vec<String>,
}

fn tsg(tenant: &V1Tenant) -> Option<String> {
    let t = tenant.tsg_id.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

impl Inventory {
    pub fn tenant_views(&self) -> Vec<V1TenantView> {
        self.tenants
            .iter()
            .flat_map(|t| t.tenants.iter())
            .enumerate()
            .map(|(i, t)| V1TenantView {
                index: u32::try_from(i).unwrap_or(u32::MAX),
                label: t.name.clone(),
                tsg_id: tsg(t),
                session_only: t.storage == parse::V1Storage::Session,
            })
            .collect()
    }
}

/// Reads v1's folder without touching its Keychain.
pub fn inventory(src: &V1Source) -> Inventory {
    let none = |state| Inventory {
        state,
        tenants: None,
        categories: Vec::new(),
        not_mapped: Vec::new(),
    };
    match source::root_state(&src.root) {
        Ok(()) => {}
        Err(Problem::Missing) => return none(SourceState::NotFound),
        Err(_) => return none(SourceState::Unavailable),
    }
    let lock = match source::lock(&src.root) {
        Ok(lock) => lock,
        Err(Problem::SourceLocked) => return none(SourceState::Locked),
        Err(_) => return none(SourceState::Unavailable),
    };
    let tenants = source::read_file(&src.root, &[parse::TENANTS_FILE], parse::TENANTS_LIMIT)
        .and_then(|b| parse::tenants(&b));
    let mut categories = Vec::new();
    let mut view = |category, found: Result<Option<u32>, Problem>| {
        categories.push(CategoryView {
            category,
            found: found.as_ref().ok().copied().flatten(),
            problem: found.err(),
        });
    };
    let count = |n: usize| Some(u32::try_from(n).unwrap_or(u32::MAX));
    let listed = tenants.as_ref().map_err(|p| *p);
    view(Category::Tenants, listed.map(|t| count(t.tenants.len())));
    view(Category::Endpoints, listed.map(|_| None));
    view(
        Category::Settings,
        file_present(src, parse::APPEARANCE_FILE).and_then(|p| {
            if p {
                Ok(Some(1))
            } else {
                Err(Problem::Missing)
            }
        }),
    );
    view(Category::Cases, listed.map(|_| None));
    view(
        Category::Reports,
        source::read_file(&src.root, &[parse::REPORTS_FILE], parse::REPORTS_LIMIT)
            .and_then(|b| parse::reports(&b))
            .map(|r| count(r.iter().map(|t| t.reports.len() + t.unfit).sum())),
    );
    view(
        Category::AcademyDrafts,
        source::list(
            &src.root,
            &[parse::ACADEMY_DIR],
            true,
            256,
            parse::is_digest,
        )
        .map(|d| count(d.len())),
    );
    view(
        Category::ApiClient,
        crate::workbench::adoption::read(&src.root.join("api-client"))
            .map(|c| count(c.len()))
            .map_err(|p| match p {
                crate::workbench::adoption::AdoptionProblem::Missing => Problem::Missing,
                crate::workbench::adoption::AdoptionProblem::Unreadable => Problem::Corrupt,
            }),
    );
    view(Category::Conversations, listed.map(|_| None));
    let mut not_mapped = Vec::new();
    if file_present(src, parse::NAVIGATION_FILE) == Ok(true) {
        not_mapped.push("navigation".to_owned());
    }
    drop(lock);
    Inventory {
        state: if tenants.is_ok() || matches!(tenants, Err(Problem::Missing)) {
            SourceState::Ready
        } else {
            SourceState::Unavailable
        },
        tenants: tenants.ok(),
        categories,
        not_mapped,
    }
}

fn file_present(src: &V1Source, name: &str) -> Result<bool, Problem> {
    source::exists(&src.root, &[name])
}

/// Where a v1 tenant's work goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A new keyless v2 tenant.
    New,
    /// An existing v2 tenant (its TSG must match).
    Existing(crate::tenants::TenantId),
    Skip,
}

/// What a candidate would write, decoded from v1.
pub enum Payload {
    Tenant {
        label: String,
        tsg_id: Option<String>,
        destination: Destination,
        /// Why v1's scan endpoint was not used (the default region is),
        /// when endpoints were selected.
        endpoint_problem: Option<Problem>,
    },
    Appearance(crate::wire::Theme),
    Cases(Vec<crate::library::TestCase>),
    Reports {
        reports: Vec<crate::sessions::Report>,
        unfit: usize,
    },
    Drafts {
        owner_digest: String,
        drafts: Vec<parse::V1Draft>,
        skipped: usize,
    },
    ApiClient(Vec<crate::workbench::adoption::Candidate>),
    Conversations(Vec<V1Snapshot>),
}

/// One item read from v1 for review, before the destination is checked.
pub struct Candidate {
    pub category: Category,
    /// The v1 tenant it belongs to (index into inspection's list).
    pub tenant: Option<u32>,
    pub label: String,
    /// SHA-256 of the v1 bytes it was read from.
    pub source_hash: String,
    pub payload: Result<Payload, Problem>,
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A v1 scan endpoint as a v2 destination: one of the regions, or a plain
/// HTTPS origin (`https://host[:port]`); anything else is not mappable.
pub fn destination(endpoint: &str) -> Result<Destination, Problem> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Ok(Destination::Region {
            region: Region::UnitedStates,
        });
    }
    let origin = crate::tenants::normalize_origin(endpoint).ok_or(Problem::EndpointNotMappable)?;
    for region in [
        Region::UnitedStates,
        Region::Europe,
        Region::India,
        Region::Singapore,
    ] {
        if region.origin() == origin {
            return Ok(Destination::Region { region });
        }
    }
    Ok(Destination::Custom { origin })
}

fn default_destination() -> Destination {
    Destination::Region {
        region: Region::UnitedStates,
    }
}

/// The account digest of a signed-in Academy learner (v1's directory name).
pub fn learner_digest(account: &str) -> String {
    parse::digest(account)
}

/// Reads what the selection needs from v1. Protected items are read only
/// for selected protected categories, and only for tenants with a target.
pub fn read(
    src: &V1Source,
    tenants: &V1Tenants,
    targets: &[Target],
    categories: &[Category],
    learner: Option<&str>,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    let selected = |c: Category| categories.contains(&c);
    for (i, tenant) in tenants.tenants.iter().enumerate() {
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        let target = targets.get(i).cloned().unwrap_or(Target::Skip);
        let candidate = |category, source_hash: String, payload| Candidate {
            category,
            tenant: Some(index),
            label: tenant.name.clone(),
            source_hash,
            payload,
        };
        if target == Target::Skip {
            for c in [Category::Tenants, Category::Cases, Category::Conversations] {
                if selected(c) {
                    out.push(candidate(c, String::new(), Err(Problem::NoTarget)));
                }
            }
            continue;
        }
        if selected(Category::Tenants) && target == Target::New {
            // An endpoint that cannot be read or mapped leaves the default
            // region, and says why; the tenant is still imported.
            let (destination, endpoint_problem, hash) = if selected(Category::Endpoints) {
                match endpoint_of(src, tenant) {
                    Ok((d, h)) => (d, None, h),
                    Err(p) => (default_destination(), Some(p), String::new()),
                }
            } else {
                (default_destination(), None, String::new())
            };
            let payload = Ok(Payload::Tenant {
                label: tenant.name.clone(),
                tsg_id: tsg(tenant),
                destination,
                endpoint_problem,
            });
            out.push(candidate(Category::Tenants, hash, payload));
        }
        let protected_tenant = tenant.storage == parse::V1Storage::Protected;
        if selected(Category::Cases) {
            let account = format!("cases-{}", tenant.id);
            let read = if !protected_tenant {
                Err(Problem::NoStoredWork)
            } else if tenants.retired.contains(&account) {
                Err(Problem::SourceRetired)
            } else {
                match source::read_item(src, &account, parse::CASES_LIMIT) {
                    Ok(Some(bytes)) => parse::cases(&bytes, &tenant.id)
                        .map(|cases| (sha(&bytes), Payload::Cases(cases))),
                    Ok(None) => Err(Problem::Missing),
                    Err(p) => Err(p),
                }
            };
            let (hash, payload) = split(read);
            out.push(candidate(Category::Cases, hash, payload));
        }
        if selected(Category::Conversations) {
            let read = match tenant.credential_ref.as_deref() {
                None => Err(Problem::NoStoredWork),
                Some(_) if !protected_tenant => Err(Problem::NoStoredWork),
                Some(reference) => {
                    let account = format!("harness-history-{reference}");
                    if tenants.retired.contains(&account) {
                        Err(Problem::SourceRetired)
                    } else {
                        match source::read_item(src, &account, parse::HISTORY_LIMIT) {
                            Ok(Some(bytes)) => parse::history(&bytes, &tenant.id)
                                .map(|c| (sha(&bytes), Payload::Conversations(c))),
                            Ok(None) => Err(Problem::Missing),
                            Err(p) => Err(p),
                        }
                    }
                }
            };
            let (hash, payload) = split(read);
            out.push(candidate(Category::Conversations, hash, payload));
        }
    }
    if selected(Category::Reports) {
        out.extend(reports(src, tenants, targets));
    }
    if selected(Category::Settings) {
        let read = source::read_file(
            &src.root,
            &[parse::APPEARANCE_FILE],
            parse::APPEARANCE_LIMIT,
        )
        .and_then(|b| parse::appearance(&b).map(|t| (sha(&b), Payload::Appearance(t))));
        let (hash, payload) = split(read);
        out.push(Candidate {
            category: Category::Settings,
            tenant: None,
            label: "Appearance".to_owned(),
            source_hash: hash,
            payload,
        });
    }
    if selected(Category::AcademyDrafts) {
        out.push(drafts(src, learner));
    }
    if selected(Category::ApiClient) {
        let read = crate::workbench::adoption::read(&src.root.join("api-client"))
            .map_err(|p| match p {
                crate::workbench::adoption::AdoptionProblem::Missing => Problem::Missing,
                crate::workbench::adoption::AdoptionProblem::Unreadable => Problem::Corrupt,
            })
            .map(|c| {
                (
                    super::content_hash(&api_identity(&c)),
                    Payload::ApiClient(c),
                )
            });
        let (hash, payload) = split(read);
        out.push(Candidate {
            category: Category::ApiClient,
            tenant: None,
            label: "API Client".to_owned(),
            source_hash: hash,
            payload,
        });
    }
    out
}

/// A collection's root, identity and private environments (what a review
/// of the API Client category depends on).
type ApiIdentity = (String, String, Vec<(String, String)>);

fn api_identity(candidates: &[crate::workbench::adoption::Candidate]) -> Vec<ApiIdentity> {
    candidates
        .iter()
        .map(|c| {
            (
                c.root.display().to_string(),
                c.id.clone(),
                c.environments.clone(),
            )
        })
        .collect()
}

fn split(read: Result<(String, Payload), Problem>) -> (String, Result<Payload, Problem>) {
    match read {
        Ok((hash, payload)) => (hash, Ok(payload)),
        Err(p) => (String::new(), Err(p)),
    }
}

fn endpoint_of(src: &V1Source, tenant: &V1Tenant) -> Result<(Destination, String), Problem> {
    if tenant.storage != parse::V1Storage::Protected {
        return Err(Problem::NoStoredWork);
    }
    let reference = tenant
        .credential_ref
        .as_deref()
        .ok_or(Problem::NoStoredWork)?;
    let bytes = source::read_item(
        src,
        &format!("credentials-{reference}"),
        parse::CREDENTIALS_LIMIT,
    )?
    .ok_or(Problem::Missing)?;
    let endpoint = Zeroizing::new(parse::scan_endpoint(&bytes)?);
    // The hash names the endpoint only, never the secret item's bytes.
    Ok((destination(&endpoint)?, sha(endpoint.as_bytes())))
}

fn reports(src: &V1Source, tenants: &V1Tenants, targets: &[Target]) -> Vec<Candidate> {
    let bytes = match source::read_file(&src.root, &[parse::REPORTS_FILE], parse::REPORTS_LIMIT) {
        Ok(bytes) => bytes,
        Err(p) => {
            return vec![Candidate {
                category: Category::Reports,
                tenant: None,
                label: "FP/FN reports".to_owned(),
                source_hash: String::new(),
                payload: Err(p),
            }];
        }
    };
    let hash = sha(&bytes);
    let book = match parse::reports(&bytes) {
        Ok(book) => book,
        Err(p) => {
            return vec![Candidate {
                category: Category::Reports,
                tenant: None,
                label: "FP/FN reports".to_owned(),
                source_hash: hash,
                payload: Err(p),
            }];
        }
    };
    let mut out = Vec::new();
    for entry in book {
        let index = tenants.tenants.iter().position(|t| t.id == entry.tenant);
        let label = index
            .map(|i| tenants.tenants[i].name.clone())
            .unwrap_or_else(|| "A removed v1 tenant".to_owned());
        let payload = match index.map(|i| targets.get(i).cloned().unwrap_or(Target::Skip)) {
            None => Err(Problem::NoTarget),
            Some(Target::Skip) => Err(Problem::NoTarget),
            Some(_) => Ok(Payload::Reports {
                reports: entry.reports,
                unfit: entry.unfit,
            }),
        };
        out.push(Candidate {
            category: Category::Reports,
            tenant: index.map(|i| u32::try_from(i).unwrap_or(u32::MAX)),
            label,
            source_hash: hash.clone(),
            payload,
        });
    }
    out
}

/// The signed-in learner's unsaved lab drafts. v1 keeps them sealed in a
/// directory named by the account's digest, keyed by a Keychain item.
fn drafts(src: &V1Source, learner: Option<&str>) -> Candidate {
    let candidate = |hash: String, payload| Candidate {
        category: Category::AcademyDrafts,
        tenant: None,
        label: "Academy lab drafts".to_owned(),
        source_hash: hash,
        payload,
    };
    let Some(account) = learner else {
        return candidate(String::new(), Err(Problem::SignInRequired));
    };
    let owner_digest = learner_digest(account);
    let dirs = match source::list(
        &src.root,
        &[parse::ACADEMY_DIR],
        true,
        256,
        parse::is_digest,
    ) {
        Ok(dirs) => dirs,
        Err(p) => return candidate(String::new(), Err(p)),
    };
    if !dirs.contains(&owner_digest) {
        return candidate(
            String::new(),
            Err(if dirs.is_empty() {
                Problem::Missing
            } else {
                Problem::AccountMismatch
            }),
        );
    }
    let read = (|| {
        let names = source::list(
            &src.root,
            &[parse::ACADEMY_DIR, &owner_digest],
            false,
            parse::ACADEMY_ENTRIES,
            |n| {
                n.strip_prefix("attempt-")
                    .and_then(|r| r.strip_suffix(".bin"))
                    .is_some_and(parse::is_digest)
            },
        )?;
        let key = source::read_item(src, &format!("academy-content-key-v1-{owner_digest}"), 64)?
            .ok_or(Problem::Missing)?;
        let mut hasher = Sha256::new();
        let mut drafts = Vec::new();
        let mut skipped = 0;
        for name in names {
            let bytes = source::read_file(
                &src.root,
                &[parse::ACADEMY_DIR, &owner_digest, &name],
                parse::ACADEMY_RECORD_LIMIT,
            )?;
            hasher.update(name.as_bytes());
            hasher.update(&*bytes);
            let value = parse::academy_record(&key, account, &name, &bytes)?;
            match parse::draft(&value, account)? {
                parse::DraftCheck::Draft(d) => {
                    if name != format!("attempt-{}.bin", parse::digest(&d.attempt_id)) {
                        return Err(Problem::Corrupt);
                    }
                    drafts.push(d);
                }
                _ => skipped += 1,
            }
        }
        Ok((
            hex::encode(hasher.finalize()),
            Payload::Drafts {
                owner_digest: owner_digest.clone(),
                drafts,
                skipped,
            },
        ))
    })();
    let (hash, payload) = split(read);
    candidate(hash, payload)
}

/// Re-reads the files a candidate came from (Keychain items are protected
/// from change by v1's instance lock, held from review to settlement).
pub fn file_hash(src: &V1Source, category: Category) -> Option<Result<String, Problem>> {
    let file = match category {
        Category::Reports => parse::REPORTS_FILE,
        Category::Settings => parse::APPEARANCE_FILE,
        _ => return None,
    };
    let limit = match category {
        Category::Reports => parse::REPORTS_LIMIT,
        _ => parse::APPEARANCE_LIMIT,
    };
    Some(source::read_file(&src.root, &[file], limit).map(|b| sha(&b)))
}
