"""Synthetic Academy learner catalog for C05 native acceptance and the C05
workload measurement: one path, one course with two modules of twelve 50 KiB
lessons each, a knowledge check and a hands-on lab per module, answering the
learner procedures Academy's own pages call. Shapes follow Academy's
routers at the pinned commit; the knowledge-check replies are the literal
fixtures shared with the Rust and TypeScript suites
(packages/desktop-contracts/fixtures/academy). Nothing here is real
content, and no answer key is ever part of a reply.

  build(repo_root) -> {"procedure": reply | {"by": field, "values": {...}}}
"""

import json
import os
import urllib.parse

COURSE = "runtime-security"
PATH_SLUG = "runtime-path"
KC_UNIT = "unit-kc-prompt-injection"
LESSONS_PER_MODULE = 12
LESSON_BYTES = 50 * 1024
MODULES = [("m-rt-1", "Scanning at runtime"), ("m-rt-2", "Responding to findings")]


def lesson_id(module, n):
    return f"{module}-lesson-{n:02d}"


def lesson_title(module, n):
    return f"{dict(MODULES)[module]}: part {n}"


def error(status, code, rpc, message, **data):
    """A tRPC error reply as Academy's server sends it."""
    return {"__error__": {"status": status, "code": code, "rpc": rpc, "message": message, "data": data}}


UNAUTHORIZED = error(401, "UNAUTHORIZED", -32001, "UNAUTHORIZED")


def by_access(guest, account):
    """Academy's content-access decision: no credential (guest) or account."""
    return {"__auth__": {"guest": guest, "account": account}}


def diagram_svg(ink, paper):
    """A pre-rendered diagram artifact (the pipeline's light/dark SVG)."""
    boxes = "".join(
        f'<rect x="{10 + i * 150}" y="20" width="120" height="50" rx="8" fill="{paper}" stroke="{ink}"/>'
        f'<text x="{70 + i * 150}" y="50" text-anchor="middle" fill="{ink}" font-family="sans-serif" font-size="14">{label}</text>'
        for i, label in enumerate(["Prompt", "Scan", "Model"]))
    arrows = "".join(f'<line x1="{130 + i * 150}" y1="45" x2="{160 + i * 150}" y2="45" stroke="{ink}"/>' for i in range(2))
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="460" height="90" viewBox="0 0 460 90">'
           f'<style>text{{font-weight:600}}</style>{boxes}{arrows}</svg>')
    return urllib.parse.quote(svg, safe="")


def lesson_html(module, n):
    """A ~50 KiB lesson: an opening paragraph, sections, a code sample, links
    (a learner path, an external page, an in-page anchor) and, in the first
    lesson of each module, a Mermaid diagram."""
    first = (f"<p>Lesson {n} of {dict(MODULES)[module]} opens here: a runtime scan inspects each "
             "prompt and response before it reaches a user or a model.</p>")
    parts = [first]
    if n == 1:
        # As the content pipeline emits them: a diagram with pre-rendered
        # light/dark artifacts, and one without (its source is shown).
        parts.append('<div class="mermaid-diagram" data-mermaid data-diagram-hash="0123456789abcdef" '
                     f'data-diagram-light="{diagram_svg("#3c3836", "#fbf1c7")}" '
                     f'data-diagram-dark="{diagram_svg("#ebdbb2", "#282828")}">'
                     "graph LR; Prompt--&gt;Scan; Scan--&gt;Model</div>")
        parts.append('<div class="mermaid-diagram" data-mermaid>graph LR; Verdict--&gt;Log</div>')
    if n == 2 and module == MODULES[0][0]:
        # Gated and retired content, reached from a lesson as learners do.
        parts.append('<p>Next: <a href="/units/unit-members">members-only notes</a>, '
                     '<a href="/units/unit-locked">the advanced path\'s first lesson</a> and '
                     '<a href="/units/unit-missing">a retired lesson</a>.</p>')
    parts.append('<p>See <a href="/learn">the learning catalog</a>, the '
                 '<a href="https://docs.example.com/runtime">product documentation</a> or '
                 '<a href="#section-3">section 3</a>.</p>')
    section = 0
    while sum(len(p) for p in parts) < LESSON_BYTES:
        section += 1
        parts.append(f'<h2 id="section-{section}">Section {section}</h2>')
        for k in range(6):
            parts.append(
                f"<p>Paragraph {k + 1} of section {section}. Detections are reported with their category and "
                "action so an operator can tell a blocked prompt from an allowed one, and every verdict is "
                "explained in terms of the policy that produced it. Long lessons keep their structure so "
                "headings, lists and code stay navigable with a keyboard and a screen reader.</p>")
        parts.append("<ul><li>Inspect the prompt.</li><li>Inspect the response.</li><li>Record the verdict.</li></ul>")
        parts.append("<pre><code>airs scan --profile default --input prompt.txt</code></pre>")
    return "".join(parts)


def module_rows():
    rows = []
    for order, (module, title) in enumerate(MODULES, start=1):
        rows.append({
            "moduleId": module, "title": title, "description": f"{title}, in {LESSONS_PER_MODULE} lessons.",
            "concepts": ["Inspect prompts", "Inspect responses"], "estMinutes": 60, "order": order,
            "unitsDone": 0, "unitCount": LESSONS_PER_MODULE + 2, "percent": 0, "xpEarned": 0,
            "xpTotal": 100 * LESSONS_PER_MODULE + 270, "complete": False, "reuseCount": 1,
            "resumeUnitId": lesson_id(module, 1),
        })
    return rows


def siblings(module):
    rows = [{"id": lesson_id(module, n), "kind": "content", "order": n, "slug": str(n),
             "title": lesson_title(module, n), "xp": 100, "estMinutes": 5,
             "status": "not_started", "locked": False}
            for n in range(1, LESSONS_PER_MODULE + 1)]
    kc = KC_UNIT if module == MODULES[0][0] else f"{module}-kc"
    rows.append({"id": kc, "kind": "knowledge_check", "order": LESSONS_PER_MODULE + 1,
                 "slug": str(LESSONS_PER_MODULE + 1), "title": f"Knowledge check: {dict(MODULES)[module]}",
                 "xp": 120, "estMinutes": 6, "status": "not_started", "locked": False})
    rows.append({"id": f"{module}-lab", "kind": "lab", "order": LESSONS_PER_MODULE + 2,
                 "slug": str(LESSONS_PER_MODULE + 2), "title": f"Lab: {dict(MODULES)[module]}",
                 "xp": 150, "estMinutes": 15, "status": "not_started", "locked": False})
    return rows


def path_context(module):
    return {"course": {"courseId": COURSE, "title": "Runtime Security Foundations", "slug": COURSE},
            "moduleTitle": dict(MODULES)[module], "modules": module_rows()}


def units(module):
    out = {}
    for row in siblings(module):
        base = {"id": row["id"], "moduleId": module, "order": row["order"], "slug": row["slug"],
                "title": row["title"], "xp": row["xp"], "estMinutes": row["estMinutes"],
                "status": "not_started", "siblings": siblings(module), "pathContext": path_context(module)}
        if row["kind"] == "content":
            n = row["order"]
            out[row["id"]] = {**base, "kind": "content", "bodyHtml": lesson_html(module, n),
                              "objectives": ["Explain what a runtime scan inspects"], "inlineChecks": [],
                              "sourceLessonId": None}
        elif row["kind"] == "knowledge_check":
            out[row["id"]] = {**base, "kind": "knowledge_check", "quizDrawCount": 1}
        else:
            out[row["id"]] = {**base, "kind": "lab", "locked": False, "unlockHint": None, "labKind": "scan",
                              "labId": f"{module}-lab-def", "placement": {"courseId": COURSE, "order": row["order"] // 7},
                              "lab": {"id": f"{module}-lab-def"}}
    return out


def gated_units():
    """Units behind Academy's content-access gate, and one that is gone."""
    members = {"id": "unit-members", "moduleId": "m-rt-1", "order": 99, "slug": "99", "title": "Members-only notes",
               "xp": 0, "estMinutes": 2, "status": "not_started", "kind": "content",
               "bodyHtml": "<p>Notes for signed-in learners.</p>", "objectives": [], "inlineChecks": [],
               "sourceLessonId": None, "siblings": siblings("m-rt-1"), "pathContext": path_context("m-rt-1")}
    return {
        "unit-members": by_access(UNAUTHORIZED, members),
        "unit-locked": by_access(UNAUTHORIZED, error(403, "FORBIDDEN", -32003, "FORBIDDEN",
                                                     domainCode="path-progression-locked")),
        "unit-missing": error(404, "NOT_FOUND", -32004, "NOT_FOUND"),
    }


def shared_fixture(root, name):
    with open(os.path.join(root, "packages/desktop-contracts/fixtures/academy", name)) as f:
        return json.load(f)["result"]["data"]["json"]


def build(root):
    unit_replies = {}
    for module, _ in MODULES:
        unit_replies.update(units(module))
    unit_replies.update(gated_units())
    start = shared_fixture(root, "units-startKnowledgeCheck.result.json")
    second_kc = f"{MODULES[1][0]}-kc"
    course = {"id": COURSE, "slug": COURSE, "title": "Runtime Security Foundations",
              "description": "Scan prompts and responses at runtime and act on what the scans find.",
              "iconKey": None, "estMinutes": 150, "status": "published"}
    return {
        "paths.list": [{"id": "path-runtime", "slug": PATH_SLUG, "title": "Runtime Security Path",
                        "description": "Protect prompts and responses at runtime.", "familyKey": "prisma-airs",
                        "iconKey": None, "tone": "info", "courseCount": 1, "estMinutes": 150, "unitCount": 28,
                        "unitsDone": 0, "locked": False, "lockReason": None}],
        # The path's own page (`paths.get` by slug, the shape the router returns:
        # the sections and their course cards); any other slug is NOT_FOUND.
        "paths.get": {"by": "pathSlug", "values": {PATH_SLUG: {
            "id": "path-runtime", "slug": PATH_SLUG, "title": "Runtime Security Path",
            "tagline": "Protect prompts and responses at runtime.",
            "description": "Scan what reaches the model and what comes back, then act on the verdicts.",
            "level": "associate", "programId": None, "iconKey": None, "tone": "info",
            "sections": [{"id": "sec-runtime-1", "title": "Foundations", "description": "The scan loop and its verdicts.", "order": 0,
                          "courses": [{"id": COURSE, "slug": COURSE, "title": course["title"], "description": course["description"],
                                       "level": "associate", "coursePurpose": None, "iconKey": None, "tone": "info", "estMinutes": 150,
                                       "order": 0, "unitCount": 28, "unitsDone": 0, "moduleCount": len(MODULES), "modulesComplete": 0,
                                       "status": "published", "teaser": None}]}]}}},
        "library.list": [
            {"type": "course", "id": COURSE, "slug": COURSE, "title": course["title"],
             "description": course["description"], "durationMinutes": 150, "level": "Associate",
             "courseCount": None, "labKind": None, "courseSlug": None},
        ],
        "courses.list": [course],
        "paths.forCourse": [],
        "enrollment.affectedByCourseRemoval": {"certifications": []},
        "progress.courseSummary": {"hasProgress": False, "units": 0, "quizzes": 0, "labs": 0, "objectives": 0},
        "modules.intro": None,
        "modules.diagramVideo": None,
        "caches.list": [],
        "units.listByPath": module_rows(),
        "units.listByModule": {"by": "moduleId", "values": {m: siblings(m) for m, _ in MODULES}},
        "units.get": {"by": "unitId", "values": unit_replies},
        "units.complete": {"completed": True, "award": None},
        "units.startKnowledgeCheck": {"by": "unitId", "values": {KC_UNIT: start, second_kc: {**start, "unitId": second_kc}}},
        "units.checkKcAnswer": {"correct": True, "explanation": "Retrieved content can carry injected instructions.",
                                "correctValue": 0},
        # The second module's check fails on the server after receiving the
        # submission (its outcome is unknown to the desktop).
        "units.submitKnowledgeCheck": {"by": "unitId", "values": {
            KC_UNIT: shared_fixture(root, "units-submitKnowledgeCheck.result.json"),
            second_kc: error(500, "INTERNAL_SERVER_ERROR", -32603, "Internal server error")}},
        "labs.get": {"lab": {"kind": "scan", "id": "m-rt-1-lab-def", "title": "Block an injected prompt",
                             "brief": "Configure a profile that blocks the injected prompt in the sample."},
                     "progress": {}},
    }
