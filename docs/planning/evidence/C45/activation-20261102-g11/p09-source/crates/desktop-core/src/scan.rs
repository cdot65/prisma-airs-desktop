//! SDK scan client binding for one credential/context generation.
//!
//! Each admitted credential generation gets its own SDK `Session`, cancelled
//! when that generation retires; a replacement never reuses a cancelled
//! session. The client allows exactly the credential's approved origin, uses
//! the app-owned transport and the explicit first-scan limits, and every
//! request disables business-operation retries. The scan operation itself is
//! introduced in C03; C02 qualifies this binding with fixture transports.

use std::sync::Arc;
use std::time::Duration;

use scm_sdk::auth::Authentication;
use scm_sdk::transport::Transport;
use scm_sdk::{Client, Endpoint, Limits, OriginPolicy, RequestOptions, Secret, Session};
use zeroize::Zeroizing;

/// Serialized scan request ceiling (matches the SDK default).
pub const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
/// Decoded scan response ceiling.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Interactive deadline for one scan, from submission.
pub const INTERACTIVE_DEADLINE: Duration = Duration::from_secs(60);

/// The explicit first-scan SDK limits.
pub fn scan_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_request_bytes = MAX_REQUEST_BYTES;
    limits.max_response_bytes = MAX_RESPONSE_BYTES;
    limits.operation_timeout = INTERACTIVE_DEADLINE;
    limits.max_retries = 0;
    limits
}

/// Why a binding could not be created (sanitized).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BindingError {
    #[error("the scan destination is not an approved HTTPS origin")]
    Destination,
    #[error("the scan key is not usable")]
    Credential,
    #[error("the scan client could not be configured")]
    Client,
}

/// An SDK client bound to one credential generation and its destination.
pub struct ScanBinding {
    client: Client,
    session: Session,
    /// Whether dropping the binding retires its session (a binding built on
    /// a shared generation session leaves that to the generation).
    owns_session: bool,
}

impl ScanBinding {
    /// Builds the binding. `key` is consumed; the SDK keeps it in its own
    /// zeroizing `Secret`.
    pub fn new(
        origin: &str,
        key: Zeroizing<String>,
        transport: Arc<dyn Transport>,
    ) -> Result<Self, BindingError> {
        let mut binding = Self::with_session(origin, key, transport, Session::new())?;
        binding.owns_session = true;
        Ok(binding)
    }

    /// Builds the binding on a given credential-generation session; the
    /// session is cancelled when that generation retires.
    pub fn with_session(
        origin: &str,
        key: Zeroizing<String>,
        transport: Arc<dyn Transport>,
        session: Session,
    ) -> Result<Self, BindingError> {
        let endpoint = Endpoint::parse(origin).map_err(|_| BindingError::Destination)?;
        if endpoint.origin() != origin {
            return Err(BindingError::Destination);
        }
        let secret = Secret::new(key.as_str()).map_err(|_| BindingError::Credential)?;
        drop(key);
        let authentication =
            Authentication::scan(Some(secret), None).map_err(|_| BindingError::Credential)?;
        let client = Client::builder(endpoint.clone())
            .authentication(authentication)
            .transport(transport)
            .destination_policy(Arc::new(OriginPolicy::new([endpoint])))
            .limits(scan_limits())
            .session(session.clone())
            .build()
            .map_err(|_| BindingError::Client)?;
        Ok(Self {
            client,
            session,
            owns_session: false,
        })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Per-request options: the remaining interactive budget, no retries.
    pub fn request_options(remaining: Duration) -> Result<RequestOptions, BindingError> {
        RequestOptions::with_timeout(remaining.max(Duration::from_millis(1)))
            .and_then(|o| o.with_retries(0))
            .map_err(|_| BindingError::Client)
    }

    /// Cancels the generation's session (retirement); in-flight requests stop.
    pub fn retire(&self) {
        self.session.cancel();
    }

    pub fn is_retired(&self) -> bool {
        self.session.is_cancelled()
    }
}

impl Drop for ScanBinding {
    fn drop(&mut self) {
        if self.owns_session {
            self.session.cancel();
        }
    }
}

/// A key the SDK would accept in its scan header, checked at entry so an
/// unusable key is refused before anything is stored.
pub fn key_is_usable(key: &str) -> bool {
    Secret::new(key)
        .ok()
        .and_then(|s| Authentication::scan(Some(s), None).ok())
        .is_some()
}
