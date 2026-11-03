//! Shared native file-picker failures. No tenant/profile reader or discovery path.
use serde::Serialize;
use ts_rs::TS;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum ImportError {
    #[error("the selected file exceeds its size limit")]
    TooLarge,
    #[error("the selected file is not supported")]
    Unsupported,
    #[error("the selected file is empty")]
    Empty,
}
