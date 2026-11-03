//! Minimum evidence contracts for the interactive Academy qualification rows.
//! Reject weak plans before dispatch; private manifests choose test data, not
//! whether a boolean shape is enough to prove a destructive reset worked.

use serde_json::{Value, json};

use super::Call;

fn input(call: &Call) -> Value {
    call.input
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null)
}

fn reference(call: &Call, pointer: &str, label: &str, source: &str) -> bool {
    input(call).pointer(pointer) == Some(&json!({"$call": label, "pointer": source}))
}

fn equals(call: &Call, pointer: &str, value: Value) -> bool {
    call.expect.iter().any(|e| e.equals(pointer, &value))
}

fn kind(call: &Call, pointer: &str, kind: &str) -> bool {
    call.expect.iter().any(|e| e.kind(pointer, kind))
}

fn same(call: &Call, pointer: &str, label: &str, source: &str) -> bool {
    call.expect.iter().any(|e| e.same(pointer, label, source))
}

/// Only an explicitly scoped hand-off may proceed after a failed checkpoint.
/// The normal reference, account and budget checks still apply.
pub(super) fn cleanup(call: &Call) -> bool {
    call.procedure == "shift.decide" && input(call)["json"]["choice"] == "hand_off"
}

pub(super) fn validate(calls: &[Call]) -> Result<(), String> {
    for call in calls {
        let required = match call.procedure.as_str() {
            "progress.resetCourse" => Some("W66"),
            "exam.start" => Some("W53"),
            "shift.start" | "shift.serve" | "shift.answer" | "shift.decide" => Some("W65"),
            _ => None,
        };
        if required.is_some_and(|workflow| !call.workflows.contains(&workflow)) {
            return Err(format!(
                "{}: the procedure requires its interactive workflow contract",
                call.label
            ));
        }
    }
    for workflow in ["W53", "W65", "W66"] {
        let row: Vec<&Call> = calls
            .iter()
            .filter(|c| c.workflows.contains(&workflow))
            .collect();
        if row.is_empty() {
            continue;
        }
        let unique: &[(&str, usize)] = match workflow {
            "W53" => &[("exam.start", 1), ("exam.submissionState", 1)],
            "W65" => &[
                ("shift.start", 1),
                ("shift.decide", 1),
                ("shift.debrief", 1),
            ],
            "W66" => &[
                ("progress.resetCourse", 1),
                ("progress.courseSummary", 2),
                ("certificate.mine", 2),
            ],
            _ => &[],
        };
        if unique.iter().any(|(procedure, count)| {
            row.iter().filter(|c| c.procedure == *procedure).count() > *count
        }) {
            return Err(format!(
                "{workflow}: duplicate critical checkpoints are not permitted"
            ));
        }
        let mut iter = row.iter().copied();
        let mut ordered = Vec::new();
        for procedure in super::workflow_steps(workflow) {
            ordered.push(iter.find(|c| c.procedure == *procedure).ok_or_else(|| {
                format!("{workflow}: the program lacks ordered interactive checkpoints")
            })?);
        }
        let valid = match workflow {
            "W53" => exam(&ordered),
            "W65" => shift(&row, &ordered),
            "W66" => reset(&ordered),
            _ => false,
        };
        if !valid {
            return Err(format!(
                "{workflow}: the program lacks required outcome assertions or identity references; see academy-2.1-follow-up.md"
            ));
        }
    }
    Ok(())
}

fn exam(steps: &[&Call]) -> bool {
    let (start, state, certificates) = (steps[0], steps[1], steps[2]);
    kind(start, "/attemptId", "nonempty-string")
        && kind(start, "/deadline", "nonempty-string")
        && kind(start, "/timeLimitSeconds", "positive-number")
        && reference(state, "/json/attemptId", &start.label, "/attemptId")
        && same(state, "/attemptId", &start.label, "/attemptId")
        && equals(state, "/status", json!("in-progress"))
        && (kind(certificates, "", "array") || kind(certificates, "", "nonempty-array"))
}

fn shift(row: &[&Call], steps: &[&Call]) -> bool {
    let start = steps[0];
    if input(start)["json"]["daily"] != true
        || !kind(start, "/shiftId", "nonempty-string")
        || !equals(start, "/alertsAnswered", json!(0))
        || !equals(start, "/atBoundary", json!(false))
    {
        return false;
    }
    let mut served = Vec::new();
    for call in row {
        if !call.procedure.starts_with("shift.") || call.procedure == "shift.start" {
            continue;
        }
        // These other read procedures do not take a shift ID.
        if [
            "shift.daily",
            "shift.dailyBoard",
            "shift.history",
            "shift.eligibility",
            "shift.getActive",
            "shift.dailyGhost",
            "shift.yesterday",
        ]
        .contains(&call.procedure.as_str())
        {
            continue;
        }
        if !reference(call, "/json/shiftId", &start.label, "/shiftId") {
            return false;
        }
        let valid = match call.procedure.as_str() {
            "shift.serve" => {
                served.push(*call);
                same(call, "/shift/shiftId", &start.label, "/shiftId")
                    && call
                        .expect
                        .iter()
                        .any(|e| e.checks_number("/shift/alertOrder"))
                    && equals(call, "/breachResolved", json!(false))
            }
            "shift.answer" => {
                served.last().is_some_and(|serve| {
                    reference(call, "/json/order", &serve.label, "/shift/alertOrder")
                }) && call.expect.iter().any(|e| e.graded_shift_outcome())
            }
            "shift.decide" => {
                cleanup(call)
                    && same(call, "/shiftId", &start.label, "/shiftId")
                    && equals(call, "/status", json!("handed_off"))
            }
            "shift.debrief" => {
                kind(call, "/themes", "nonempty-array") && kind(call, "/links", "array")
            }
            _ => true,
        };
        if !valid {
            return false;
        }
    }
    true
}

fn reset(steps: &[&Call]) -> bool {
    let (certs, before, reset, after, certs_after) =
        (steps[0], steps[1], steps[2], steps[3], steps[4]);
    let course = input(before)["json"]["courseId"].clone();
    if !course.as_str().is_some_and(|c| !c.is_empty())
        || input(reset)["json"]["courseId"] != course
        || input(after)["json"]["courseId"] != course
    {
        return false;
    }
    let counts = ["units", "quizzes", "labs", "objectives"];
    kind(certs, "", "nonempty-array")
        && equals(before, "/hasProgress", json!(true))
        && counts
            .iter()
            .any(|c| kind(before, &format!("/{c}"), "positive-number"))
        && counts.iter().all(|c| {
            let pointer = format!("/{c}");
            (kind(before, &pointer, "number") || kind(before, &pointer, "positive-number"))
                && same(reset, &format!("/cleared/{c}"), &before.label, &pointer)
                && equals(after, &pointer, json!(0))
        })
        && equals(after, "/hasProgress", json!(false))
        && same(certs_after, "", &certs.label, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live_academy::assertions::Expect;
    use desktop_core::learner::LEARNER_PROCEDURES;
    use std::collections::BTreeSet;

    fn call(
        label: &str,
        workflow: &str,
        procedure: &str,
        input: Value,
        expectations: &[&str],
    ) -> Call {
        let names = ["start", "certs", "before", "serve"]
            .map(str::to_owned)
            .into_iter()
            .collect::<BTreeSet<_>>();
        Call {
            label: label.to_owned(),
            workflows: match workflow {
                "W53" => &["W53"],
                "W65" => &["W65"],
                _ => &["W66"],
            },
            procedure: procedure.to_owned(),
            kind: LEARNER_PROCEDURES
                .iter()
                .find(|(p, _)| *p == procedure)
                .expect("procedure")
                .1,
            input: (!input.is_null()).then(|| input.to_string()),
            expect: expectations
                .iter()
                .map(|e| Expect::parse(e, &names).expect("assertion"))
                .collect(),
        }
    }

    fn exam_plan() -> Vec<Call> {
        vec![
            call(
                "start",
                "W53",
                "exam.start",
                json!({"json":{"consent":true}}),
                &[
                    "/attemptId:nonempty-string",
                    "/deadline:nonempty-string",
                    "/timeLimitSeconds:positive-number",
                ],
            ),
            call(
                "state",
                "W53",
                "exam.submissionState",
                json!({"json":{"attemptId":{"$call":"start","pointer":"/attemptId"}}}),
                &["/attemptId=@start/attemptId", "/status=\"in-progress\""],
            ),
            call("certs", "W53", "certificate.mine", Value::Null, &[":array"]),
        ]
    }

    fn shift_plan() -> Vec<Call> {
        let id = json!({"$call":"start","pointer":"/shiftId"});
        vec![
            call(
                "start",
                "W65",
                "shift.start",
                json!({"json":{"daily":true}}),
                &[
                    "/shiftId:nonempty-string",
                    "/alertsAnswered=0",
                    "/atBoundary=false",
                ],
            ),
            call(
                "serve",
                "W65",
                "shift.serve",
                json!({"json":{"shiftId":id}}),
                &[
                    "/shift/shiftId=@start/shiftId",
                    "/shift/alertOrder:number",
                    "/breachResolved=false",
                ],
            ),
            call(
                "answer",
                "W65",
                "shift.answer",
                json!({"json":{"shiftId":id,"order":{"$call":"serve","pointer":"/shift/alertOrder"},"values":["999999"]}}),
                &["/outcome in [\"correct\",\"incorrect\"]"],
            ),
            call(
                "decide",
                "W65",
                "shift.decide",
                json!({"json":{"shiftId":id,"choice":"hand_off"}}),
                &["/shiftId=@start/shiftId", "/status=\"handed_off\""],
            ),
            call(
                "debrief",
                "W65",
                "shift.debrief",
                json!({"json":{"shiftId":id}}),
                &["/themes:nonempty-array", "/links:array"],
            ),
        ]
    }

    fn reset_plan() -> Vec<Call> {
        let course = json!({"json":{"courseId":"test-course"}});
        vec![
            call(
                "certs",
                "W66",
                "certificate.mine",
                Value::Null,
                &[":nonempty-array"],
            ),
            call(
                "before",
                "W66",
                "progress.courseSummary",
                course.clone(),
                &[
                    "/hasProgress=true",
                    "/units:positive-number",
                    "/quizzes:number",
                    "/labs:number",
                    "/objectives:number",
                ],
            ),
            call(
                "reset",
                "W66",
                "progress.resetCourse",
                course.clone(),
                &[
                    "/cleared/units=@before/units",
                    "/cleared/quizzes=@before/quizzes",
                    "/cleared/labs=@before/labs",
                    "/cleared/objectives=@before/objectives",
                ],
            ),
            call(
                "after",
                "W66",
                "progress.courseSummary",
                course,
                &[
                    "/hasProgress=false",
                    "/units=0",
                    "/quizzes=0",
                    "/labs=0",
                    "/objectives=0",
                ],
            ),
            call(
                "after-certs",
                "W66",
                "certificate.mine",
                Value::Null,
                &["=@certs"],
            ),
        ]
    }

    #[test]
    fn every_mandatory_outcome_assertion_is_required_individually() {
        for factory in [exam_plan, shift_plan, reset_plan] {
            let mut plan = factory();
            assert!(validate(&plan).is_ok());
            for i in 0..plan.len() {
                for j in 0..plan[i].expect.len() {
                    let removed = plan[i].expect.remove(j);
                    assert!(
                        validate(&plan).is_err(),
                        "{} assertion {j}",
                        plan[i].procedure
                    );
                    plan[i].expect.insert(j, removed);
                }
            }
        }
    }

    #[test]
    fn identity_references_and_critical_occurrences_cannot_be_weakened() {
        for factory in [exam_plan, shift_plan, reset_plan] {
            let mut plan = factory();
            let critical = plan
                .iter()
                .position(|c| {
                    matches!(
                        c.procedure.as_str(),
                        "exam.start" | "shift.start" | "progress.resetCourse"
                    )
                })
                .expect("critical call");
            let workflow = plan[critical].workflows;
            plan[critical].workflows = &["W54"];
            assert!(validate(&plan).is_err());
            plan[critical].workflows = workflow;
            let mut duplicate = factory().remove(critical);
            duplicate.label = "duplicate".to_owned();
            plan.push(duplicate);
            assert!(validate(&plan).is_err());
        }
        let mut plan = shift_plan();
        for i in 1..plan.len() {
            let original = plan[i].input.take();
            plan[i].input = Some(json!({"json":{"shiftId":"unbound-id"}}).to_string());
            assert!(validate(&plan).is_err(), "{}", plan[i].procedure);
            plan[i].input = original;
        }
        let mut plan = reset_plan();
        plan[3].input = Some(json!({"json":{"courseId":"different-course"}}).to_string());
        assert!(validate(&plan).is_err());
        let mut plan = exam_plan();
        plan[1].input = Some(json!({"json":{"attemptId":"unbound-id"}}).to_string());
        assert!(validate(&plan).is_err());
        let mut plan = shift_plan();
        plan[0].input = Some(json!({"json":{"daily":false}}).to_string());
        assert!(validate(&plan).is_err());
        let mut plan = shift_plan();
        let mut changed = input(&plan[2]);
        changed["json"]["order"] = json!(999);
        plan[2].input = Some(changed.to_string());
        assert!(validate(&plan).is_err());
        let mut plan = reset_plan();
        plan[2].input = Some(json!({"json":{"courseId":"different-course"}}).to_string());
        assert!(validate(&plan).is_err());
    }
}
