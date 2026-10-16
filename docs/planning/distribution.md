# Source and binary distribution gate

D001 calls for public noncommercial application distribution with no billing.
That does not itself select an outbound source license. Q24 asks the owner to
choose newly authored v2 terms; no blanket license has been applied yet.

## Required G01 output before code extraction

Record an owner-approved distribution/provenance manifest covering:

1. Source license and binary distribution terms for newly authored v2 code.
   Record the exact decision; do not infer terms from an intention not to charge.
2. Written authorization/ownership basis and scope for redistributing shared
   Academy presentation inside v2, retaining third-party terms/notices.
   D001/D004/D005 establish intended public application/shared-source reuse;
   confirm actual redistribution scope with the owner at G01 before extracting
   or publishing packages. Do not assume `private: true` is a license or relicense
   all Academy source by implication.
3. Every planned framework-derived import and its disposition: eligible under
   verified terms/permission, separately licensed third-party dependency, or
   excluded in favor of independently authored implementation/patterns. Initially
   no ArtCraft first-party source is approved for copying. Root fair-source
   billing/link retention terms remain intact in the learning fork.
4. SDK/Harness/native/font/icon/JS/Rust distribution obligations, modified-file
   notices where required, and the package/binary notice inventory.

Unresolved provenance/redistribution blocks G01 and relevant extraction. This is
an observed source constraint, not a presumption that every private package is
unusable. Do not re-ask permission where a recorded owner decision already grants
the exact needed scope.

## C26 precondition and check

The approved terms/permission manifest must match exact shipped source,
dependency, native and resource inventories. Check required LICENSE/NOTICE/
attribution and modification notices in distributed source/binary forms; no
unapproved framework code or relicensed private Academy source may slip into the
bundle. Hash the manifest/notices with the final artifact. Missing permission,
terms or notices block public delivery even when signing/notarization passes.

If selected, Apache-2.0 covers newly authored owner-controlled code only;
it permits redistribution/commercial reuse subject to its terms. Upstream code
keeps its own terms. [Authoritative Apache-2.0 text](https://www.apache.org/licenses/LICENSE-2.0)
