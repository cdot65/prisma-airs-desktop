//! cURL import (C14, v1's rules): one pasted command is lexed (never run by
//! a shell, no file or network read) into a new unsaved request. Recognized
//! credentials become secret references whose values the operator enters
//! in Secrets; credential-named URL query, JSON or form fields are refused.

use serde::Serialize;
use toml_edit::{DocumentMut, InlineTable, Item, Table, value};
use ts_rs::TS;

use super::document::Fields;

pub const CURL_BYTES: usize = 1024 * 1024;
pub const MAX_WORDS: usize = 4096;

/// Why a command could not be imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CurlProblem {
    TooLarge,
    /// The input does not begin with `curl`.
    NotCurl,
    /// Shell expansion, operators, comments or separators.
    Shell,
    Unclosed,
    TooManyWords,
    /// An option outside the supported list.
    UnsupportedOption,
    MissingArgument,
    /// No URL, several URLs, or not an absolute HTTP(S) URL.
    Url,
    /// Credentials in the URL's user-info.
    UrlCredentials,
    /// A credential-named query parameter.
    QueryCredentials,
    /// URL glob expressions.
    Glob,
    /// `{{` template syntax in the URL, a header or the body.
    Template,
    /// A credential-named JSON or form body field.
    BodyCredentials,
    /// `--json` data that is not JSON.
    Json,
    /// A header that is not a single-line `Name: value` with a value.
    Header,
    /// Files, stdin or interactive prompts.
    External,
    /// Options that cannot be combined (or an invalid method).
    Combination,
}

/// What the conversion did (names and references only, never values).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportNote {
    /// A presentation or globbing flag was left out.
    FlagOmitted,
    /// An auth option's credential became a secret reference.
    AuthRemoved { reference: String },
    /// A credential-named header became a secret variable.
    HeaderReplaced { variable: String, reference: String },
    /// `--compressed` became `Accept-Encoding: identity`.
    Compressed,
    /// An empty cookie option added no cookie.
    EmptyCookie,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversion {
    pub fields: Fields,
    pub notes: Vec<ImportNote>,
}

/// Names understood as credentials, including common custom headers (v1).
pub fn credential_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase().replace(['-', '_'], "");
    if matches!(name.as_str(), "key" | "auth" | "credential" | "credentials") {
        return true;
    }
    [
        "authorization",
        "cookie",
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "accesskey",
    ]
    .iter()
    .any(|part| name.contains(part))
}

/// `$'...'` ANSI-C quoting: common escapes only.
fn ansi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<String, CurlProblem> {
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\'' => return Ok(out),
            '\\' => match chars.next().ok_or(CurlProblem::Unclosed)? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '\\' => out.push('\\'),
                '\'' => out.push('\''),
                '"' => out.push('"'),
                'x' => {
                    let hex: String = (0..2).filter_map(|_| chars.next()).collect();
                    let byte = u8::from_str_radix(&hex, 16).map_err(|_| CurlProblem::Shell)?;
                    if !byte.is_ascii() || byte == 0 {
                        return Err(CurlProblem::Shell);
                    }
                    out.push(byte as char);
                }
                _ => return Err(CurlProblem::Shell),
            },
            c => out.push(c),
        }
    }
    Err(CurlProblem::Unclosed)
}

/// A deliberately small Unix command lexer, never a shell evaluator.
pub fn words(input: &str) -> Result<Vec<String>, CurlProblem> {
    if input.len() > CURL_BYTES {
        return Err(CurlProblem::TooLarge);
    }
    if input.contains("\\\r\n") || input.contains('\0') {
        return Err(CurlProblem::Shell);
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    word.push(c);
                }
            }
            Some(_) => match c {
                '"' => quote = None,
                '$' | '`' => return Err(CurlProblem::Shell),
                '\\' => {
                    let next = chars.next().ok_or(CurlProblem::Unclosed)?;
                    if next != '\n' {
                        if !matches!(next, '$' | '`' | '"' | '\\') {
                            word.push('\\');
                        }
                        word.push(next);
                    }
                }
                _ => word.push(c),
            },
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    started = true;
                }
                '\\' => {
                    let next = chars.next().ok_or(CurlProblem::Unclosed)?;
                    if next != '\n' {
                        word.push(next);
                        started = true;
                    }
                }
                '$' if chars.peek() == Some(&'\'') => {
                    chars.next();
                    word.push_str(&ansi(&mut chars)?);
                    started = true;
                }
                '$' | '`' | ';' | '|' | '&' | '<' | '>' | '(' | ')' | '#' | '*' | '?' | '['
                | ']' | '~' | '{' | '}' => return Err(CurlProblem::Shell),
                '\n' | '\r' => {
                    if words.is_empty() && !started {
                        continue;
                    }
                    if !chars.clone().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
                        return Err(CurlProblem::Shell);
                    }
                    break;
                }
                ' ' | '\t' => {
                    if started {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                }
                _ => {
                    word.push(c);
                    started = true;
                }
            },
        }
        if words.len() > MAX_WORDS {
            return Err(CurlProblem::TooManyWords);
        }
    }
    if quote.is_some() {
        return Err(CurlProblem::Unclosed);
    }
    if started {
        words.push(word);
    }
    if words.len() > MAX_WORDS {
        return Err(CurlProblem::TooManyWords);
    }
    Ok(words)
}

#[derive(Default)]
struct Arguments {
    url: Option<String>,
    method: Option<String>,
    headers: Vec<String>,
    user_agent: Option<String>,
    cookie: Option<String>,
    compressed: bool,
    body: String,
    /// Some(true): `--json`; Some(false): form data.
    body_mode: Option<bool>,
    /// (kind, basic username)
    auth: Option<(&'static str, String)>,
    head: bool,
    globoff: bool,
    notes: Vec<ImportNote>,
}

fn arguments(words: &[String]) -> Result<Arguments, CurlProblem> {
    if words.first().map(String::as_str) != Some("curl") {
        return Err(CurlProblem::NotCurl);
    }
    let mut a = Arguments::default();
    let mut literal_urls = false;
    let mut i = 1;
    while i < words.len() {
        let word = &words[i];
        i += 1;
        if word == "--" && !literal_urls {
            literal_urls = true;
            continue;
        }
        if literal_urls || !word.starts_with('-') {
            if a.url.replace(word.clone()).is_some() {
                return Err(CurlProblem::Url);
            }
            continue;
        }
        let (option, attached) = if word.starts_with("--") {
            word.split_once('=')
                .map_or((word.as_str(), None), |(k, v)| (k, Some(v)))
        } else if word.len() > 2
            && matches!(word.get(..2), Some("-X" | "-H" | "-d" | "-u" | "-A" | "-b"))
        {
            (&word[..2], Some(&word[2..]))
        } else {
            (word.as_str(), None)
        };
        let takes_value = matches!(
            option,
            "-X" | "--request"
                | "--url"
                | "-H"
                | "--header"
                | "-d"
                | "--data"
                | "--data-ascii"
                | "--data-raw"
                | "--data-binary"
                | "--json"
                | "-u"
                | "--user"
                | "--oauth2-bearer"
                | "-A"
                | "--user-agent"
                | "-b"
                | "--cookie"
        );
        let argument = if takes_value {
            match attached {
                Some(v) => v.to_owned(),
                None => {
                    let v = words.get(i).ok_or(CurlProblem::MissingArgument)?.clone();
                    i += 1;
                    v
                }
            }
        } else {
            if attached.is_some() {
                return Err(CurlProblem::UnsupportedOption);
            }
            String::new()
        };
        match option {
            "-X" | "--request" => a.method = Some(argument),
            "--url" => {
                if a.url.replace(argument).is_some() {
                    return Err(CurlProblem::Url);
                }
            }
            "-H" | "--header" => {
                if argument.starts_with('@') {
                    return Err(CurlProblem::External);
                }
                a.headers.push(argument);
            }
            "-A" | "--user-agent" => a.user_agent = Some(argument),
            "-b" | "--cookie" => {
                if a.cookie.is_some() {
                    return Err(CurlProblem::Combination);
                }
                if !argument.is_empty() && !argument.contains('=') {
                    return Err(CurlProblem::External);
                }
                a.cookie = Some(argument);
            }
            "--compressed" => a.compressed = true,
            "-I" | "--head" => a.head = true,
            "-u" | "--user" => {
                if a.auth.is_some() {
                    return Err(CurlProblem::Combination);
                }
                let (username, _) = argument.split_once(':').ok_or(CurlProblem::External)?;
                if username.contains(['\r', '\n']) || username.contains("{{") {
                    return Err(CurlProblem::Template);
                }
                a.auth = Some(("basic", username.to_owned()));
            }
            "--oauth2-bearer" => {
                if a.auth.is_some() {
                    return Err(CurlProblem::Combination);
                }
                if argument.is_empty() {
                    return Err(CurlProblem::MissingArgument);
                }
                a.auth = Some(("bearer", String::new()));
            }
            "-d" | "--data" | "--data-ascii" | "--data-raw" | "--data-binary" | "--json" => {
                if option != "--data-raw" && argument.starts_with('@') {
                    return Err(CurlProblem::External);
                }
                let json = option == "--json";
                if a.body_mode.is_some_and(|old| old != json) {
                    return Err(CurlProblem::Combination);
                }
                if a.body_mode.is_some() && !json {
                    a.body.push('&');
                }
                a.body.push_str(&argument);
                a.body_mode = Some(json);
            }
            "-s" | "-S" | "-sS" | "-Ss" | "--silent" | "--show-error" | "-i" | "--include"
            | "--show-headers" | "-v" | "--verbose" | "--globoff" | "-g" => {
                a.globoff |= matches!(option, "--globoff" | "-g");
                if !a.notes.contains(&ImportNote::FlagOmitted) {
                    a.notes.push(ImportNote::FlagOmitted);
                }
            }
            _ => return Err(CurlProblem::UnsupportedOption),
        }
    }
    Ok(a)
}

fn reference() -> Result<String, CurlProblem> {
    super::project::new_id()
        .map(|id| format!("import-{}", &id[..16]))
        .ok_or(CurlProblem::External)
}

fn validate_url(url: &str, globoff: bool) -> Result<(), CurlProblem> {
    if url.chars().any(char::is_control) {
        return Err(CurlProblem::Url);
    }
    if url.contains("{{") {
        return Err(CurlProblem::Template);
    }
    let parsed = url::Url::parse(url).map_err(|_| CurlProblem::Url)?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(CurlProblem::Url);
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(CurlProblem::UrlCredentials);
    }
    if parsed.query_pairs().any(|(k, _)| credential_name(&k)) {
        return Err(CurlProblem::QueryCredentials);
    }
    if !globoff
        && (url.contains(['{', '}'])
            || parsed.path().contains(['[', ']'])
            || parsed.query().is_some_and(|q| q.contains(['[', ']'])))
    {
        return Err(CurlProblem::Glob);
    }
    Ok(())
}

fn secret_json(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(values) => values
            .iter()
            .any(|(k, v)| credential_name(k) || secret_json(v)),
        serde_json::Value::Array(values) => values.iter().any(secret_json),
        _ => false,
    }
}

fn validate_body(body: &str, mode: Option<bool>) -> Result<(), CurlProblem> {
    if body.contains("{{") {
        return Err(CurlProblem::Template);
    }
    let json = serde_json::from_str::<serde_json::Value>(body);
    if mode == Some(true) && json.is_err() {
        return Err(CurlProblem::Json);
    }
    if let Ok(json) = json {
        if secret_json(&json) {
            return Err(CurlProblem::BodyCredentials);
        }
    } else if mode == Some(false)
        && url::form_urlencoded::parse(body.as_bytes()).any(|(k, _)| credential_name(&k))
    {
        return Err(CurlProblem::BodyCredentials);
    }
    Ok(())
}

fn header_ok(name: &str, value: &str) -> bool {
    http::HeaderName::from_bytes(name.as_bytes()).is_ok()
        && http::HeaderValue::from_str(value).is_ok()
        && !value.is_empty()
}

/// Converts one pasted cURL command.
pub fn parse(input: &str) -> Result<Conversion, CurlProblem> {
    let a = arguments(&words(input)?)?;
    if a.head && (a.body_mode.is_some() || a.method.as_deref().is_some_and(|m| m != "HEAD")) {
        return Err(CurlProblem::Combination);
    }
    let method = a.method.clone().unwrap_or_else(|| {
        if a.head {
            "HEAD"
        } else if a.body_mode.is_some() {
            "POST"
        } else {
            "GET"
        }
        .into()
    });
    if http::Method::from_bytes(method.as_bytes()).is_err()
        || !method.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
    {
        return Err(CurlProblem::Combination);
    }
    let url = a.url.clone().ok_or(CurlProblem::Url)?;
    validate_url(&url, a.globoff)?;
    validate_body(&a.body, a.body_mode)?;
    let mut notes = a.notes;
    let mut settings = DocumentMut::new();
    if let Some((kind, username)) = &a.auth {
        let reference = reference()?;
        settings["auth"] = Item::Table(Table::new());
        settings["auth"]["kind"] = value(*kind);
        settings["auth"]["secret"] = value(&reference);
        if *kind == "basic" {
            settings["auth"]["username"] = value(username.as_str());
        }
        notes.push(ImportNote::AuthRemoved { reference });
    }
    let mut headers = a.headers;
    if let Some(agent) = a.user_agent
        && !headers.iter().any(|h| {
            h.split_once(':')
                .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case("User-Agent"))
        })
    {
        headers.push(format!("User-Agent: {agent}"));
    }
    if let Some(cookie) = a.cookie {
        if headers.iter().any(|h| {
            h.split_once(':')
                .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case("Cookie"))
        }) {
            return Err(CurlProblem::Combination);
        }
        if cookie.is_empty() {
            notes.push(ImportNote::EmptyCookie);
        } else {
            headers.push(format!("Cookie: {cookie}"));
        }
    }
    let mut rows = Vec::new();
    for header in headers {
        if header.contains(['\r', '\n']) {
            return Err(CurlProblem::Header);
        }
        let (name, contents) = header.split_once(':').ok_or(CurlProblem::Header)?;
        let (name, contents) = (name.trim(), contents.trim());
        if contents.contains("{{") {
            return Err(CurlProblem::Template);
        }
        if !header_ok(name, contents) {
            return Err(CurlProblem::Header);
        }
        if settings.get("auth").is_some() && name.eq_ignore_ascii_case("Authorization") {
            return Err(CurlProblem::Combination);
        }
        if credential_name(name) {
            let reference = reference()?;
            let variable = format!("import_header_{}", rows.len());
            let mut entry = InlineTable::new();
            entry.insert("secret", reference.clone().into());
            if settings.get("variables").is_none() {
                settings["variables"] = Item::Table(Table::new());
            }
            settings["variables"][&variable] = value(entry);
            rows.push(format!("{name}: {{{{{variable}}}}}"));
            notes.push(ImportNote::HeaderReplaced {
                variable,
                reference,
            });
        } else {
            rows.push(format!("{name}: {contents}"));
        }
    }
    if let Some(json) = a.body_mode {
        if !rows
            .iter()
            .any(|s| s.to_ascii_lowercase().starts_with("content-type:"))
        {
            rows.push(format!(
                "Content-Type: {}",
                if json {
                    "application/json"
                } else {
                    "application/x-www-form-urlencoded"
                }
            ));
        }
        if json
            && !rows
                .iter()
                .any(|s| s.to_ascii_lowercase().starts_with("accept:"))
        {
            rows.push("Accept: application/json".into());
        }
    }
    if a.compressed {
        rows.retain(|r| !r.to_ascii_lowercase().starts_with("accept-encoding:"));
        rows.push("Accept-Encoding: identity".into());
        notes.push(ImportNote::Compressed);
    }
    Ok(Conversion {
        fields: Fields {
            method,
            url,
            headers: rows.join("\n"),
            query: String::new(),
            body: a.body,
            body_kind: if a.body_mode == Some(true) {
                "json"
            } else {
                "text"
            }
            .into(),
            settings: if settings.is_empty() {
                String::new()
            } else {
                settings.to_string()
            },
        },
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_a_literal_command_and_removes_credentials() {
        let c = parse(
            "curl -sS https://api.example.com/v1/items \\\n  -H 'X-Api-Key: abc123' -H \"Accept: text/plain\" --json '{\"q\":1}' -u alice:pw --compressed",
        )
        .expect("fixture");
        assert_eq!(c.fields.method, "POST");
        assert_eq!(c.fields.body_kind, "json");
        assert!(!format!("{c:?}").contains("abc123"));
        assert!(!format!("{c:?}").contains(":pw"));
        assert!(c.fields.headers.contains("X-Api-Key: {{import_header_0}}"));
        assert!(c.fields.headers.ends_with("Accept-Encoding: identity"));
        assert!(c.fields.settings.contains("kind = \"basic\""));
        assert!(c.fields.settings.contains("username = \"alice\""));
        assert!(super::super::document::check(&c.fields).is_ok());
        assert!(
            c.notes
                .iter()
                .any(|n| matches!(n, ImportNote::AuthRemoved { .. }))
        );
    }

    #[test]
    fn refuses_what_it_cannot_import_faithfully() {
        for (input, problem) in [
            ("wget https://a", CurlProblem::NotCurl),
            ("curl https://a; rm -rf x", CurlProblem::Shell),
            ("curl \"$(id)\"", CurlProblem::Shell),
            ("curl 'https://a", CurlProblem::Unclosed),
            ("curl --proxy x https://a", CurlProblem::UnsupportedOption),
            ("curl -d @file https://a", CurlProblem::External),
            ("curl https://u:p@a/", CurlProblem::UrlCredentials),
            ("curl 'https://a/?api_key=1'", CurlProblem::QueryCredentials),
            (
                "curl --json '{\"password\":1}' https://a",
                CurlProblem::BodyCredentials,
            ),
            ("curl -d 'token=1' https://a", CurlProblem::BodyCredentials),
            ("curl --json '{bad' https://a", CurlProblem::Json),
            ("curl 'https://{{host}}/'", CurlProblem::Template),
            ("curl ftp://a/", CurlProblem::Url),
            ("curl https://a https://b", CurlProblem::Url),
            ("curl -I -d x https://a", CurlProblem::Combination),
        ] {
            assert_eq!(parse(input).map(|_| ()), Err(problem), "{input}");
        }
        assert_eq!(
            parse(&"x".repeat(CURL_BYTES + 1)).map(|_| ()),
            Err(CurlProblem::TooLarge)
        );
    }

    #[test]
    fn ansi_quotes_and_continuations_lex_literally() {
        assert_eq!(
            words("curl $'a\\tb' \\\n 'c d'").expect("fixture"),
            ["curl", "a\tb", "c d"]
        );
    }
}
