//! The tenant-scoped case Library (C12): five products' authored cases, of
//! which only Runtime Security cases can fill the scan form; the others are
//! specifications to inspect and transfer. Nothing here executes anything.
//!
//! Working v1 (74754c02, read-only): `src/settings.rs` (`TestCase`, its
//! limits and the export file), `src/case_library.rs` (products, the example
//! catalog and its URL rule) and `examples/test-cases/catalog.json` (the
//! bundled examples, copied to `assets/case-catalog.json`). `expected` is a
//! free-text test goal: v1 never compared it with a verdict, and neither
//! does v2.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::vault::{RecordId, Vault, VaultError};

/// Cases per tenant (v1 `CASE_LIMIT`).
pub const CASE_LIMIT: usize = 32;
/// Largest encoded library, export, import or catalog (v1).
pub const LIBRARY_BYTES: usize = 256 * 1024;
/// v1's field limits.
pub const NAME_LIMIT: usize = 128;
pub const PROFILE_LIMIT: usize = 256;
pub const TEXT_LIMIT: usize = 96 * 1024;
pub const EXPECTED_LIMIT: usize = 1024;
/// The bundled example catalog (v1's `examples/test-cases/catalog.json`).
pub const BUNDLED_CATALOG: &[u8] = include_bytes!("../assets/case-catalog.json");
/// The public example catalog (v1's documented URL).
pub const PUBLIC_CATALOG: &str =
    "https://raw.githubusercontent.com/cdot65/prisma-airs-test-cases/main/catalog.json";
/// The export file's suggested name (v1).
pub const EXPORT_NAME: &str = "prisma-airs-test-cases.json";

/// The five products a case can belong to (v1 wire values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CaseProduct {
    AiGateway,
    AiRedTeaming,
    ModelSecurity,
    #[default]
    RuntimeSecurity,
    SkillScanning,
}

impl CaseProduct {
    /// Only Runtime Security cases can fill the scan form.
    pub fn can_scan(self) -> bool {
        self == Self::RuntimeSecurity
    }
}

/// One case as v1 stored and exported it (snake_case fields).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCase {
    pub id: String,
    pub tenant: String,
    #[serde(default)]
    pub product: CaseProduct,
    pub name: String,
    #[serde(default)]
    pub profile: String,
    pub prompt: String,
    pub response: String,
    #[serde(default)]
    pub app_name: String,
    #[serde(default)]
    pub expected: String,
    #[serde(default)]
    pub payload: Value,
}

/// Why a case, import or catalog was refused (v1's messages are the
/// presentation's).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CaseProblem {
    /// A name, profile or application is missing, or a field is too long.
    Fields,
    /// A product payload is not a JSON object.
    Payload,
    /// The case belongs to another tenant.
    ForeignTenant,
    /// The tenant already has 32 cases (or an import would exceed them).
    Full,
    /// The library would exceed 256 KiB.
    TooLarge,
    /// A case with this identifier already exists, or a file repeats one.
    Duplicate,
    /// The file is not a case export of this tenant with at most 32 cases.
    Format,
    /// The catalog is not a version 1 example catalog of 1-32 valid examples.
    Catalog,
    /// Every example is already in this tenant's library.
    AllPresent,
    /// No such case.
    Missing,
}

/// A v4 UUID in its text form.
pub fn new_id() -> Option<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).ok()?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    Some(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl TestCase {
    /// v1's `TestCase::validate` for `tenant`, with a well-formed ID.
    pub fn validate(&self, tenant: &str) -> Result<(), CaseProblem> {
        if self.tenant != tenant {
            return Err(CaseProblem::ForeignTenant);
        }
        if !self.payload.is_null() && !self.payload.is_object() {
            return Err(CaseProblem::Payload);
        }
        let runtime = self.product.can_scan();
        let ok = is_uuid(&self.id)
            && !self.name.trim().is_empty()
            && self.name.len() <= NAME_LIMIT
            && !(runtime && self.profile.trim().is_empty())
            && self.profile.len() <= PROFILE_LIMIT
            && !(runtime && self.app_name.trim().is_empty())
            && self.app_name.len() <= PROFILE_LIMIT
            && self.prompt.len() + self.response.len() <= TEXT_LIMIT
            && self.expected.len() <= EXPECTED_LIMIT
            && serde_json::to_vec(&self.payload).is_ok_and(|b| b.len() <= TEXT_LIMIT);
        ok.then_some(()).ok_or(CaseProblem::Fields)
    }
}

/// What the operator authors (the presentation's fields, read when Save is
/// pressed). Runtime cases never carry a payload; the other products never
/// carry a profile or application (v1 `author.rs`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CaseDraft {
    pub product: CaseProduct,
    pub name: String,
    pub profile: String,
    pub prompt: String,
    pub response: String,
    pub app_name: String,
    pub expected: String,
    /// A JSON object as text (non-Runtime products; empty for none).
    pub payload: String,
}

impl CaseDraft {
    /// The case this draft authors for `tenant`.
    pub fn into_case(self, tenant: &str) -> Result<TestCase, CaseProblem> {
        let runtime = self.product.can_scan();
        let payload = if runtime || self.payload.trim().is_empty() {
            Value::Null
        } else {
            match serde_json::from_str::<Value>(&self.payload) {
                Ok(value @ Value::Object(_)) => value,
                _ => return Err(CaseProblem::Payload),
            }
        };
        let case = TestCase {
            id: new_id().ok_or(CaseProblem::Fields)?,
            tenant: tenant.to_owned(),
            product: self.product,
            name: self.name,
            profile: if runtime { self.profile } else { String::new() },
            prompt: self.prompt,
            response: self.response,
            app_name: if runtime {
                self.app_name
            } else {
                String::new()
            },
            expected: self.expected,
            payload,
        };
        case.validate(tenant)?;
        Ok(case)
    }
}

/// A case as the presentation sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CaseView {
    pub id: String,
    pub product: CaseProduct,
    pub name: String,
    pub profile: String,
    pub prompt: String,
    pub response: String,
    pub app_name: String,
    pub expected: String,
    /// The payload as pretty JSON (non-Runtime products), if any.
    pub payload: Option<String>,
}

impl From<&TestCase> for CaseView {
    fn from(c: &TestCase) -> Self {
        Self {
            id: c.id.clone(),
            product: c.product,
            name: c.name.clone(),
            profile: c.profile.clone(),
            prompt: c.prompt.clone(),
            response: c.response.clone(),
            app_name: c.app_name.clone(),
            expected: c.expected.clone(),
            payload: (!c.payload.is_null())
                .then(|| serde_json::to_string_pretty(&c.payload).ok())
                .flatten(),
        }
    }
}

/// The cases of one tenant, in order, within v1's bounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CaseLibrary {
    pub cases: Vec<TestCase>,
}

impl CaseLibrary {
    fn encoded_len(cases: &[TestCase]) -> usize {
        serde_json::to_vec(cases).map_or(usize::MAX, |b| b.len())
    }

    /// v1 `add_case`: validated, unique, within 32 cases and 256 KiB.
    pub fn add(&mut self, case: TestCase, tenant: &str) -> Result<(), CaseProblem> {
        case.validate(tenant)?;
        if self.cases.len() >= CASE_LIMIT {
            return Err(CaseProblem::Full);
        }
        if self.cases.iter().any(|c| c.id == case.id) {
            return Err(CaseProblem::Duplicate);
        }
        let mut next = self.cases.clone();
        next.push(case);
        if Self::encoded_len(&next) > LIBRARY_BYTES {
            return Err(CaseProblem::TooLarge);
        }
        self.cases = next;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Option<TestCase> {
        let index = self.cases.iter().position(|c| c.id == id)?;
        Some(self.cases.remove(index))
    }

    /// v1 `merge_imported_cases`: identifiers already present are skipped;
    /// if the rest would exceed 32 cases (or 256 KiB) nothing is added.
    /// Returns how many were added.
    pub fn merge(&mut self, incoming: Vec<TestCase>) -> Result<usize, CaseProblem> {
        let fresh: Vec<TestCase> = incoming
            .into_iter()
            .filter(|c| !self.cases.iter().any(|e| e.id == c.id))
            .collect();
        if self.cases.len() + fresh.len() > CASE_LIMIT {
            return Err(CaseProblem::Full);
        }
        let mut next = self.cases.clone();
        let added = fresh.len();
        next.extend(fresh);
        if Self::encoded_len(&next) > LIBRARY_BYTES {
            return Err(CaseProblem::TooLarge);
        }
        self.cases = next;
        Ok(added)
    }

    /// Checks stored cases on load (v1 re-validates every case).
    pub fn from_cases(cases: Vec<TestCase>, tenant: &str) -> Option<Self> {
        let ok = cases.len() <= CASE_LIMIT
            && Self::encoded_len(&cases) <= LIBRARY_BYTES
            && cases.iter().all(|c| c.validate(tenant).is_ok())
            && cases.iter().map(|c| &c.id).collect::<HashSet<_>>().len() == cases.len();
        ok.then_some(Self { cases })
    }
}

/// The export file (v1 `CaseExport`, version 2).
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseExport {
    version: u8,
    tenant: String,
    cases: Vec<TestCase>,
}

/// The pretty JSON export of a tenant's cases (plaintext, v1 version 2).
pub fn export(tenant: &str, cases: &[TestCase]) -> Result<Vec<u8>, CaseProblem> {
    if cases.len() > CASE_LIMIT {
        return Err(CaseProblem::Full);
    }
    let bytes = serde_json::to_vec_pretty(&CaseExport {
        version: 2,
        tenant: tenant.to_owned(),
        cases: cases.to_vec(),
    })
    .map_err(|_| CaseProblem::Format)?;
    if bytes.len() > LIBRARY_BYTES {
        return Err(CaseProblem::TooLarge);
    }
    Ok(bytes)
}

/// v1's import rules: at most 256 KiB, version 1 or 2, this tenant, at most
/// 32 valid cases with distinct identifiers.
pub fn parse_import(bytes: &[u8], tenant: &str) -> Result<Vec<TestCase>, CaseProblem> {
    if bytes.len() > LIBRARY_BYTES {
        return Err(CaseProblem::TooLarge);
    }
    let file: CaseExport = serde_json::from_slice(bytes).map_err(|_| CaseProblem::Format)?;
    if !(1..=2).contains(&file.version) || file.tenant != tenant || file.cases.len() > CASE_LIMIT {
        return Err(CaseProblem::Format);
    }
    for case in &file.cases {
        case.validate(tenant)?;
    }
    if file
        .cases
        .iter()
        .map(|c| &c.id)
        .collect::<HashSet<_>>()
        .len()
        != file.cases.len()
    {
        return Err(CaseProblem::Duplicate);
    }
    Ok(file.cases)
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Example {
    product: CaseProduct,
    name: String,
    #[serde(default)]
    profile: String,
    #[serde(default)]
    app_name: String,
    prompt: String,
    #[serde(default)]
    response: String,
    expected: String,
    #[serde(default)]
    payload: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExampleCatalog {
    version: u8,
    kind: String,
    examples: Vec<Example>,
}

/// v1 `ExampleCatalog::parse` then `into_cases`: every example becomes a
/// new case of `tenant`.
pub fn parse_catalog(bytes: &[u8], tenant: &str) -> Result<Vec<TestCase>, CaseProblem> {
    if bytes.len() > LIBRARY_BYTES {
        return Err(CaseProblem::Catalog);
    }
    let catalog: ExampleCatalog =
        serde_json::from_slice(bytes).map_err(|_| CaseProblem::Catalog)?;
    if catalog.version != 1
        || catalog.kind != "prisma-airs-test-case-examples"
        || catalog.examples.is_empty()
        || catalog.examples.len() > CASE_LIMIT
    {
        return Err(CaseProblem::Catalog);
    }
    let mut names = HashSet::new();
    let mut cases = Vec::new();
    for e in catalog.examples {
        if e.expected.trim().is_empty()
            || e.expected.len() > EXPECTED_LIMIT
            || !names.insert((e.product, e.name.to_lowercase()))
        {
            return Err(CaseProblem::Catalog);
        }
        let case = TestCase {
            id: new_id().ok_or(CaseProblem::Catalog)?,
            tenant: tenant.to_owned(),
            product: e.product,
            name: e.name,
            profile: e.profile,
            prompt: e.prompt,
            response: e.response,
            app_name: e.app_name,
            expected: e.expected,
            payload: e.payload,
        };
        case.validate(tenant).map_err(|_| CaseProblem::Catalog)?;
        cases.push(case);
    }
    Ok(cases)
}

/// v1 `library/catalog.rs`: examples whose (product, name) the library
/// already holds (ASCII case-insensitive) are dropped.
pub fn new_examples(library: &CaseLibrary, examples: Vec<TestCase>) -> Vec<TestCase> {
    examples
        .into_iter()
        .filter(|e| {
            !library
                .cases
                .iter()
                .any(|c| c.product == e.product && c.name.eq_ignore_ascii_case(&e.name))
        })
        .collect()
}

/// v1 `validate_catalog_url`, for the configured catalog origin (the
/// production origin is raw.githubusercontent.com; fixture builds may point
/// it at a local snapshot server): HTTPS, no port beyond the origin's, no
/// credentials, query or fragment, a `.json` path of at least four segments.
pub fn catalog_url_ok(url: &url::Url, origin: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == origin.host_str()
        && url.port() == origin.port()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().ends_with(".json")
        && url.path_segments().is_some_and(|s| s.count() >= 4)
}

// ----------------------------------------------------------- sealed file

const SCHEMA: &str = "prisma-airs-desktop-v2/case-library/1";

/// The credential-namespace Keychain record holding a tenant's library key,
/// derived from the tenant (no persisted reference; removed with the tenant
/// and by reset).
pub fn key_record(tenant: &str) -> RecordId {
    let digest = crate::offline::digest(&format!("case-library-key:{tenant}"));
    RecordId::parse(&format!("cr-{}", &digest[..16])).expect("16 hex digits")
}

/// Where a tenant's sealed library lives.
pub fn library_path(root: &Path, tenant: &str) -> PathBuf {
    let digest = crate::offline::digest(&format!("case-library:{tenant}"));
    root.join(format!("{}.sealed", &digest[..32]))
}

fn aad(tenant: &str) -> Vec<u8> {
    format!("{SCHEMA}:{}", crate::offline::digest(tenant)).into_bytes()
}

/// Why a library could not be opened or saved (sanitized).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LibraryFailure {
    /// The Keychain is locked.
    Locked,
    /// macOS denied Keychain access.
    Denied,
    /// The file or Keychain is unavailable.
    Unavailable,
    /// The saved library does not open under its key; it was kept aside.
    Unreadable,
}

fn vault_failure(error: VaultError) -> LibraryFailure {
    match error {
        VaultError::Locked => LibraryFailure::Locked,
        VaultError::Denied => LibraryFailure::Denied,
        _ => LibraryFailure::Unavailable,
    }
}

fn key(
    keychain: &dyn Vault,
    tenant: &str,
    create: bool,
) -> Result<Option<[u8; 32]>, LibraryFailure> {
    let record = key_record(tenant);
    let bytes = match keychain.get(&record) {
        Ok(bytes) => bytes,
        Err(VaultError::NotFound) if create => {
            let fresh = crate::offline::new_key().ok_or(LibraryFailure::Unavailable)?;
            keychain.put(&record, &fresh).map_err(vault_failure)?;
            fresh
        }
        Err(VaultError::NotFound) => return Ok(None),
        Err(e) => return Err(vault_failure(e)),
    };
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LibraryFailure::Unreadable)?;
    Ok(Some(key))
}

/// Opens a tenant's sealed library (empty when nothing was saved). A file
/// that does not open is renamed aside and reported, never overwritten.
pub fn load(
    root: &Path,
    keychain: &dyn Vault,
    tenant: &str,
) -> Result<CaseLibrary, LibraryFailure> {
    use std::io::Read;
    let path = library_path(root, tenant);
    let mut sealed = Vec::new();
    match std::fs::File::open(&path) {
        Ok(file) => {
            file.take(LIBRARY_BYTES as u64 + 1024)
                .read_to_end(&mut sealed)
                .map_err(|_| LibraryFailure::Unavailable)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CaseLibrary::default()),
        Err(_) => return Err(LibraryFailure::Unavailable),
    }
    let opened = key(keychain, tenant, false)?
        .and_then(|key| crate::sessions::open(&key, &aad(tenant), &sealed))
        .and_then(|plain| serde_json::from_slice::<Vec<TestCase>>(&plain).ok())
        .and_then(|cases| CaseLibrary::from_cases(cases, tenant));
    match opened {
        Some(library) => Ok(library),
        None => {
            let _ = std::fs::rename(&path, path.with_extension("unreadable"));
            Err(LibraryFailure::Unreadable)
        }
    }
}

/// Seals and replaces a tenant's library (0600, atomic) under the placement
/// lock; nothing is placed for a retired tenant.
pub fn store(
    root: &Path,
    keychain: &dyn Vault,
    tenant: &str,
    library: &CaseLibrary,
) -> Result<(), LibraryFailure> {
    let plain = serde_json::to_vec(&library.cases).map_err(|_| LibraryFailure::Unavailable)?;
    let key = key(keychain, tenant, true)?.ok_or(LibraryFailure::Unavailable)?;
    let sealed =
        crate::sessions::seal(&key, &aad(tenant), &plain).ok_or(LibraryFailure::Unavailable)?;
    crate::sessions::write_private(&library_path(root, tenant), &sealed)
        .map_err(|_| LibraryFailure::Unavailable)
}

/// Removes a deleted tenant's library and key; true when both are gone.
pub fn forget(root: Option<&Path>, keychain: &dyn Vault, tenant: &str) -> bool {
    let file = root.is_none_or(|root| {
        let path = library_path(root, tenant);
        let _ = std::fs::remove_file(path.with_extension("unreadable"));
        match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    });
    let key = matches!(
        keychain.delete(&key_record(tenant)),
        Ok(()) | Err(VaultError::NotFound)
    );
    file && key
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T: &str = "tn-0123456789abcdef";

    fn draft(product: CaseProduct) -> CaseDraft {
        CaseDraft {
            product,
            name: "Case".into(),
            profile: "prod".into(),
            prompt: "hello".into(),
            response: String::new(),
            app_name: "App".into(),
            expected: "A goal".into(),
            payload: "{\"k\":1}".into(),
        }
    }

    #[test]
    fn drafts_follow_v1s_product_rules_and_limits() {
        let runtime = draft(CaseProduct::RuntimeSecurity)
            .into_case(T)
            .expect("runtime");
        assert!(runtime.payload.is_null(), "runtime cases carry no payload");
        assert!(is_uuid(&runtime.id));
        let model = draft(CaseProduct::ModelSecurity)
            .into_case(T)
            .expect("model");
        assert_eq!((model.profile.as_str(), model.app_name.as_str()), ("", ""));
        assert_eq!(model.payload, json!({"k": 1}));
        let mut bad = draft(CaseProduct::SkillScanning);
        bad.payload = "[1]".into();
        assert_eq!(bad.into_case(T), Err(CaseProblem::Payload));
        let mut nameless = draft(CaseProduct::RuntimeSecurity);
        nameless.name = "  ".into();
        assert_eq!(nameless.into_case(T), Err(CaseProblem::Fields));
        let mut no_profile = draft(CaseProduct::RuntimeSecurity);
        no_profile.profile = String::new();
        assert_eq!(no_profile.into_case(T), Err(CaseProblem::Fields));
        // A specification needs no profile or application.
        let mut spec = draft(CaseProduct::AiGateway);
        spec.profile = String::new();
        spec.app_name = String::new();
        assert!(spec.into_case(T).is_ok());
        let mut long = draft(CaseProduct::RuntimeSecurity);
        long.name = "n".repeat(NAME_LIMIT + 1);
        assert_eq!(long.into_case(T), Err(CaseProblem::Fields));
        let mut text = draft(CaseProduct::RuntimeSecurity);
        text.prompt = "p".repeat(TEXT_LIMIT);
        text.response = "r".into();
        assert_eq!(text.into_case(T), Err(CaseProblem::Fields));
        let mut goal = draft(CaseProduct::RuntimeSecurity);
        goal.expected = "e".repeat(EXPECTED_LIMIT + 1);
        assert_eq!(goal.into_case(T), Err(CaseProblem::Fields));
    }

    #[test]
    fn the_library_is_bounded_and_imports_merge_like_v1() {
        let mut library = CaseLibrary::default();
        for _ in 0..CASE_LIMIT {
            library
                .add(
                    draft(CaseProduct::RuntimeSecurity)
                        .into_case(T)
                        .expect("ok"),
                    T,
                )
                .expect("added");
        }
        assert_eq!(
            library.add(
                draft(CaseProduct::RuntimeSecurity)
                    .into_case(T)
                    .expect("ok"),
                T
            ),
            Err(CaseProblem::Full)
        );
        let first = library.cases[0].clone();
        assert_eq!(library.add(first.clone(), T), Err(CaseProblem::Full));
        library.remove(&first.id).expect("removed");
        assert_eq!(
            library.add(library.cases[0].clone(), T),
            Err(CaseProblem::Duplicate)
        );
        // A merge skips present identifiers and refuses to exceed the limit.
        let present = library.cases[0].clone();
        assert_eq!(library.merge(vec![present]), Ok(0));
        let two = vec![
            draft(CaseProduct::AiGateway).into_case(T).expect("ok"),
            draft(CaseProduct::AiGateway).into_case(T).expect("ok"),
        ];
        assert_eq!(library.merge(two), Err(CaseProblem::Full));
        assert_eq!(library.cases.len(), CASE_LIMIT - 1);
    }

    #[test]
    fn exports_and_imports_keep_v1s_file_and_refuse_foreign_or_malformed_files() {
        let case = draft(CaseProduct::RuntimeSecurity)
            .into_case(T)
            .expect("ok");
        let bytes = export(T, std::slice::from_ref(&case)).expect("export");
        let text = String::from_utf8(bytes.clone()).expect("ok");
        assert!(
            text.contains("\"version\": 2") && text.contains("\"product\": \"runtime-security\"")
        );
        assert_eq!(parse_import(&bytes, T), Ok(vec![case.clone()]));
        assert_eq!(
            parse_import(&bytes, "tn-ffffffffffffffff"),
            Err(CaseProblem::Format)
        );
        let v1 = json!({"version": 1, "tenant": T, "cases": [case]}).to_string();
        assert!(parse_import(v1.as_bytes(), T).is_ok());
        let v3 = json!({"version": 3, "tenant": T, "cases": []}).to_string();
        assert_eq!(parse_import(v3.as_bytes(), T), Err(CaseProblem::Format));
        let extra = json!({"version": 2, "tenant": T, "cases": [], "api_key": "x"}).to_string();
        assert_eq!(parse_import(extra.as_bytes(), T), Err(CaseProblem::Format));
        let mut unknown = serde_json::to_value(&case).expect("ok");
        unknown["product"] = json!("prisma-access");
        let file = json!({"version": 2, "tenant": T, "cases": [unknown]}).to_string();
        assert_eq!(parse_import(file.as_bytes(), T), Err(CaseProblem::Format));
        let dup = json!({"version": 2, "tenant": T, "cases": [case, case]}).to_string();
        assert_eq!(parse_import(dup.as_bytes(), T), Err(CaseProblem::Duplicate));
        let foreign_case = json!({"version": 2, "tenant": T, "cases": [
            {"id": case.id, "tenant": "tn-ffffffffffffffff", "name": "n", "profile": "p", "prompt": "x",
             "response": "", "app_name": "a"}]}).to_string();
        assert_eq!(
            parse_import(foreign_case.as_bytes(), T),
            Err(CaseProblem::ForeignTenant)
        );
        assert_eq!(
            parse_import(&vec![b' '; LIBRARY_BYTES + 1], T),
            Err(CaseProblem::TooLarge)
        );
    }

    #[test]
    fn the_bundled_catalog_has_v1s_eleven_examples_across_five_products() {
        let cases = parse_catalog(BUNDLED_CATALOG, T).expect("catalog");
        assert_eq!(cases.len(), 11);
        let products: HashSet<_> = cases.iter().map(|c| c.product).collect();
        assert_eq!(products.len(), 5);
        assert!(
            cases
                .iter()
                .all(|c| c.tenant == T && !c.expected.is_empty())
        );
        // Importing twice adds nothing the second time.
        let mut library = CaseLibrary::default();
        library
            .merge(new_examples(&library, cases))
            .expect("merged");
        let again = parse_catalog(BUNDLED_CATALOG, T).expect("ok");
        assert!(new_examples(&library, again).is_empty());
        for bad in [
            json!({"version": 2, "kind": "prisma-airs-test-case-examples", "examples": []}),
            json!({"version": 1, "kind": "other", "examples": []}),
            json!({"version": 1, "kind": "prisma-airs-test-case-examples", "examples": []}),
            json!({"version": 1, "kind": "prisma-airs-test-case-examples", "api_key": "x", "examples": []}),
            json!({"version": 1, "kind": "prisma-airs-test-case-examples", "examples": [
                {"product": "ai-gateway", "name": "A", "prompt": "p", "expected": ""}]}),
            json!({"version": 1, "kind": "prisma-airs-test-case-examples", "examples": [
                {"product": "ai-gateway", "name": "A", "prompt": "p", "expected": "e"},
                {"product": "ai-gateway", "name": "a", "prompt": "p", "expected": "e"}]}),
            json!({"version": 1, "kind": "prisma-airs-test-case-examples", "examples": [
                {"product": "quantum", "name": "A", "prompt": "p", "expected": "e"}]}),
        ] {
            assert_eq!(
                parse_catalog(bad.to_string().as_bytes(), T),
                Err(CaseProblem::Catalog),
                "{bad}"
            );
        }
    }

    #[test]
    fn catalog_urls_follow_v1s_rule_for_the_configured_origin() {
        let origin = url::Url::parse("https://raw.githubusercontent.com").expect("ok");
        let ok = |s: &str| catalog_url_ok(&url::Url::parse(s).expect("ok"), &origin);
        assert!(ok(PUBLIC_CATALOG));
        assert!(!ok(
            "http://raw.githubusercontent.com/a/b/main/catalog.json"
        ));
        assert!(!ok("https://example.com/a/b/main/catalog.json"));
        assert!(!ok(
            "https://raw.githubusercontent.com:8443/a/b/main/catalog.json"
        ));
        assert!(!ok(
            "https://u:p@raw.githubusercontent.com/a/b/main/catalog.json"
        ));
        assert!(!ok(
            "https://raw.githubusercontent.com/a/b/main/catalog.json?token=x"
        ));
        assert!(!ok(
            "https://raw.githubusercontent.com/a/b/main/catalog.json#f"
        ));
        assert!(!ok(
            "https://raw.githubusercontent.com/a/b/main/catalog.txt"
        ));
        assert!(!ok("https://raw.githubusercontent.com/a/catalog.json"));
    }
}
