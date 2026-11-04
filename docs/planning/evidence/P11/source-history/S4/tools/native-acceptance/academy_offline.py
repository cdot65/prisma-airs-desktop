"""Synthetic Academy offline contract for C07 native acceptance: the course
access check, the course bundle (Academy's own unit and lab replies, as
Academy's desktop router builds it), the revision manifest, and a second
course past Academy's bundle cap: four unit bundles, each within Academy's
7 MiB bundle cap (MAX_BUNDLE_BYTES, applied to unitBundle too), that
together exceed the desktop's 16 MiB record limit after the third. Leases and the account follow the endpoint's mode
(academy_server.py placeholders). Nothing here is real content.

  build(catalog) -> {"procedure": reply}, given the C06 catalog
"""

import json

import academy_catalog as cat
import academy_program as prog

BIG = "model-security"
BIG_TITLE = "Model Security"
WEEK = 7 * 86_400


def lease(seconds=WEEK):
    return {"issuedAt": "{{in_s:0}}", "expiresAt": f"{{{{in_s:{seconds}}}}}", "access": "restricted"}


def access(allowed=True, seconds=WEEK):
    def course(course_id):
        if not allowed:
            return {"courseId": course_id, "allowed": False, "reason": "FORBIDDEN",
                    "issuedAt": "{{in_s:0}}", "expiresAt": "{{in_s:0}}"}
        return {"courseId": course_id, "allowed": True, "access": "restricted",
                "issuedAt": "{{in_s:0}}", "expiresAt": f"{{{{in_s:{seconds}}}}}"}
    return {"serverTime": "{{in_s:0}}", "accountId": "{{account}}", "courses": [course(cat.COURSE), course(BIG)]}


def course_units(catalog):
    values = catalog["units.get"]["values"]
    return [values[row["id"]] for module, _ in cat.MODULES for row in cat.siblings(module)]


def bundle(catalog, revision="rev-1", seconds=WEEK, manifest=False):
    units = course_units(catalog)
    if manifest:
        units = [{"id": u["id"], "moduleId": u["moduleId"], "order": u["order"], "kind": u["kind"],
                  "title": u["title"], "labId": u.get("labId")} for u in units]
    labs = [] if manifest else [v["lab"] for v in catalog["labs.get"]["values"].values()]
    return {"schemaVersion": 1, "courseId": cat.COURSE, "title": "Runtime Security Foundations",
            "description": "Scan prompts and responses at runtime and act on what the scans find.",
            "revision": revision, "hash": revision, "accountId": "{{account}}",
            "modules": [{"moduleId": m, "order": i + 1} for i, (m, _) in enumerate(cat.MODULES)],
            "units": units, "labs": labs, "labModes": {}, "manifest": manifest, "lease": lease(seconds)}


BIG_UNITS = 4
PART_BYTES = 6 * 1024 * 1024
# Academy's MAX_BUNDLE_BYTES (contracts.ts), applied to bundle and unitBundle.
MAX_BUNDLE_BYTES = 7 * 1024 * 1024


def big_parts():
    """Four 6 MiB unit bundles (each below Academy's 7 MiB cap)."""
    def unit(n):
        return {"id": f"{BIG}-unit-{n}", "moduleId": f"{BIG}-m1", "order": n, "kind": "content",
                "title": f"Model Security part {n}", "bodyHtml": "<p>" + ("x" * PART_BYTES) + "</p>",
                "siblings": [], "pathContext": None, "status": "not_started"}
    manifest = {"schemaVersion": 1, "courseId": BIG, "title": BIG_TITLE, "description": "d", "revision": "big-1",
                "hash": "big-1", "accountId": "{{account}}", "modules": [{"moduleId": f"{BIG}-m1", "order": 1}],
                "units": [{"id": f"{BIG}-unit-{n}", "moduleId": f"{BIG}-m1", "order": n, "kind": "content",
                           "title": f"Model Security part {n}", "labId": None} for n in range(1, BIG_UNITS + 1)],
                "labs": [], "labModes": {}, "manifest": True, "lease": lease()}
    # Each part is Academy's whole bundle projection of one unit
    # (bundles.ts `bundle(ctx, course, {unitId})`), checked against its cap
    # the way Academy checks it (UTF-8 bytes of the JSON).
    parts = {}
    for n in range(1, BIG_UNITS + 1):
        part = {**manifest, "units": [unit(n)], "manifest": False}
        size = len(json.dumps(part, separators=(",", ":")).replace("{{account}}", "user_2fixturelearner").encode())
        assert size < MAX_BUNDLE_BYTES, size
        parts[f"{BIG}-unit-{n}"] = part
    return manifest, parts


def build(catalog):
    big_manifest, parts = big_parts()
    too_large = cat.error(413, "PAYLOAD_TOO_LARGE", -32013, "PAYLOAD_TOO_LARGE")
    course = catalog["courses.list"][0]
    return {
        "courses.list": [course, {**course, "id": BIG, "slug": BIG, "title": BIG_TITLE}],
        "desktop.access": access(),
        "desktop.bundle": {"by": "courseId", "values": {cat.COURSE: bundle(catalog), BIG: too_large}},
        "desktop.manifest": {"by": "courseId", "values": {cat.COURSE: bundle(catalog, manifest=True), BIG: big_manifest}},
        "desktop.unitBundle": {"by": "unitId", "values": parts},
        # The learner already has an attempt at the first module's lab, so
        # opening it reads the attempt (kept for offline use) with the lease
        # Academy gives an unlocked attempt.
        "desktop.attempt.get": {**prog.lab_attempt(0, prog.EMPTY_DRAFT), "accountId": "{{account}}", "lease": lease()},
        "desktop.attempt.list": {"accountId": "{{account}}",
                                 "attempts": [prog.lab_attempt(0, prog.EMPTY_DRAFT)], "nextCursor": None},
    }
