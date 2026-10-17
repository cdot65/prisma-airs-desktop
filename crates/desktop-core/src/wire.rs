//! Versioned wire contract between the React presentation and the native host.
//!
//! Requests are parsed from raw bytes with an explicit size ceiling and strict
//! schemas: unknown fields, unknown variants, wrong protocol versions and
//! non-canonical sequence numbers are rejected before any authority is
//! consulted. Opaque IDs, revisions and sequences are strings so JavaScript
//! never rounds a u64. TypeScript declarations are generated from these types
//! (`xtask bindings`); they document the shape but do not validate at runtime.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::authority::{Intent, Receipt, Refusal, ShellSnapshot};
use crate::delivery::{Delivery, DeliverySession};
use crate::features::{Availability, FeatureEntry, FeatureId};

/// The only wire protocol version this build accepts.
pub const PROTOCOL: u32 = 1;

/// Ceiling for one serialized request (DESIGN: 32MiB JSON envelope).
pub const MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;

/// Desktop-wide appearance preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// One Intent delivery from the renderer.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DispatchRequest {
    pub protocol: u32,
    pub session: String,
    /// Decimal u64 without sign or leading zeros.
    pub sequence: String,
    pub intent: Intent,
}

/// An observation or session request from the renderer.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObserveRequest {
    pub protocol: u32,
    pub scope: WireScope,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WireScope {
    /// Register a new delivery session, retiring the previous one.
    DeliverySession,
    /// Observe the shell; `updates` is the renderer's IPC channel token.
    Shell { updates: String },
    /// Stop the observation delivered on `updates`.
    CloseShell { updates: String },
}

/// A native menu selection forwarded to the presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MenuCommand {
    Navigate { feature: FeatureId },
    CommandPalette,
    SetAppearance { theme: Theme },
}

/// Result of `desktop_dispatch`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum DispatchResponse {
    Receipt { receipt: Receipt },
    Refusal { refusal: Refusal },
}

impl From<Result<Receipt, Refusal>> for DispatchResponse {
    fn from(result: Result<Receipt, Refusal>) -> Self {
        match result {
            Ok(receipt) => Self::Receipt { receipt },
            Err(refusal) => Self::Refusal { refusal },
        }
    }
}

/// Result of `desktop_observe`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ObserveResponse {
    #[serde(rename_all = "camelCase")]
    DeliverySession {
        session: String,
        next_sequence: String,
    },
    Shell {
        snapshot: Box<ShellSnapshot>,
    },
    /// The named observation was stopped (or was not active).
    Closed,
    Refusal {
        refusal: Refusal,
    },
}

impl From<DeliverySession> for ObserveResponse {
    fn from(session: DeliverySession) -> Self {
        Self::DeliverySession {
            session: session.nonce,
            next_sequence: session.next_sequence.to_string(),
        }
    }
}

/// Ordered update pushed on an observation channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ObservationUpdate {
    Shell {
        snapshot: Box<ShellSnapshot>,
    },
    Menu {
        command: MenuCommand,
    },
    /// Updates were dropped; observe again for a fresh snapshot.
    Resync,
}

/// Why a request was rejected before reaching authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("request exceeds {MAX_REQUEST_BYTES} bytes")]
    TooLarge,
    #[error("request does not match the wire schema")]
    Malformed,
    #[error("unsupported protocol version")]
    Protocol,
}

impl WireError {
    /// The sanitized refusal returned to the renderer.
    pub fn refusal(self) -> Refusal {
        Refusal::InvalidInput
    }
}

impl DispatchRequest {
    /// Parses one delivery from raw renderer bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, WireError> {
        let request: Self = parse_bounded(bytes)?;
        if request.protocol != PROTOCOL {
            return Err(WireError::Protocol);
        }
        parse_sequence(&request.sequence)?;
        if request.session.is_empty() || request.session.len() > 128 {
            return Err(WireError::Malformed);
        }
        Ok(request)
    }

    /// The delivery identity, validated by [`DispatchRequest::parse`].
    pub fn delivery(&self) -> Result<Delivery, WireError> {
        Ok(Delivery {
            session: self.session.clone(),
            sequence: parse_sequence(&self.sequence)?,
        })
    }
}

impl ObserveRequest {
    pub fn parse(bytes: &[u8]) -> Result<Self, WireError> {
        let request: Self = parse_bounded(bytes)?;
        if request.protocol != PROTOCOL {
            return Err(WireError::Protocol);
        }
        Ok(request)
    }
}

fn parse_bounded<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, WireError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(WireError::TooLarge);
    }
    serde_json::from_slice(bytes).map_err(|_| WireError::Malformed)
}

fn parse_sequence(text: &str) -> Result<u64, WireError> {
    let canonical = !text.is_empty()
        && text.len() <= 20
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if !canonical {
        return Err(WireError::Malformed);
    }
    text.parse().map_err(|_| WireError::Malformed)
}

/// Writes the TypeScript declarations for every wire type into `dir`.
pub fn export_typescript(dir: &Path) -> Result<(), ts_rs::ExportError> {
    let config = ts_rs::Config::new().with_out_dir(dir);
    DispatchRequest::export_all(&config)?;
    ObserveRequest::export_all(&config)?;
    DispatchResponse::export_all(&config)?;
    ObserveResponse::export_all(&config)?;
    ObservationUpdate::export_all(&config)?;
    MenuCommand::export_all(&config)?;
    FeatureEntry::export_all(&config)?;
    FeatureId::export_all(&config)?;
    Availability::export_all(&config)?;
    Ok(())
}
