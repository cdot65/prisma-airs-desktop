//! Tenant setup (C29, screens/wizard.md): the first-run wizard and the
//! add-tenant sheet work on one core-owned draft tenant with its own
//! context. Credentials attach to the draft through the existing intents,
//! verification runs in the core over the admitted reads and streams its
//! steps through the shell snapshot, completion commits the draft as a
//! tenant in one step, and a discard cleans its credentials up. Nothing of
//! the draft is visible as a tenant until completion.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use tokio::time::Instant;

use super::gateway_domain::SetupGateway;
use super::{CredentialClass, MAX_TENANTS, Refusal, ScanCapability, State, Tenant};
use crate::context::ContextRef;
use crate::features::{
    OnboardingState, PackView, ProductId, SetupMode, SetupView, VerifyLine, VerifyState,
    VerifyStep, VerifyView, VerifyWarning,
};
use crate::gateway::{GatewayRead, GatewayResource, GatewayResult, PlaneAccess, ScopeBinding};
use crate::library::CaseProduct;
use crate::management::{ManagementFailure, ManagementRequest, ManagementResult};
use crate::runtime::OperationId;
use crate::tenants::TenantId;
use crate::tenants::{ProfileDraft, TenantProfile};
use crate::vault::{RecordId, SessionVault, StorageMode};

/// The draft under setup.
#[derive(Clone, Debug)]
pub(super) struct Setup {
    pub mode: SetupMode,
    pub tenant: Tenant,
    pub context: ContextRef,
    pub products: Vec<ProductId>,
    pub verify: Option<VerifyRun>,
    /// The draft's own Gateway listing and choice.
    pub gateway: SetupGateway,
}

/// One verification run: its steps and the inputs it was run over.
#[derive(Clone, Debug)]
pub(super) struct VerifyRun {
    generation: u32,
    signature: u64,
    steps: Vec<StepState>,
}

#[derive(Clone, Debug)]
struct StepState {
    step: VerifyStep,
    operation: Option<OperationId>,
}

/// What a completed setup left for the handler to act on.
pub(super) struct Completed {
    pub tenant: TenantId,
    pub first_run: bool,
    pub packs: Vec<CaseProduct>,
}

const MAX_WARNINGS: usize = 8;

/// The bundled case-catalog counts per product (the packs step).
fn pack_counts() -> &'static [(CaseProduct, u32)] {
    static COUNTS: OnceLock<Vec<(CaseProduct, u32)>> = OnceLock::new();
    COUNTS.get_or_init(|| {
        let cases = crate::library::parse_catalog(crate::library::BUNDLED_CATALOG, "catalog")
            .unwrap_or_default();
        let mut counts: Vec<(CaseProduct, u32)> = Vec::new();
        for case in cases {
            match counts.iter_mut().find(|(p, _)| *p == case.product) {
                Some((_, n)) => *n += 1,
                None => counts.push((case.product, 1)),
            }
        }
        counts
    })
}

/// The case-catalog products a chosen product's starter pack holds.
pub(super) fn case_products(product: ProductId) -> &'static [CaseProduct] {
    match product {
        ProductId::RuntimeSecurity => &[CaseProduct::RuntimeSecurity],
        ProductId::Gateway => &[CaseProduct::AiGateway],
        ProductId::RedTeam => &[CaseProduct::AiRedTeaming],
        ProductId::ModelSkill => &[CaseProduct::ModelSecurity, CaseProduct::SkillScanning],
    }
}

fn product_label(product: ProductId) -> &'static str {
    match product {
        ProductId::RuntimeSecurity => "AI Runtime Security",
        ProductId::Gateway => "AI Gateway",
        ProductId::RedTeam => "AI Red Teaming",
        ProductId::ModelSkill => "Model and skill scans",
    }
}

fn needs_management(product: ProductId) -> bool {
    product != ProductId::RuntimeSecurity
}

/// The verify step's reason for a management failure.
fn failure_reason(failure: ManagementFailure) -> &'static str {
    match failure {
        ManagementFailure::Unauthorized => {
            "The management client was not accepted. Check the client ID and secret."
        }
        ManagementFailure::Denied => "The management client is not allowed to read this tenant.",
        ManagementFailure::NotFound => "The service found nothing for this TSG.",
        ManagementFailure::RateLimited => "The service is rate-limiting reads; try again shortly.",
        ManagementFailure::Unavailable => "The management service could not be reached.",
        ManagementFailure::Rejected => "The service rejected the request.",
        ManagementFailure::Timeout => "The service did not answer in time.",
        ManagementFailure::Cancelled => "The check was cancelled.",
        ManagementFailure::Network => "The network request failed.",
        ManagementFailure::Destination => "The management destination is not reachable from here.",
        ManagementFailure::InvalidResponse => {
            "The service returned a reply the desktop could not use."
        }
        ManagementFailure::CredentialLocked => {
            "The Keychain is locked; unlock it and verify again."
        }
        ManagementFailure::CredentialDenied => "The Keychain denied access to the client secret.",
        ManagementFailure::CredentialMissing => "The client secret is missing from the Keychain.",
        ManagementFailure::Configuration => "The management client is not fully configured.",
    }
}

fn chosen(products: Vec<ProductId>) -> Vec<ProductId> {
    let mut chosen = vec![ProductId::RuntimeSecurity];
    for p in products {
        if !chosen.contains(&p) {
            chosen.push(p);
        }
    }
    chosen
}

impl State {
    /// Whether `context` is the draft's.
    pub(super) fn is_setup_context(&self, context: &ContextRef) -> bool {
        self.data
            .setup
            .as_ref()
            .is_some_and(|s| &s.context == context)
    }

    /// The draft's tenant id for its context.
    pub(super) fn setup_tenant_for(&self, context: &ContextRef) -> Option<TenantId> {
        self.data
            .setup
            .as_ref()
            .filter(|s| &s.context == context)
            .map(|s| s.tenant.profile.id.clone())
    }

    fn setup_for(&self, context: &ContextRef) -> Result<&Setup, Refusal> {
        self.data
            .setup
            .as_ref()
            .filter(|s| &s.context == context)
            .ok_or(Refusal::StaleContext)
    }

    fn setup_for_mut(&mut self, context: &ContextRef) -> Result<&mut Setup, Refusal> {
        self.data
            .setup
            .as_mut()
            .filter(|s| &s.context == context)
            .ok_or(Refusal::StaleContext)
    }

    /// Starts a setup with a validated profile. One draft at a time.
    pub(super) fn begin_setup(
        &mut self,
        mode: SetupMode,
        profile: ProfileDraft,
    ) -> Result<(), Refusal> {
        let profile = profile.validate().ok_or(Refusal::InvalidInput)?;
        if self.data.setup.is_some() {
            return Err(Refusal::Busy);
        }
        if self.data.tenants.len() >= MAX_TENANTS {
            return Err(Refusal::Busy);
        }
        if mode == SetupMode::FirstRun && !self.data.tenants.is_empty() {
            return Err(Refusal::InvalidInput);
        }
        let id = TenantId::issue().ok_or(Refusal::ShuttingDown)?;
        self.epoch += 1;
        let context = ContextRef::issue(&self.salt, self.epoch);
        self.data.setup = Some(Setup {
            mode,
            tenant: Tenant::new(TenantProfile {
                id,
                label: profile.label,
                tsg_id: profile.tsg_id,
                destination: profile.destination,
                environment: profile.environment,
            }),
            context,
            products: vec![ProductId::RuntimeSecurity],
            verify: None,
            gateway: SetupGateway::default(),
        });
        Ok(())
    }

    /// Whether `tenant` is the draft under setup.
    pub(super) fn is_setup_tenant(&self, tenant: &TenantId) -> bool {
        self.data
            .setup
            .as_ref()
            .is_some_and(|s| &s.tenant.profile.id == tenant)
    }

    /// The draft's inputs changed (a credential, the profile): a fresh
    /// context replaces the old one so nothing bound to it survives — the
    /// cached management binding (SDK clients, token cache, TSG), reads in
    /// flight, the verify run and the Gateway listing and choice.
    pub(super) fn reissue_setup_context(&mut self) {
        if self.data.setup.is_none() {
            return;
        }
        self.epoch += 1;
        let fresh = ContextRef::issue(&self.salt, self.epoch);
        self.retire_setup_authority();
        if let Some(setup) = self.data.setup.as_mut() {
            setup.context = fresh;
            setup.verify = None;
            setup.gateway = SetupGateway::default();
        }
    }

    /// A tenant was committed outside the setup (the legacy Create tenant
    /// or a v1 import during first run): a first-run draft is over, since
    /// the desktop now has a tenant and the shell takes over. Its Keychain
    /// records go to the cleanup queue; returned session records are the
    /// caller's to delete.
    pub(super) fn end_first_run_setup(&mut self) -> Vec<RecordId> {
        if self
            .data
            .setup
            .as_ref()
            .is_none_or(|s| s.mode != SetupMode::FirstRun)
        {
            return Vec::new();
        }
        let context = self.data.setup.as_ref().map(|s| s.context.clone());
        match context {
            Some(context) => self.discard_setup(&context).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Changes the draft's profile. A new destination retires credentials
    /// bound to the old one (returned session records are the caller's to
    /// delete); verification results go stale through the signature.
    pub(super) fn update_setup(
        &mut self,
        context: &ContextRef,
        profile: ProfileDraft,
    ) -> Result<Vec<RecordId>, Refusal> {
        let profile = profile.validate().ok_or(Refusal::InvalidInput)?;
        let setup = self.setup_for(context)?;
        let id = setup.tenant.profile.id.clone();
        let destination_changed = setup.tenant.profile.destination != profile.destination;
        let tsg_changed = setup.tenant.profile.tsg_id != profile.tsg_id;
        let mut retired = Vec::new();
        if destination_changed {
            for class in [
                CredentialClass::Scan,
                CredentialClass::Management,
                CredentialClass::Inference,
            ] {
                retired.extend(self.retire_class(&id, class));
            }
        } else if tsg_changed {
            retired.extend(self.retire_class(&id, CredentialClass::Management));
        }
        let setup = self.setup_for_mut(context)?;
        let changed = setup.tenant.profile.label != profile.label
            || setup.tenant.profile.tsg_id != profile.tsg_id
            || setup.tenant.profile.destination != profile.destination
            || setup.tenant.profile.environment != profile.environment;
        setup.tenant.profile.label = profile.label;
        setup.tenant.profile.tsg_id = profile.tsg_id;
        setup.tenant.profile.destination = profile.destination;
        setup.tenant.profile.environment = profile.environment;
        if changed {
            self.reissue_setup_context();
        }
        Ok(retired)
    }

    /// Records the chosen products (Runtime Security always, in first place).
    pub(super) fn choose_setup_products(
        &mut self,
        context: &ContextRef,
        products: Vec<ProductId>,
    ) -> Result<(), Refusal> {
        let setup = self.setup_for_mut(context)?;
        setup.products = chosen(products);
        Ok(())
    }

    /// A digest of every input verification depends on: profile, products
    /// and the credential records. Results for another digest are stale.
    fn setup_signature(setup: &Setup) -> u64 {
        let mut h = DefaultHasher::new();
        let p = &setup.tenant.profile;
        p.label.hash(&mut h);
        p.tsg_id.hash(&mut h);
        p.destination.origin().hash(&mut h);
        (p.environment as u8).hash(&mut h);
        for product in &setup.products {
            (*product as u8).hash(&mut h);
        }
        for class in [
            CredentialClass::Scan,
            CredentialClass::Management,
            CredentialClass::Inference,
        ] {
            match setup.tenant.capability(class) {
                ScanCapability::Ready { record, .. } => record.as_str().hash(&mut h),
                ScanCapability::Storing { record, .. } => {
                    "storing".hash(&mut h);
                    record.as_str().hash(&mut h);
                }
                ScanCapability::Missing | ScanCapability::Failed { .. } => "none".hash(&mut h),
            }
        }
        setup.tenant.management_client.hash(&mut h);
        setup
            .gateway
            .selection
            .as_ref()
            .map(|s| s.workspace.id.as_str())
            .hash(&mut h);
        h.finish()
    }

    /// Starts a verification run over the current inputs: every line is
    /// decided in the core; the lines that need a service read are admitted
    /// through the management pipeline and settle through it.
    pub(super) fn begin_verify(
        &mut self,
        context: &ContextRef,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<Vec<crate::runtime::Start>, Refusal> {
        let setup = self.setup_for(context)?;
        let generation = setup.verify.as_ref().map_or(1, |v| v.generation + 1);
        let signature = Self::setup_signature(setup);
        let mode = setup.mode;
        let products = setup.products.clone();
        let scan = setup.tenant.scan.clone();
        let management_ready = matches!(setup.tenant.management, ScanCapability::Ready { .. });
        let mut starts = Vec::new();
        let mut steps = Vec::new();
        let mut admit = |state: &mut Self, request: ManagementRequest| -> Option<OperationId> {
            match state.admit_management(
                context.clone(),
                request,
                Arc::clone(&keychain),
                Arc::clone(&session_vault),
            ) {
                Ok((operation, s)) => {
                    starts.extend(s);
                    Some(operation)
                }
                Err(_) => None,
            }
        };
        // Runtime Security scan key: stored keys are proven by the first
        // scan (there is no admitted read that exercises a scan key).
        let (state_, reason) = match scan {
            ScanCapability::Ready { .. } => (
                VerifyState::Stored,
                "Stored in the Keychain. Not checked here: the first scan proves it against the service.",
            ),
            ScanCapability::Storing { .. } => {
                (VerifyState::Fail, "The scan key is still being saved.")
            }
            _ if mode == SetupMode::FirstRun => (
                VerifyState::Fail,
                "No scan key saved. Runtime Security needs one on first run.",
            ),
            _ => (
                VerifyState::Warning,
                "No scan key saved; scans stay unavailable until one is added in Administration.",
            ),
        };
        steps.push(StepState {
            step: VerifyStep {
                line: VerifyLine::ScanKey,
                product: Some(ProductId::RuntimeSecurity),
                state: state_,
                reason: reason.to_owned(),
            },
            operation: None,
        });
        // Management client: a real verify read when one is stored.
        let needing: Vec<&str> = products
            .iter()
            .copied()
            .filter(|p| needs_management(*p))
            .map(product_label)
            .collect();
        let management = if management_ready {
            match admit(self, ManagementRequest::Verify) {
                Some(op) => (VerifyState::Pending, "Reading…".to_owned(), Some(op)),
                None => (
                    VerifyState::Fail,
                    "The verify read could not be started.".to_owned(),
                    None,
                ),
            }
        } else if needing.is_empty() {
            (
                VerifyState::Skipped,
                "No management client; Administration stays read-only.".to_owned(),
                None,
            )
        } else {
            (
                VerifyState::Warning,
                format!(
                    "No management client: {} stay read-only until one is saved in Administration.",
                    needing.join(", ")
                ),
                None,
            )
        };
        steps.push(StepState {
            step: VerifyStep {
                line: VerifyLine::ManagementClient,
                product: Some(ProductId::RuntimeSecurity),
                state: management.0,
                reason: management.1,
            },
            operation: management.2,
        });
        let chosen_workspace = self.setup_workspace_reference();
        for product in products.iter().copied().filter(|p| needs_management(*p)) {
            let (line, request) = match product {
                // With a workspace chosen its prerequisites are read (the
                // admitted scoped read); otherwise only the listing can be.
                ProductId::Gateway => (
                    VerifyLine::Gateway,
                    match chosen_workspace.clone() {
                        Some(reference) => ManagementRequest::Gateway {
                            workspace: Some(reference),
                            read: GatewayRead::Prerequisites,
                        },
                        None => ManagementRequest::Gateway {
                            workspace: None,
                            read: GatewayRead::Inventory {
                                resource: GatewayResource::Workspaces,
                            },
                        },
                    },
                ),
                ProductId::RedTeam => (
                    VerifyLine::RedTeam,
                    ManagementRequest::RedTeam { window: 7 },
                ),
                ProductId::ModelSkill => (VerifyLine::ModelSkill, ManagementRequest::SupplyChain),
                ProductId::RuntimeSecurity => continue,
            };
            let (state_, reason, operation) = if !management_ready {
                (
                    VerifyState::Warning,
                    "Needs the management client; not checked.".to_owned(),
                    None,
                )
            } else {
                match admit(self, request) {
                    Some(op) => (VerifyState::Pending, "Reading…".to_owned(), Some(op)),
                    None => (
                        VerifyState::Fail,
                        "The read could not be started.".to_owned(),
                        None,
                    ),
                }
            };
            steps.push(StepState {
                step: VerifyStep {
                    line,
                    product: Some(product),
                    state: state_,
                    reason,
                },
                operation,
            });
        }
        // Products without an admitted service are listed, never checked.
        for line in [VerifyLine::Ais, VerifyLine::Aes] {
            steps.push(StepState {
                step: VerifyStep {
                    line,
                    product: None,
                    state: VerifyState::Unavailable,
                    reason: "Not available in this release: no admitted service (D058).".to_owned(),
                },
                operation: None,
            });
        }
        let setup = self.setup_for_mut(context)?;
        setup.verify = Some(VerifyRun {
            generation,
            signature,
            steps,
        });
        Ok(starts)
    }

    /// The operations the current verify run waits on.
    pub(super) fn setup_operations(&self) -> Vec<OperationId> {
        self.data
            .setup
            .as_ref()
            .and_then(|s| s.verify.as_ref())
            .map(|v| v.steps.iter().filter_map(|s| s.operation.clone()).collect())
            .unwrap_or_default()
    }

    /// Settles one verify line from its management result.
    pub(super) fn setup_settled(&mut self, operation: &OperationId, result: &ManagementResult) {
        let Some(run) = self.data.setup.as_mut().and_then(|s| s.verify.as_mut()) else {
            return;
        };
        let Some(step) = run
            .steps
            .iter_mut()
            .find(|s| s.operation.as_ref() == Some(operation))
        else {
            return;
        };
        step.operation = None;
        let (state, reason): (VerifyState, String) = match (&step.step.line, result) {
            (_, ManagementResult::Failed { failure, .. }) => {
                (VerifyState::Fail, failure_reason(*failure).to_owned())
            }
            (VerifyLine::ManagementClient, ManagementResult::Verified) => (
                VerifyState::Pass,
                "The management client can read this tenant.".to_owned(),
            ),
            (
                VerifyLine::Gateway,
                ManagementResult::Gateway {
                    gateway: GatewayResult::List { list },
                },
            ) => {
                // No workspace chosen: the listing is readable, but the
                // workspace's prerequisites were not checked.
                if list.count > 0 {
                    (
                        VerifyState::Warning,
                        format!(
                            "{} Gateway workspace(s) readable; none chosen, so its prerequisites were not checked.",
                            list.count
                        ),
                    )
                } else {
                    (
                        VerifyState::Warning,
                        "No Gateway workspace in this tenant yet.".to_owned(),
                    )
                }
            }
            (
                VerifyLine::Gateway,
                ManagementResult::Gateway {
                    gateway: GatewayResult::Prerequisites { prerequisites },
                },
            ) => {
                let sources = [
                    ("routes", prerequisites.routes_failure),
                    ("providers", prerequisites.providers_failure),
                    ("integrations", prerequisites.integrations_failure),
                    ("service keys", prerequisites.service_keys_failure),
                ];
                let mut failed: Vec<String> = sources
                    .iter()
                    .filter_map(|(name, f)| f.map(|f| format!("{name}: {}", failure_reason(f))))
                    .collect();
                // Every source attempted counts: the IAM scope binding and the
                // three planes are part of the prerequisites, not decoration.
                let plane = |name: &str, access: PlaneAccess| match access {
                    PlaneAccess::Granted => None,
                    PlaneAccess::Denied => Some(format!("{name} plane: permission refused")),
                    PlaneAccess::Failed => Some(format!("{name} plane: reads failed")),
                };
                failed.extend(plane("data", prerequisites.data_plane));
                failed.extend(plane("admin", prerequisites.admin_plane));
                failed.extend(plane("IAM", prerequisites.iam_plane));
                failed.extend(match prerequisites.scope {
                    ScopeBinding::Bound => None,
                    ScopeBinding::Unbound => Some(format!(
                        "scope {} does not list the workspace",
                        prerequisites.scope_name
                    )),
                    ScopeBinding::Absent => {
                        Some(format!("scope {} does not exist", prerequisites.scope_name))
                    }
                    ScopeBinding::Denied => Some(format!(
                        "scope {} may not be read by this client",
                        prerequisites.scope_name
                    )),
                    ScopeBinding::Failed => Some(format!(
                        "scope {} could not be read",
                        prerequisites.scope_name
                    )),
                });
                let lists_failed = sources.iter().filter(|(_, f)| f.is_some()).count();
                if failed.is_empty() {
                    (
                        VerifyState::Pass,
                        format!(
                            "Workspace {}: {} route(s), {} provider(s), {} inference key(s) readable.",
                            prerequisites.workspace,
                            prerequisites.routes.as_ref().map_or(0, |l| l.count),
                            prerequisites.providers.as_ref().map_or(0, |l| l.count),
                            prerequisites.inference_keys
                        ),
                    )
                } else if lists_failed < sources.len() {
                    (
                        VerifyState::Warning,
                        format!(
                            "Workspace {}: partial; {}",
                            prerequisites.workspace,
                            failed.join("; ")
                        ),
                    )
                } else {
                    (
                        VerifyState::Fail,
                        format!(
                            "Workspace {}: {}",
                            prerequisites.workspace,
                            failed.join("; ")
                        ),
                    )
                }
            }
            (VerifyLine::RedTeam, ManagementResult::RedTeam { observation }) => {
                match (&observation.scans, &observation.statistics) {
                    (Some(scans), Some(_)) => (
                        VerifyState::Pass,
                        format!(
                            "Red Team scans and statistics readable ({} scans listed).",
                            scans.rows.len()
                        ),
                    ),
                    (Some(scans), None) => (
                        VerifyState::Warning,
                        format!(
                            "Red Team scans readable ({} listed); statistics did not: {}",
                            scans.rows.len(),
                            observation
                                .statistics_failure
                                .map_or("no reason", failure_reason)
                        ),
                    ),
                    (None, Some(_)) => (
                        VerifyState::Warning,
                        format!(
                            "Statistics read; the scan list did not: {}",
                            observation
                                .scans_failure
                                .map_or("no reason", failure_reason)
                        ),
                    ),
                    (None, None) => (
                        VerifyState::Fail,
                        observation
                            .scans_failure
                            .or(observation.statistics_failure)
                            .map_or("The Red Team reads returned nothing.", failure_reason)
                            .to_owned(),
                    ),
                }
            }
            (VerifyLine::ModelSkill, ManagementResult::SupplyChain { observation }) => {
                let ok = [
                    observation.models.is_some(),
                    observation.skills.is_some(),
                    observation.skill_statistics.is_some(),
                ];
                let readable = ok.iter().filter(|b| **b).count();
                if readable == 3 {
                    (
                        VerifyState::Pass,
                        format!(
                            "Model and skill scans readable ({} models, {} skills).",
                            observation.models.as_ref().map_or(0, |m| m.rows.len()),
                            observation.skills.as_ref().map_or(0, |s| s.rows.len())
                        ),
                    )
                } else if readable > 0 {
                    (
                        VerifyState::Warning,
                        format!("{readable} of 3 model and skill sources readable."),
                    )
                } else {
                    (
                        VerifyState::Fail,
                        observation
                            .models_failure
                            .or(observation.skills_failure)
                            .map_or(
                                "The model and skill reads returned nothing.",
                                failure_reason,
                            )
                            .to_owned(),
                    )
                }
            }
            _ => (
                VerifyState::Fail,
                "The service returned a reply the desktop could not use.".to_owned(),
            ),
        };
        step.step.state = state;
        step.step.reason = reason;
    }

    fn verify_view(setup: &Setup) -> Option<VerifyView> {
        let run = setup.verify.as_ref()?;
        let steps: Vec<VerifyStep> = run.steps.iter().map(|s| s.step.clone()).collect();
        let done = steps.iter().all(|s| s.state != VerifyState::Pending);
        let failed = steps.iter().any(|s| s.state == VerifyState::Fail);
        // Only a required line blocks: the scan key on first run. An optional
        // service that failed is carried as a warning the operator can
        // continue over (and the Inbox shows).
        let blocking = steps
            .iter()
            .any(|s| s.state == VerifyState::Fail && s.line == VerifyLine::ScanKey);
        let warnings = steps
            .iter()
            .filter_map(|s| {
                let product = s.product?;
                match s.state {
                    VerifyState::Warning => Some(VerifyWarning {
                        product,
                        reason: s.reason.clone(),
                    }),
                    VerifyState::Fail if s.line != VerifyLine::ScanKey => Some(VerifyWarning {
                        product,
                        reason: format!("Failed: {}", s.reason),
                    }),
                    _ => None,
                }
            })
            .collect();
        Some(VerifyView {
            generation: run.generation,
            steps,
            done,
            failed,
            blocking,
            stale: run.signature != Self::setup_signature(setup),
            warnings,
        })
    }

    /// The draft as the shell shows it.
    pub(super) fn setup_view(&self) -> Option<SetupView> {
        let setup = self.data.setup.as_ref()?;
        let verify = Self::verify_view(setup);
        let completable = verify
            .as_ref()
            .is_some_and(|v| v.done && !v.blocking && !v.stale);
        let packs = setup
            .products
            .iter()
            .map(|p| PackView {
                product: *p,
                count: case_products(*p)
                    .iter()
                    .map(|cp| {
                        pack_counts()
                            .iter()
                            .find(|(c, _)| c == cp)
                            .map_or(0, |(_, n)| *n)
                    })
                    .sum(),
            })
            .collect();
        Some(SetupView {
            mode: setup.mode,
            context: setup.context.clone(),
            tenant: super::tenant_view(&setup.tenant),
            products: setup.products.clone(),
            scan_required: setup.mode == SetupMode::FirstRun,
            packs,
            workspace: self.setup_workspace(&setup.context),
            verify,
            completable,
        })
    }

    /// Commits the draft: it becomes a tenant with its products and the
    /// verify warnings, is made active, and a first run's onboarding is
    /// done. Refused unless the current inputs were verified without a
    /// failure (the presentation cannot skip or forge a check).
    /// Whether the draft under `context` may complete with `packs` now (the
    /// checks of [`State::complete_setup`], without changing anything):
    /// the draft tenant's id when it may.
    pub(super) fn completable_setup(
        &self,
        context: &ContextRef,
        packs: &[ProductId],
    ) -> Result<TenantId, Refusal> {
        let setup = self.setup_for(context)?;
        let view = Self::verify_view(setup).ok_or(Refusal::InvalidInput)?;
        if !view.done || view.blocking || view.stale {
            return Err(Refusal::InvalidInput);
        }
        if setup.mode == SetupMode::FirstRun
            && !matches!(setup.tenant.scan, ScanCapability::Ready { .. })
        {
            return Err(Refusal::InvalidInput);
        }
        if packs.iter().any(|p| !setup.products.contains(p)) {
            return Err(Refusal::InvalidInput);
        }
        if self.data.tenants.len() >= MAX_TENANTS {
            return Err(Refusal::Busy);
        }
        Ok(setup.tenant.profile.id.clone())
    }

    pub(super) fn complete_setup(
        &mut self,
        context: &ContextRef,
        packs: Vec<ProductId>,
        recorded_secs: u32,
    ) -> Result<Completed, Refusal> {
        self.completable_setup(context, &packs)?;
        let setup = self.setup_for(context)?;
        let view = Self::verify_view(setup).ok_or(Refusal::InvalidInput)?;
        // The draft's authority ends with it: the tenant gets a fresh active
        // context and its own binding.
        self.retire_setup_authority();
        let Some(mut setup) = self.data.setup.take() else {
            return Err(Refusal::StaleContext);
        };
        let first_run = setup.mode == SetupMode::FirstRun;
        setup.tenant.products = setup.products.clone();
        for (n, w) in view.warnings.into_iter().take(MAX_WARNINGS).enumerate() {
            setup
                .tenant
                .setup_warnings
                .push(crate::features::SetupWarning {
                    id: format!("vw-{}", n + 1),
                    product: w.product,
                    reason: w.reason,
                    recorded_secs,
                });
        }
        let id = setup.tenant.profile.id.clone();
        self.data.tenants.push(setup.tenant);
        self.data.active = Some(id.clone());
        if first_run {
            self.data.onboarding = Some(OnboardingState::Done);
        }
        let packs = packs
            .iter()
            .flat_map(|p| case_products(*p).iter().copied())
            .collect();
        Ok(Completed {
            tenant: id,
            first_run,
            packs,
        })
    }

    /// Drops the draft. Its Keychain records go to the cleanup queue; the
    /// returned session records are the caller's to delete. No tenant,
    /// selection or onboarding state changes.
    pub(super) fn discard_setup(&mut self, context: &ContextRef) -> Result<Vec<RecordId>, Refusal> {
        let id = self.setup_for(context)?.tenant.profile.id.clone();
        let mut retired = Vec::new();
        for class in [
            CredentialClass::Scan,
            CredentialClass::Management,
            CredentialClass::Inference,
        ] {
            retired.extend(self.retire_class(&id, class));
        }
        self.retire_setup_authority();
        self.data.setup = None;
        Ok(retired)
    }

    /// Nothing bound to the draft's context outlives the draft: the cached
    /// management binding (SDK clients, token cache), the reads queued or in
    /// flight under its context and under its chosen workspace reference.
    /// Late replies are withdrawn by the pipeline (the context is gone).
    fn retire_setup_authority(&mut self) {
        let Some((context, reference)) = self.data.setup.as_ref().map(|s| {
            (
                s.context.clone(),
                s.gateway.selection.as_ref().map(|w| w.reference.clone()),
            )
        }) else {
            return;
        };
        self.retire_management_binding(&context);
        let now = Instant::now();
        let mut starts = self.operations.retire_context(context.as_str(), now);
        if let Some(reference) = reference {
            starts.extend(self.operations.retire_context(reference.as_str(), now));
        }
        self.queue_gateway_starts(starts);
    }

    /// Whether a draft credential is stored in the Keychain (its removal
    /// needs storage).
    pub(super) fn setup_has_keychain_credential(&self) -> bool {
        self.data.setup.as_ref().is_some_and(|s| {
            [
                CredentialClass::Scan,
                CredentialClass::Management,
                CredentialClass::Inference,
            ]
            .into_iter()
            .any(|c| {
                matches!(
                    s.tenant.capability(c),
                    ScanCapability::Ready {
                        mode: StorageMode::Keychain,
                        ..
                    }
                )
            })
        })
    }
}
