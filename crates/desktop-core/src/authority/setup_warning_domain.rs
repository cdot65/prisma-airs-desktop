//! Dedicated, read-only reconciliation of recorded setup warnings. Ordinary
//! product refreshes do not enter this path. Original diagnostics are immutable.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::{CredentialClass, Refusal, ScanCapability, State, Tenant, TenantView};
use crate::context::ContextRef;
use crate::features::{
    ProductId, SetupWarning, SetupWarningBasis, SetupWarningBinding, SetupWarningRecheck,
    SetupWarningWorkspace, VerifyLine, VerifyState,
};
use crate::gateway::{GatewayRead, WorkspaceScope};
use crate::management::{ManagementRequest, ManagementResult};
use crate::runtime::{OperationId, Start};
use crate::tenants::TenantId;
use crate::vault::SessionVault;

#[derive(Clone, Debug)]
pub(super) struct Recheck {
    operation: OperationId,
    context: ContextRef,
    tenant: TenantId,
    warning: String,
    binding: SetupWarningBinding,
    signature: u64,
    started_secs: u32,
}

pub(super) fn line_for(product: ProductId) -> VerifyLine {
    match product {
        ProductId::RuntimeSecurity => VerifyLine::ScanKey,
        ProductId::Gateway => VerifyLine::Gateway,
        ProductId::RedTeam => VerifyLine::RedTeam,
        ProductId::ModelSkill => VerifyLine::ModelSkill,
    }
}

pub(super) fn binding_for(
    tenant: &Tenant,
    line: VerifyLine,
    workspace: Option<WorkspaceScope>,
) -> SetupWarningBinding {
    SetupWarningBinding {
        version: 1,
        line,
        tsg_id: tenant.profile.tsg_id.clone().unwrap_or_default(),
        origin: tenant.profile.destination.origin().to_owned(),
        workspace: workspace.filter(|_| line == VerifyLine::Gateway).map(|w| {
            SetupWarningWorkspace {
                id: w.id,
                slug: w.slug,
                scope_name: w.scope_name,
            }
        }),
    }
}

/// Reconstruct only known historical formatters. Never infer scope from an
/// arbitrary substring or treat an unrelated healthy overview as evidence.
fn missing_workspace(reason: &str) -> bool {
    reason == "No Gateway workspace in this tenant yet." || reason
        .strip_suffix(" Gateway workspace(s) readable; none chosen, so its prerequisites were not checked.")
        .and_then(|count| count.parse::<u32>().ok()).is_some_and(|count| count > 0)
}

fn basis(warning: &SetupWarning, current: &SetupWarningBinding) -> SetupWarningBasis {
    let reason = warning
        .reason
        .strip_prefix("Failed: ")
        .unwrap_or(&warning.reason);
    if let Some(original) = &warning.check {
        let same = original.version == 1
            && original.line == current.line
            && original.tsg_id == current.tsg_id
            && original.origin == current.origin;
        // A recorded scope is exact. The one explicit predicate exception is
        // the known no-selection check: selecting a workspace and passing its
        // full prerequisites supersedes that specific missing-selection fact.
        let selected_now = current
            .workspace
            .as_ref()
            .is_some_and(|w| !w.slug.is_empty() && !w.scope_name.is_empty());
        let was_missing_selection = original.line == VerifyLine::Gateway
            && original.workspace.is_none()
            && selected_now
            && missing_workspace(reason);
        let workspace = original.workspace == current.workspace || was_missing_selection;
        return if same && workspace {
            SetupWarningBasis::RecordedScope
        } else {
            SetupWarningBasis::Unknown
        };
    }
    match warning.product {
        ProductId::RuntimeSecurity
            if matches!(
                reason,
                "No scan key saved; scans stay unavailable until one is added in Administration."
                    | "No scan key saved. Runtime Security needs one on first run."
                    | "No scan key configured; scanning stays unavailable until one is added in Settings."
            ) =>
        {
            SetupWarningBasis::LegacyTenantSources
        }
        ProductId::ModelSkill
            if matches!(
                reason,
                "1 of 3 model and skill sources readable."
                    | "2 of 3 model and skill sources readable."
            ) =>
        {
            SetupWarningBasis::LegacyTenantSources
        }
        ProductId::Gateway => {
            let Some(workspace) = current.workspace.as_ref() else {
                return SetupWarningBasis::Unknown;
            };
            if workspace.slug.is_empty() || workspace.scope_name.is_empty() {
                return SetupWarningBasis::Unknown;
            }
            if missing_workspace(reason) {
                return SetupWarningBasis::LegacyTenantSources;
            }
            let prefix = format!("Workspace {}: ", workspace.id);
            let Some(detail) = reason.strip_prefix(&prefix) else {
                return SetupWarningBasis::Unknown;
            };
            let detail = detail.strip_prefix("partial; ").unwrap_or(detail);
            let expected = [
                "does not list the workspace",
                "does not exist",
                "may not be read by this client",
                "could not be read",
            ]
            .map(|suffix| format!("scope {} {suffix}", workspace.scope_name));
            if expected.iter().any(|expected| detail == expected) {
                SetupWarningBasis::LegacyWorkspaceScope
            } else {
                SetupWarningBasis::Unknown
            }
        }
        _ => SetupWarningBasis::Unknown,
    }
}

impl State {
    fn warning_signature(
        &self,
        context: &ContextRef,
        tenant: &Tenant,
        binding: &SetupWarningBinding,
    ) -> u64 {
        let mut hash = DefaultHasher::new();
        context.as_str().hash(&mut hash);
        tenant.profile.id.as_str().hash(&mut hash);
        binding.tsg_id.hash(&mut hash);
        binding.origin.hash(&mut hash);
        for class in [CredentialClass::Scan, CredentialClass::Management] {
            match tenant.capability(class) {
                ScanCapability::Ready { record, .. } => record.as_str().hash(&mut hash),
                _ => "not-ready".hash(&mut hash),
            }
        }
        tenant.management_client.hash(&mut hash);
        if let Some(workspace) = &binding.workspace {
            workspace.id.hash(&mut hash);
            workspace.slug.hash(&mut hash);
            workspace.scope_name.hash(&mut hash);
        }
        if binding.line == VerifyLine::Gateway {
            self.gateway_selection()
                .as_ref()
                .map(|w| w.as_str())
                .hash(&mut hash);
        }
        hash.finish()
    }

    pub(super) fn begin_warning_recheck(
        &mut self,
        context: &ContextRef,
        warning: &str,
        keychain: Arc<dyn super::CredentialStore>,
        session: Arc<SessionVault>,
    ) -> Result<(Option<OperationId>, Vec<Start>), Refusal> {
        self.require_storage()?;
        self.prune_warning_rechecks();
        let id = self.check_context(context)?;
        if self.is_setup_context(context) {
            return Err(Refusal::InvalidInput);
        }
        if self
            .warning_rechecks
            .iter()
            .any(|r| &r.context == context && r.warning == warning)
        {
            return Err(Refusal::Busy);
        }
        let tenant = self.tenant(&id).ok_or(Refusal::StaleContext)?;
        let saved = tenant
            .setup_warnings
            .iter()
            .find(|w| w.id == warning && w.resolved_secs.is_none())
            .ok_or(Refusal::InvalidInput)?;
        let line = saved
            .check
            .as_ref()
            .map_or(line_for(saved.product), |b| b.line);
        if line != line_for(saved.product)
            && !(saved.product == ProductId::RuntimeSecurity
                && line == VerifyLine::ManagementClient)
        {
            return Err(Refusal::InvalidInput);
        }
        let binding = binding_for(tenant, line, self.chosen_scope(context));
        let signature = self.warning_signature(context, tenant, &binding);
        let at = super::wall_secs(self.clock_offset);
        let request = match line {
            VerifyLine::ScanKey => {
                let result = if matches!(tenant.scan, ScanCapability::Ready { .. }) {
                    (VerifyState::Stored, "Scan key configured. Its service validity is checked only when you submit a scan.".to_owned())
                } else {
                    (
                        VerifyState::Warning,
                        "No ready scan key configured; scanning stays unavailable.".to_owned(),
                    )
                };
                self.finish_warning_recheck(&id, warning, &binding, result, at);
                return Ok((None, Vec::new()));
            }
            VerifyLine::ManagementClient => ManagementRequest::Verify,
            VerifyLine::Gateway => {
                let Some(workspace) = self
                    .gateway_selection()
                    .filter(|_| self.chosen_scope(context).is_some())
                else {
                    self.finish_warning_recheck(&id, warning, &binding, (VerifyState::Warning, "Choose the relevant Gateway workspace in Administration before rechecking its setup.".to_owned()), at);
                    return Ok((None, Vec::new()));
                };
                ManagementRequest::Gateway {
                    workspace: Some(workspace),
                    read: GatewayRead::Prerequisites,
                }
            }
            VerifyLine::RedTeam => ManagementRequest::RedTeam { window: 7 },
            VerifyLine::ModelSkill => ManagementRequest::SupplyChain,
            _ => return Err(Refusal::InvalidInput),
        };
        let (operation, starts) =
            self.admit_management(context.clone(), request, keychain, session)?;
        self.warning_rechecks.push(Recheck {
            operation: operation.clone(),
            context: context.clone(),
            tenant: id,
            warning: warning.to_owned(),
            binding,
            signature,
            started_secs: at,
        });
        Ok((Some(operation), starts))
    }

    fn warning_recheck_current(&self, run: &Recheck) -> bool {
        if self.active_ref.as_ref() != Some(&run.context)
            || self.data.active.as_ref() != Some(&run.tenant)
        {
            return false;
        }
        let Some(tenant) = self.tenant(&run.tenant) else {
            return false;
        };
        let current = binding_for(tenant, run.binding.line, self.chosen_scope(&run.context));
        current == run.binding
            && self.warning_signature(&run.context, tenant, &current) == run.signature
    }

    /// Lease invalidation cannot depend on a service ever replying. Keep
    /// only checks still bound to the current context/credential/workspace.
    pub(super) fn prune_warning_rechecks(&mut self) {
        let retired = self
            .warning_rechecks
            .iter()
            .filter(|r| !self.warning_recheck_current(r))
            .map(|r| r.operation.clone())
            .collect::<Vec<_>>();
        self.warning_rechecks
            .retain(|r| !retired.contains(&r.operation));
        self.retire_warning_check_records(&retired);
    }

    pub(super) fn warning_recheck_operations(&self) -> Vec<OperationId> {
        self.warning_rechecks
            .iter()
            .map(|r| r.operation.clone())
            .collect()
    }

    pub(super) fn warning_recheck_settled(
        &mut self,
        operation: &OperationId,
        result: &ManagementResult,
    ) {
        let Some(index) = self
            .warning_rechecks
            .iter()
            .position(|r| &r.operation == operation)
        else {
            return;
        };
        let run = self.warning_rechecks.remove(index);
        if self.active_ref.as_ref() != Some(&run.context)
            || self.data.active.as_ref() != Some(&run.tenant)
        {
            return;
        }
        let Some(tenant) = self.tenant(&run.tenant) else {
            return;
        };
        let binding = binding_for(tenant, run.binding.line, self.chosen_scope(&run.context));
        if binding != run.binding
            || self.warning_signature(&run.context, tenant, &binding) != run.signature
        {
            return;
        }
        let result = super::setup_domain::verify_result(binding.line, result);
        self.finish_warning_recheck(
            &run.tenant,
            &run.warning,
            &binding,
            result,
            super::wall_secs(self.clock_offset),
        );
    }

    fn finish_warning_recheck(
        &mut self,
        tenant: &TenantId,
        id: &str,
        binding: &SetupWarningBinding,
        (state, mut reason): (VerifyState, String),
        at: u32,
    ) {
        let before = self.data.clone();
        let Some(warning) = self
            .data
            .tenants
            .iter_mut()
            .find(|t| &t.profile.id == tenant)
            .and_then(|t| {
                t.setup_warnings
                    .iter_mut()
                    .find(|w| w.id == id && w.resolved_secs.is_none())
            })
        else {
            return;
        };
        let basis = basis(warning, binding);
        // A stored scan-key configuration is a tenant-local fact, including
        // old scan-only profiles without a TSG. Remote checks still require it.
        let equivalent = basis != SetupWarningBasis::Unknown
            && (binding.line == VerifyLine::ScanKey || !binding.tsg_id.is_empty());
        let passed = state == VerifyState::Pass
            || (binding.line == VerifyLine::ScanKey && state == VerifyState::Stored);
        if passed && !equivalent {
            reason.push_str(" The current check passed, but the original check scope cannot be established; its warning remains active.");
        }
        if basis == SetupWarningBasis::LegacyTenantSources {
            reason.push_str(" Reconstructed from the known legacy setup check for this tenant; the original diagnostic is unchanged.");
        }
        if basis == SetupWarningBasis::LegacyWorkspaceScope {
            reason.push_str(" Reconstructed from the legacy workspace UUID and scope name. The current listed slug was checked against IAM; the original slug was not recorded.");
        }
        warning.recheck = Some(SetupWarningRecheck {
            checked_secs: at,
            state,
            reason,
            equivalent,
            basis,
        });
        if passed
            && equivalent
            && binding.line == VerifyLine::Gateway
            && warning
                .check
                .as_ref()
                .is_some_and(|b| b.workspace.is_none())
            && missing_workspace(&warning.reason)
        {
            if let Some(check) = &mut warning.recheck {
                check.reason.push_str(" The original check found no selected workspace; the current selection's full setup prerequisites were checked.");
            }
        }
        if passed && equivalent {
            warning.resolved_secs = Some(at);
        }
        if self.save_with(&before).is_err() {
            if let Some(warning) = self
                .data
                .tenants
                .iter_mut()
                .find(|t| &t.profile.id == tenant)
                .and_then(|t| t.setup_warnings.iter_mut().find(|w| w.id == id))
            {
                warning.recheck = Some(SetupWarningRecheck { checked_secs: at, state: VerifyState::Fail,
                    reason: "The recheck result could not be saved; the original warning remains active. Try again when local storage is available.".to_owned(), equivalent: false, basis: SetupWarningBasis::Unknown });
            }
        }
    }

    pub(super) fn warning_recheck_view(&self, mut view: TenantView) -> TenantView {
        for warning in &mut view.setup_warnings {
            if let Some(run) = self.warning_rechecks.iter().find(|r| {
                r.tenant == view.id
                    && r.warning == warning.id
                    && self.active_ref.as_ref() == Some(&r.context)
            }) {
                warning.recheck = Some(SetupWarningRecheck {
                    checked_secs: run.started_secs,
                    state: VerifyState::Pending,
                    reason: "Rechecking the setup finding…".to_owned(),
                    equivalent: false,
                    basis: SetupWarningBasis::Unknown,
                });
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn context_retirement_prunes_checks_without_waiting_for_a_management_reply() {
        let (adapters, _) = crate::fixtures::adapters();
        let core = crate::Core::new(crate::CoreConfig::default(), adapters);
        let mut state = super::super::lock(&core.inner).unwrap();
        let tenant = crate::tenants::TenantId::issue().unwrap();
        state
            .data
            .tenants
            .push(Tenant::new(crate::tenants::TenantProfile {
                id: tenant.clone(),
                label: "Fixture".to_owned(),
                tsg_id: Some("1234567890".to_owned()),
                destination: crate::tenants::Destination::Custom {
                    origin: "https://localhost:9".to_owned(),
                },
                environment: crate::features::TenantEnvironment::Production,
            }));
        state.data.active = Some(tenant.clone());
        state.reissue_context();
        let context = state.active_ref.clone().unwrap();
        let binding = binding_for(state.tenant(&tenant).unwrap(), VerifyLine::ModelSkill, None);
        let signature = state.warning_signature(&context, state.tenant(&tenant).unwrap(), &binding);
        state.warning_rechecks.push(Recheck {
            operation: serde_json::from_str("\"op-1\"").unwrap(),
            context,
            tenant,
            warning: "vw-1".to_owned(),
            binding,
            signature,
            started_secs: 1,
        });
        state.changed();
        assert_eq!(
            state.warning_rechecks.len(),
            1,
            "a current unanswered check stays pending"
        );
        state.reissue_context();
        assert!(
            state.warning_rechecks.is_empty(),
            "no completion can be required to release this bookkeeping"
        );
    }
}
