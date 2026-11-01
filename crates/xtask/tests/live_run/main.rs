//! The C25 controlled live runner against synthetic TLS services on this
//! Mac: the production core is driven exactly as for a live run, but every
//! receipt is recorded as `fixture` evidence. Secrets come from an
//! in-memory resolver and browser steps from a stand-in operator; no
//! Keychain item is read and no product endpoint is called.

#[allow(dead_code)]
#[path = "../../../desktop-core/tests/context_credentials/support.rs"]
mod support;

mod cloud;

/// What the shared MCP fixture expects of its desk.
mod desk {
    pub const DATA: &str = crate::cloud::DATA;
}

mod common;
#[allow(dead_code)]
#[path = "../../../desktop-core/tests/workspace_mcp/fixture.rs"]
mod mcp_fixture;

mod academy;
mod changes;
mod inference;
mod mcp;
mod reads;
mod scan;
mod wizard;
mod workbench;
