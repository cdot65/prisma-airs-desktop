//! Assertions and data dependencies for an operator-designated Academy program.
//! Hosted values stay in memory; receipts expose scalar verdicts or fingerprints.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

#[derive(Debug)]
pub(super) enum Expect {
    Kind(String, String),
    Equals(String, Value),
    Same(String, String, String),
    OneOf(String, Vec<Value>),
}

fn pointer_valid(pointer: &str) -> bool {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return false;
    }
    let mut chars = pointer.chars();
    while let Some(c) = chars.next() {
        if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

fn reference_valid(label: &str, pointer: &str, earlier: &BTreeSet<String>) -> Result<(), String> {
    if !earlier.contains(label) {
        return Err("a reference must name an earlier call in the same family".to_owned());
    }
    if !pointer_valid(pointer) {
        return Err("invalid JSON pointer in call reference".to_owned());
    }
    Ok(())
}

impl Expect {
    pub fn parse(text: &str, earlier: &BTreeSet<String>) -> Result<Self, String> {
        let text = text.trim();
        let assertion = if let Some((ptr, rest)) = text.split_once(" in ") {
            let values: Vec<Value> = serde_json::from_str(rest)
                .map_err(|_| "membership expects a nonempty JSON array".to_owned())?;
            if values.is_empty() || values.iter().any(|v| !v.is_string()) {
                return Err("membership expects a nonempty array of strings".to_owned());
            }
            Self::OneOf(ptr.to_owned(), values)
        } else if let Some((ptr, rest)) = text.split_once('=') {
            if let Some(other) = rest.strip_prefix('@') {
                let (label, ptr2) = other
                    .find('/')
                    .map_or((other, ""), |i| (&other[..i], &other[i..]));
                reference_valid(label, ptr2, earlier)?;
                Self::Same(ptr.to_owned(), label.to_owned(), ptr2.to_owned())
            } else {
                Self::Equals(
                    ptr.to_owned(),
                    serde_json::from_str(rest)
                        .map_err(|_| "the expected value is not JSON".to_owned())?,
                )
            }
        } else {
            match text.rsplit_once(':') {
                Some((ptr, kind))
                    if [
                        "bool",
                        "array",
                        "number",
                        "object",
                        "string",
                        "nonempty-array",
                        "nonempty-string",
                        "positive-number",
                    ]
                    .contains(&kind) =>
                {
                    Self::Kind(ptr.to_owned(), kind.to_owned())
                }
                _ => return Err("not an expectation".to_owned()),
            }
        };
        if !pointer_valid(assertion.pointer()) {
            return Err("invalid JSON pointer in expectation".to_owned());
        }
        Ok(assertion)
    }

    fn pointer(&self) -> &str {
        match self {
            Self::Kind(p, _) | Self::Equals(p, _) | Self::Same(p, _, _) | Self::OneOf(p, _) => p,
        }
    }

    pub fn ready(&self, earlier: &BTreeMap<String, Value>) -> bool {
        match self {
            Self::Same(_, label, pointer) => earlier
                .get(label)
                .and_then(|v| v.pointer(pointer))
                .is_some(),
            _ => true,
        }
    }

    pub fn is_grading_verdict(&self) -> bool {
        matches!(self, Self::Equals(pointer, Value::Bool(_)) if pointer == "/correct")
    }

    pub fn equals(&self, pointer: &str, value: &Value) -> bool {
        matches!(self, Self::Equals(p, v) if p == pointer && v == value)
    }

    pub fn kind(&self, pointer: &str, kind: &str) -> bool {
        matches!(self, Self::Kind(p, k) if p == pointer && k == kind)
    }

    pub fn same(&self, pointer: &str, label: &str, earlier_pointer: &str) -> bool {
        matches!(self, Self::Same(p, l, ep) if p == pointer && l == label && ep == earlier_pointer)
    }

    pub fn graded_shift_outcome(&self) -> bool {
        match self {
            Self::Equals(p, v) => {
                p == "/outcome" && [json!("correct"), json!("incorrect")].contains(v)
            }
            Self::OneOf(p, values) => {
                p == "/outcome"
                    && values
                        .iter()
                        .all(|v| [json!("correct"), json!("incorrect")].contains(v))
            }
            _ => false,
        }
    }

    pub fn checks_number(&self, pointer: &str) -> bool {
        self.kind(pointer, "number")
            || self.kind(pointer, "positive-number")
            || matches!(self, Self::Equals(p, Value::Number(_)) if p == pointer)
    }

    /// Private strings/containers are summarized without reversible fingerprints.
    pub fn observe(&self, answer: &Value, earlier: &BTreeMap<String, Value>) -> Value {
        let actual = answer.pointer(self.pointer());
        let (passed, expected) = match self {
            Self::Kind(_, kind) => (
                actual.is_some_and(|v| match kind.as_str() {
                    "bool" => v.is_boolean(),
                    "array" => v.is_array(),
                    "number" => v.is_number(),
                    "object" => v.is_object(),
                    "string" => v.is_string(),
                    "nonempty-array" => v.as_array().is_some_and(|a| !a.is_empty()),
                    "nonempty-string" => v.as_str().is_some_and(|s| !s.is_empty()),
                    "positive-number" => v.as_f64().is_some_and(|n| n > 0.0),
                    _ => false,
                }),
                json!({"kind": kind}),
            ),
            Self::Equals(_, want) => (actual == Some(want), shown_value(Some(want))),
            Self::Same(_, label, pointer) => {
                let want = earlier.get(label).and_then(|v| v.pointer(pointer));
                (
                    actual.is_some() && actual == want,
                    json!({"call": label, "pointer": pointer, "value": shown_value(want)}),
                )
            }
            Self::OneOf(_, values) => (
                actual.is_some_and(|v| values.contains(v)),
                json!({"one_of": values.iter().map(|v| shown_value(Some(v))).collect::<Vec<_>>()}),
            ),
        };
        json!({"pointer": self.pointer(), "passed": passed,
            "actual": shown_value(actual), "expected": expected})
    }
}

fn input_reference(value: &serde_json::Map<String, Value>) -> Result<Option<(&str, &str)>, String> {
    if !value.contains_key("$call") {
        return Ok(None);
    }
    if value.len() != 2 {
        return Err("a call reference contains exactly $call and pointer".to_owned());
    }
    match (
        value["$call"].as_str(),
        value.get("pointer").and_then(Value::as_str),
    ) {
        (Some(label), Some(pointer)) => Ok(Some((label, pointer))),
        _ => Err("a call reference needs string $call and pointer fields".to_owned()),
    }
}

pub(super) fn validate_input(value: &Value, earlier: &BTreeSet<String>) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if let Some((label, pointer)) = input_reference(map)? {
                return reference_valid(label, pointer, earlier);
            }
            for v in map.values() {
                validate_input(v, earlier)?;
            }
        }
        Value::Array(values) => {
            for v in values {
                validate_input(v, earlier)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Exact JSON objects are references; strings are never interpolated or evaluated.
pub(super) fn resolve_input(
    value: Value,
    earlier: &BTreeMap<String, Value>,
) -> Result<Value, String> {
    Ok(match value {
        Value::Object(map) => {
            if let Some((label, pointer)) = input_reference(&map)? {
                return earlier
                    .get(label)
                    .and_then(|v| v.pointer(pointer))
                    .cloned()
                    .ok_or_else(|| {
                        "an earlier validated result is unavailable; call not sent".to_owned()
                    });
            }
            Value::Object(
                map.into_iter()
                    .map(|(k, v)| Ok((k, resolve_input(v, earlier)?)))
                    .collect::<Result<_, String>>()?,
            )
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|v| resolve_input(v, earlier))
                .collect::<Result<_, _>>()?,
        ),
        v => v,
    })
}

/// The transport's SuperJSON envelope must really contain an answer. Missing
/// `json` and malformed JSON must never be converted into an asserted null.
pub(super) fn answer(result: &Value) -> Option<Value> {
    let data: Value = serde_json::from_str(result["data"].as_str()?).ok()?;
    data.get("json").cloned()
}

fn shown_value(value: Option<&Value>) -> Value {
    match value {
        None => json!({"present": false}),
        Some(v) if v.is_boolean() || v.is_number() || v.is_null() => {
            json!({"present": true, "value": v})
        }
        Some(v) => {
            let text = v.to_string();
            json!({"present": true, "bytes": text.len(),
                "array_len": v.as_array().map(Vec::len)})
        }
    }
}

/// Preserve the operator's references, but never publish authored answers.
pub(super) fn receipt_input(input: &Option<String>) -> Value {
    let Some(input) = input else {
        return Value::Null;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(input) else {
        return Value::Null;
    };
    for pointer in ["/json/answer", "/json/values"] {
        if let Some(answer) = value.pointer_mut(pointer) {
            *answer = json!({"redacted":true});
        }
    }
    json!(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointers_and_dependencies_are_validated_before_dispatch() {
        let earlier = BTreeSet::from(["01-before".to_owned()]);
        for text in [
            "/x=@future",
            "/x=@01-before/bad~",
            "bad=true",
            "/bad~2=true",
            ":unknown",
        ] {
            assert!(Expect::parse(text, &earlier).is_err(), "{text}");
        }
        assert!(Expect::parse("/a~1b~0c=@01-before", &earlier).is_ok());
        for input in [
            json!({"$call":"future", "pointer":""}),
            json!({"$call":"01-before"}),
            json!({"$call":"01-before", "pointer":"", "extra":true}),
        ] {
            assert!(validate_input(&input, &earlier).is_err());
        }
    }

    #[test]
    fn equality_does_not_confuse_missing_null_or_boolean_shape_with_a_verdict() {
        let earlier = BTreeSet::new();
        let empty = BTreeMap::new();
        let correct =
            Expect::parse("/correct=true", &earlier).expect("valid fixture assertion or reference");
        for response in [
            json!({}),
            json!({"correct":false}),
            json!({"correct":"true"}),
        ] {
            assert_eq!(correct.observe(&response, &empty)["passed"], false);
        }
        assert_eq!(
            correct.observe(&json!({"correct":true}), &empty)["passed"],
            true
        );
        let null =
            Expect::parse("/value=null", &earlier).expect("valid fixture assertion or reference");
        assert_eq!(null.observe(&json!({}), &empty)["passed"], false);
        assert_eq!(null.observe(&json!({"value":null}), &empty)["passed"], true);
        assert!(answer(&json!({"data":"{}"})).is_none());
        assert!(answer(&json!({"data":"not JSON"})).is_none());
        assert_eq!(
            answer(&json!({"data":"{\"json\":null}"})),
            Some(Value::Null)
        );
    }

    #[test]
    fn dependencies_preserve_json_types_and_values_never_enter_receipts() {
        let names = BTreeSet::from(["01-start".to_owned()]);
        let earlier = BTreeMap::from([(
            "01-start".to_owned(),
            json!({"id":"private-id", "order":2, "certs":[{"name":"Private Learner"}]}),
        )]);
        let input = json!({"json":{"id":{"$call":"01-start", "pointer":"/id"}, "order":{"$call":"01-start", "pointer":"/order"}}});
        validate_input(&input, &names).expect("valid fixture assertion or reference");
        assert_eq!(
            resolve_input(input, &earlier).expect("valid fixture assertion or reference"),
            json!({"json":{"id":"private-id", "order":2}})
        );
        let expect = Expect::parse("/certs=@01-start/certs", &names)
            .expect("valid fixture assertion or reference");
        let observed = expect.observe(&earlier["01-start"], &earlier);
        assert_eq!(observed["passed"], true);
        assert!(!observed.to_string().contains("Private Learner"));
        assert!(
            !Expect::parse("/certs=@01-start/certs", &names)
                .expect("valid fixture assertion or reference")
                .ready(&BTreeMap::new())
        );
        assert!(
            resolve_input(json!({"$call":"01-start", "pointer":"/missing"}), &earlier).is_err()
        );
        assert_eq!(
            Expect::parse(":nonempty-array", &names)
                .expect("valid fixture assertion or reference")
                .observe(&json!([]), &earlier)["passed"],
            false
        );
    }

    #[test]
    fn membership_accepts_only_the_designated_graded_outcomes() {
        let expected = Expect::parse("/outcome in [\"correct\",\"incorrect\"]", &BTreeSet::new())
            .expect("membership");
        for (value, passes) in [
            ("correct", true),
            ("incorrect", true),
            ("ungraded", false),
            ("breached", false),
        ] {
            assert_eq!(
                expected.observe(&json!({"outcome":value}), &BTreeMap::new())["passed"],
                passes
            );
        }
        for invalid in ["/outcome in []", "/outcome in [true]", "/outcome in null"] {
            assert!(Expect::parse(invalid, &BTreeSet::new()).is_err());
        }
        let private =
            Expect::parse("/name=\"Private Learner\"", &BTreeSet::new()).expect("literal");
        let receipt = private.observe(&json!({"name":"Private Learner"}), &BTreeMap::new());
        assert!(!receipt.to_string().contains("Private Learner"));
        assert!(!receipt.to_string().contains("sha256"));
    }

    #[test]
    fn receipt_templates_and_membership_never_publish_authored_answers() {
        let input=Some(json!({"json":{"questionId":"q-1","answer":"PRIVATE-ANSWER","values":["PRIVATE-CHOICE"]}}).to_string());
        let recorded = receipt_input(&input).to_string();
        assert!(!recorded.contains("PRIVATE-ANSWER"));
        assert!(!recorded.contains("PRIVATE-CHOICE"));
        assert!(recorded.contains("questionId"));
        let expect =
            Expect::parse("/name in [\"PRIVATE-NAME\"]", &BTreeSet::new()).expect("membership");
        let recorded = expect
            .observe(&json!({"name":"PRIVATE-NAME"}), &BTreeMap::new())
            .to_string();
        assert!(!recorded.contains("PRIVATE-NAME"));
    }
}
