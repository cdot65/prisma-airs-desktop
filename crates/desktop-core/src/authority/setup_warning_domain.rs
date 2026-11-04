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
fn basis(warning: &SetupWarning, current: &SetupWarningBinding) -> SetupWarningBasis {
    if let Some(original) = &warning.check {
        let same = original.version == 1
            && original.line == current.line
            && original.tsg_id == current.tsg_id
            && original.origin == current.origin;
        let workspace = original
            .workspace
            .as_ref()
            .is_none_or(|w| current.workspace.as_ref() == Some(w));
        return if same && workspace {
            SetupWarningBasis::RecordedScope
        } else {
            SetupWarningBasis::Unknown
        };
    }
    let reason = warning
        .reason
        .strip_prefix("Failed: ")
        .unwrap_or(&warning.reason);
    match warning.product {
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
        let equivalent = basis != SetupWarningBasis::Unknown && !binding.tsg_id.is_empty();
        let passed = state == VerifyState::Pass
            || (binding.line == VerifyLine::ScanKey && state == VerifyState::Stored);
        if passed && !equivalent {
            reason.push_str(" The current check passed, but the original check scope cannot be established; its warning remains active.");
        }
        if basis == SetupWarningBasis::LegacyTenantSources {
            reason.push_str(" Reconstructed from the known legacy three-source setup check for this tenant; the original diagnostic is unchanged.");
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
