"""Deliberate behavior mutations for C13's Red Team and Model/skill
observation: v1's tallies, breakdowns and no-scans confirmation, list
completeness, privacy of rows (no names, URIs, findings or prompts),
per-read skill pseudonyms, attack paging and outcomes, admission bounds,
the product clients' tenant header and endpoints, and the presentation and
contract rules (lower bounds, unknown sums, superseded replies, sourced
states).

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 -I -B probes.py <copy-root> <results.json> [name filter]
Each probe applies one mutation that violates a guarded invariant, runs the
guarding target, requires it to fail on the named test, and restores the
files. Unmutated baselines of every target must pass three runs, with their
output kept.
"""
import hashlib
import json
import os
import re
import subprocess
import sys

REPO = sys.argv[1]
HOME = os.environ["HOME"]
ENV = {"HOME": HOME, "PATH": f"{REPO}/.tools/bin:{HOME}/.cargo/bin:{HOME}/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin",
       "RUSTUP_TOOLCHAIN": "1.99.0", "CI": "1", "NO_COLOR": "1", "CARGO_TERM_COLOR": "never",
       "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0", "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
READS_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "redteam_model_reads"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "red_team"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_VIEW = vitest("src/features/red-team/red-team-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (READS_T, UNIT_T, FE_VIEW, FE_SNAPSHOT)

RT = "crates/desktop-core/src/red_team.rs"
MGMT = "crates/desktop-core/src/management.rs"
VIEW = "apps/desktop/src/features/red-team/red-team-view.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

PROBES = [
    ("a missing total is zero despite a busy breakdown",
     [(RT, "        None => quiet.then_some(0),\n    };\n    Some(RedTeamStatistics {", "        None => Some(0),\n    };\n    Some(RedTeamStatistics {")],
     UNIT_T, "statistics_follow_v1s_tallies_and_bars"),
    ("an unreported breakdown is empty",
     [(RT, "        None | Some(Value::Null) => return Ok((None, false)),", "        None | Some(Value::Null) => return Ok((Some(Vec::new()), false)),")],
     UNIT_T, "statistics_follow_v1s_tallies_and_bars"),
    ("unknown labels pass through",
     [(RT, "    } else {\n        \"OTHER\".to_owned()\n    }", "    } else {\n        upper.replace('_', \" \")\n    }")],
     UNIT_T, "known_labels_are_shown_in_words_and_any_other_is_other"),
    ("absent statistics are zero without confirmation",
     [(MGMT, "                Ok(reply) if rt::proves_no_scans(&reply) => Ok(rt::no_scans()),", "                Ok(_) => Ok(rt::no_scans()),")],
     READS_T, "absent_statistics_mean_no_scans_only_when_an_empty_list_proves_it"),
    ("a list without a total is always complete",
     [(RT, "            None => rows.len() < LIST_LIMIT as usize,", "            None => true,")],
     UNIT_T, "lists_are_complete_only_when_the_service_says_so"),
    ("a capped count is complete",
     [(RT, "    let complete = !capped\n        && match total {", "    let complete = match total {")],
     UNIT_T, "lists_are_complete_only_when_the_service_says_so"),
    ("a scan row keeps its name",
     [(RT, "                job_type: token(row, \"job_type\"),", "                job_type: token(row, \"name\"),")],
     READS_T, "red_teaming_reads_statistics_and_scans_for_a_window"),
    ("a model row keeps its URI",
     [(RT, "                outcome: token(row, \"eval_outcome\"),\n                created_at: token(row, \"created_at\"),\n            })\n        })\n        .collect::<Option<Vec<_>>>()?;\n    Some(ModelScans {",
       "                outcome: token(row, \"model_uri\").or_else(|| token(row, \"eval_outcome\")),\n                created_at: token(row, \"created_at\"),\n            })\n        })\n        .collect::<Option<Vec<_>>>()?;\n    Some(ModelScans {")],
     UNIT_T, "rows_keep_no_names_uris_or_findings"),
    ("a skill is shown by name",
     [(RT, "                pseudonym: pseudonym(salt, name),", "                pseudonym: name.to_owned(),")],
     UNIT_T, "rows_keep_no_names_uris_or_findings"),
    ("the pseudonym salt is fixed",
     [(MGMT, "    let salt = rt::salt().ok_or(PageError::Invalid)?;", "    let salt = [0u8; 16];")],
     READS_T, "models_and_skills_read_three_sources_with_per_read_pseudonyms"),
    ("an attack row keeps its prompt",
     [(RT, "                sub_category: token(row, \"sub_category_display_name\")", "                sub_category: token(row, \"prompt\")\n                    .or_else(|| token(row, \"sub_category_display_name\"))")],
     READS_T, "attack_results_are_read_100_per_page_up_to_ten_pages_without_prompts"),
    ("an unrated attack counts as succeeded",
     [(RT, "                None | Some(Value::Null) => None,\n                Some(v) => Some(v.as_bool()?),\n            };\n            let multi_turn", "                None | Some(Value::Null) => Some(true),\n                Some(v) => Some(v.as_bool()?),\n            };\n            let multi_turn")],
     UNIT_T, "attack_outcomes_are_read_not_inferred"),
    ("attack pages skip the wrong rows",
     [(MGMT, "(u32::from(*page) * crate::red_team::LIST_LIMIT).to_string()", "u32::from(*page).to_string()")],
     READS_T, "attack_results_are_read_100_per_page_up_to_ten_pages_without_prompts"),
    ("an eleventh attack page is admitted",
     [(MGMT, "                    && *page < crate::red_team::ATTACK_PAGES", "                    && *page <= crate::red_team::ATTACK_PAGES")],
     READS_T, "attack_results_are_read_100_per_page_up_to_ten_pages_without_prompts"),
    ("any window is admitted",
     [(MGMT, "            Self::RedTeam { window } => crate::red_team::WINDOWS.contains(window),", "            Self::RedTeam { window } => *window > 0,")],
     READS_T, "red_teaming_reads_statistics_and_scans_for_a_window"),
    ("a read with no answering source succeeds",
     [(MGMT, "        (Err(error), Err(_)) => return Err(error),\n        both => both,", "        both => both,")],
     READS_T, "each_source_keeps_its_own_state_and_a_read_with_none_fails"),
    # The tenant header is fixed by each SDK client's constructor (on for
    # AgentGuard, off for Model Security), so no edit here can change it;
    # what this module chooses is the plane each read goes to.
    ("Model Security scans are read from the AgentGuard plane",
     [(MGMT, "        .product_read(\n            Product::ModelSecurity,\n", "        .product_read(\n            Product::AgentGuard,\n")],
     READS_T, "models_and_skills_read_three_sources_with_per_read_pseudonyms"),
    ("model scans are read as a background refresh",
     [(MGMT, "    let background = [(\"isBackgroundRefresh\", \"false\")];", "    let background = [(\"isBackgroundRefresh\", \"true\")];")],
     READS_T, "models_and_skills_read_three_sources_with_per_read_pseudonyms"),
    ("an unreported status breakdown sums to zero",
     [(VIEW, "    if (bars === null) return scans === 0 ? 0 : null", "    if (bars === null) return 0")],
     FE_VIEW, "treats an empty tenant as no scans and an unreported breakdown as zero there"),
    ("a partial sample is shown as exact",
     [(VIEW, "const bound = (n: number, complete: boolean) => (complete ? n.toLocaleString() : `≥ ${n.toLocaleString()}`)", "const bound = (n: number, _complete: boolean) => n.toLocaleString()")],
     FE_VIEW, "reads models and skills with partial samples as lower bounds and skills by pseudonym"),
    ("a superseded window's reply is applied",
     [(VIEW, "        if (mine !== tokens.current.redTeam) return\n", "")],
     FE_VIEW, "drops a superseded window’s late reply"),
    ("a failed refresh clears the last read",
     [(VIEW, "        else setRedTeam((h) => ({ ...h, loading: false, failure: typeof result === 'string' ? result : resultFailure(result) }))",
       "        else setRedTeam({ read: null, loading: false, failure: typeof result === 'string' ? result : resultFailure(result) })")],
     FE_VIEW, "states failed and unreported sources instead of zeros and keeps the last read"),
    ("attack results are read past the cap",
     [(VIEW, "            for (let page = 0; page < ATTACK_PAGES && open; page++) {", "            for (let page = 0; page <= ATTACK_PAGES && open; page++) {")],
     FE_VIEW, "reads attack results to ten pages on opening and counts only reported successes"),
    ("attack results are read past a covered total",
     [(VIEW, "                if (result.attacks.complete || result.attacks.rows.length < 100) break\n", "")],
     FE_VIEW, "stops reading attack results once the total is covered or a read fails"),
    ("every attack read is rendered at once",
     [(VIEW, "                        {rows.slice(shown * 100, shown * 100 + 100).map((a) => (", "                        {rows.map((a) => (")],
     FE_VIEW, "reads attack results to ten pages on opening and counts only reported successes"),
    ("unfinished scans offer attack results",
     [(VIEW, "                                                {REPORTED.includes((r.status ?? '').toUpperCase()) ? (", "                                                {true ? (")],
     FE_VIEW, "reads Red Teaming on opening and shows v1’s tiles, lists and coverage"),
    ("a source may carry data and a failure",
     [(CLIENT, "    optionalFailure(failure) && (data === null ? failure !== null : failure === null && valid(data))", "    optionalFailure(failure) && (data === null ? failure !== null : valid(data))")],
     FE_SNAPSHOT, "rejects a source with both data and a failure, or with neither"),
    # P04 push: the C13 review's R1-R3 (statistics envelopes and quiet
    # sources, coverage from rows received, capped totals).
    ("statistics accept a failure envelope",
     [(RT, "    !reports_failure(reply)\n        && reply", "    reply")],
     UNIT_T, "statistics_need_a_recognised_error_free_reply"),
    ("statistics need no known field",
     [(RT, ".is_some_and(|fields| known.iter().any(|k| fields.contains_key(*k)))", ".is_some_and(|_| true)")],
     UNIT_T, "statistics_need_a_recognised_error_free_reply"),
    ("a skill group may be a bare number",
     [(RT, "            Some(group) if group.is_object() => optional_count(group, \"count\"),\n            Some(_) => Err(()),", "            Some(group) => optional_count(group, \"count\"),")],
     UNIT_T, "statistics_need_a_recognised_error_free_reply"),
    ("a positive reported total leaves its partner zero",
     [(RT, "        && totals.iter().all(|n| n.unwrap_or(0) == 0);", ";")],
     UNIT_T, "missing_totals_are_zero_only_when_every_count_is_zero"),
    ("a positive skill count leaves its partner zero",
     [(RT, "        && counts.iter().all(|n| n.unwrap_or(0) == 0);", ";")],
     UNIT_T, "missing_totals_are_zero_only_when_every_count_is_zero"),
    ("a breakdown may exceed 64 entries",
     [(RT, "        Some(Value::Array(entries)) if entries.len() <= MAX_BARS => entries,", "        Some(Value::Array(entries)) => entries,")],
     UNIT_T, "missing_totals_are_zero_only_when_every_count_is_zero"),
    ("invalid statistics fail through the suite",
     [(RT, "    let quiet = !(busy_status || busy_risk || busy_types || rated)\n        && totals", "    let quiet = rated || true\n        && totals")],
     READS_T, "invalid_statistics_fail_alone_and_the_other_sources_stay_usable"),
    ("a total below the page's rows is accepted",
     [(RT, "    if total.is_some_and(|t| (t as usize) < rows.len()) {\n        return None;\n    }\n", "")],
     UNIT_T, "list_pages_follow_v1s_shape_rules"),
    ("a capped count is dropped",
     [(RT, "        Some(v) => v.as_bool()?,\n    };", "        Some(v) => v.as_bool().map(|_| false)?,\n    };")],
     READS_T, "attack_coverage_counts_rows_received_and_capped_totals_are_bounds"),
    ("attack coverage counts offsets",
     [(MGMT, "crate::red_team::attacks(&reply, scan_id, *page, *received)", "crate::red_team::attacks(&reply, scan_id, *page, u32::from(*page) * crate::red_team::LIST_LIMIT)")],
     READS_T, "attack_coverage_counts_rows_received_and_capped_totals_are_bounds"),
    ("more rows than the earlier pages hold are admitted",
     [(MGMT, "                    && *received <= u32::from(*page) * crate::red_team::LIST_LIMIT\n", "")],
     READS_T, "attack_results_are_read_100_per_page_up_to_ten_pages_without_prompts"),
    ("the attack walk continues past a short page",
     [(VIEW, "if (result.attacks.complete || result.attacks.rows.length < 100) break", "if (result.attacks.complete) break")],
     FE_VIEW, 'stops on a short page and never calls an unread remainder "no attacks"'),
    ("an incomplete empty read says no attacks",
     [(VIEW, "            {!loading && last?.complete && rows.length === 0 ? (", "            {!loading && last && rows.length === 0 ? (")],
     FE_VIEW, 'stops on a short page and never calls an unread remainder "no attacks"'),
    ("attack pages do not carry the rows received",
     [(VIEW, "                received += result.attacks.rows.length\n", "")],
     FE_VIEW, "stops reading attack results once the total is covered or a read fails"),
    ("a capped total is shown as exact",
     [(VIEW, "const reported = (total: number, capped: boolean) => `${capped ? '≥ ' : ''}${total.toLocaleString()}`", "const reported = (total: number, _capped: boolean) => total.toLocaleString()")],
     FE_VIEW, "shows a capped total as a lower bound"),
    ("a capped tile is shown as exact",
     [(VIEW, "(list.complete || list.total !== null) && !list.capped)", "list.complete || list.total !== null)")],
     FE_VIEW, "shows a capped total as a lower bound"),
    ("a capped total may be complete",
     [(CLIENT, "        !(value.capped && value.complete)", "        true")],
     FE_SNAPSHOT, "rejects a total without its exactness, or a capped total called complete"),
    # P04 push, attempt-02: nested rated targets (R1) and pagination (R4).
    ("rated target counts are not validated",
     [(RT, "            rated |= target.get(\"count\").and_then(count).ok_or(())? > 0;", "            rated |= target.get(\"count\").and_then(count).is_some_and(|n| n > 0);")],
     UNIT_T, "rated_targets_are_validated_before_a_source_counts_as_quiet"),
    ("a malformed targets group is skipped",
     [(RT, "            Some(Value::Array(targets)) if targets.len() <= MAX_BARS => targets,\n            Some(_) => return Err(()),", "            Some(Value::Array(targets)) if targets.len() <= MAX_BARS => targets,\n            Some(_) => continue,")],
     READS_T, "invalid_statistics_fail_alone_and_the_other_sources_stay_usable"),
    ("malformed pagination is accepted",
     [(RT, "    let pagination = reply.get(\"pagination\")?.as_object()?;\n    let total = match pagination.get(\"total_items\") {", "    let total = match reply.get(\"pagination\").and_then(|p| p.get(\"total_items\")) {")],
     READS_T, "absent_statistics_mean_no_scans_only_when_an_empty_list_proves_it"),
    ("a null count_capped is read as false",
     [(RT, "        None => false,\n        Some(v) => v.as_bool()?,", "        None | Some(Value::Null) => false,\n        Some(v) => v.as_bool()?,")],
     UNIT_T, "list_pages_follow_v1s_shape_rules"),
    ("a skill row may carry a name",
     [(CLIENT, "    /^[0-9a-f]{8}$/.test(r.pseudonym) &&", "    r.pseudonym.length > 0 &&")],
     FE_SNAPSHOT, "rejects windows, identifiers, pages and pseudonyms outside v1’s reads"),
]


def run(cmd):
    try:
        p = subprocess.run(cmd, cwd=REPO, env=ENV, capture_output=True, text=True, timeout=900)
    except subprocess.TimeoutExpired:
        return "timeout", ""
    return p.returncode, p.stdout + p.stderr


def failed_on(test, out):
    """True only when the named test itself reports a failure (a Rust
    `test ... FAILED` line or a Vitest failure line naming it), so a build
    error or another test's failure never counts as the guard catching
    the mutation."""
    name = re.escape(test)
    return bool(re.search(rf"^test (\S+::)?{name} \.\.\. FAILED$", out, re.M)
                or re.search(rf"^.*(×|FAIL).*(> |× ){name}(\s+\d+ms)?\s*$", out, re.M))


def apply(edits):
    """Checks every anchor first, then writes; a bad anchor changes nothing."""
    texts = {}
    for path, old, _ in edits:
        full = os.path.join(REPO, path)
        texts.setdefault(full, open(full).read())
    pending = dict(texts)
    for path, old, new in edits:
        full = os.path.join(REPO, path)
        assert pending[full].count(old) == 1, (path, old)
        pending[full] = pending[full].replace(old, new)
    for full, text in pending.items():
        with open(full, "w") as f:
            f.write(text)
    return texts


def restore(saved):
    for full, text in saved.items():
        with open(full, "w") as f:
            f.write(text)
        assert digest(full) == INPUTS[os.path.relpath(full, REPO)], full


def digest(full):
    with open(full, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


# The files the probes mutate, as found in the copy: their hashes bind the
# run to the reviewed sources, and their modification times are renewed so
# no build made from an earlier mutation is reused (a copy made with rsync
# -a restores older times).
TARGETS = sorted({path for _, edits, _, _ in PROBES for path, _, _ in edits})
INPUTS = {path: digest(os.path.join(REPO, path)) for path in TARGETS}
for path in TARGETS:
    os.utime(os.path.join(REPO, path), None)
results = [{"inputs": INPUTS}]
# Unmodified baselines run first and three times each; their output is kept
# beside the results so any failure can be identified.
LOGS = os.path.join(os.path.dirname(os.path.abspath(sys.argv[2])), "baselines")
# An optional third argument runs only the probes whose name contains it.
ONLY = sys.argv[3] if len(sys.argv) > 3 else None
if not ONLY:
    os.makedirs(LOGS, exist_ok=True)
for index, cmd in enumerate(() if ONLY else BASELINES):
    codes = []
    for attempt in range(1, 4):
        code, out = run(cmd)
        codes.append(code)
        with open(os.path.join(LOGS, f"{index}-{cmd[-1].rsplit('/', 1)[-1]}-{attempt}.log"), "w") as f:
            f.write(out)
    results.append({"baseline": cmd, "exit_codes": codes, "passes": codes == [0, 0, 0]})
    print("baseline", cmd[-1], codes, flush=True)
for name, edits, cmd, test in PROBES:
    if ONLY and ONLY not in name:
        continue
    saved = apply(edits)
    try:
        code, out = run(cmd)
    finally:
        restore(saved)
    results.append({"mutation": name, "files": sorted({e[0] for e in edits}), "command": cmd, "guard": test,
                    "exit_code": code, "guard_failed": code != 0 and failed_on(test, out)})
    print(name, code, results[-1]["guard_failed"], flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
