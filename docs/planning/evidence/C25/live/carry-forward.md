# C25 live receipts: carry-forward to the final tree

Every current receipt under `live/L*/` and the run record name one source
fingerprint:

- receipts' tree: `3df30f29f163e8330e3f7b3b15aea803897a66df9fd226806d02362cd040a08d`
- final tree: `5104c79e6fd4822bc094cd678d0d594d6b2da8fdf6e0535358026a6047acfede`

The final tree differs from the receipts' tree in exactly two files, and in
nothing the live runner or the app executes differently:

- `crates/desktop-core/src/changes.rs`: comment text only, correcting why
  a filtering profile replace leaves out the service's own fields. The SDK
  contract accepts unknown fields; only its nulls are refused.
- `crates/desktop-core/tests/dlp_subset/main.rs`: a test expectation. The
  replace no longer sends the profile's `name`, which the service keeps.
  The C25 test gate's `dlp_subset` target found it, and the receipts' own
  change was already that.

Verified mechanically: restoring these two files to their earlier text
reproduces the receipts' fingerprint exactly (`xtask fingerprint --chunk
C25`, `source`), and restoring the final text reproduces the final one.
Exact diff:

```diff
--- a/crates/desktop-core/src/changes.rs
+++ b/crates/desktop-core/src/changes.rs
@@ -274,8 +274,8 @@
         .is_some_and(|s| !s.trim().is_empty())
 }
 
-/// Fields a data filtering profile reads back with that its request
-/// contract does not take.
+/// The service's own fields of a data filtering profile, as it reads one
+/// back; a replace leaves them out.
 const FILTERING_READ_ONLY: &[&str] = &[
     "id",
     "name",
@@ -351,9 +351,9 @@
         ChangeKind::DataPatterns => {}
         ChangeKind::FilteringProfiles => {
             // An edit begins from the item as read. The service reads unset
-            // fields back as null and adds fields of its own; the request
-            // contract refuses both, and the service keeps them unchanged
-            // when they are left out.
+            // fields back as null, which the request contract refuses, and
+            // adds fields of its own, which a replace never sends back; the
+            // service keeps both unchanged when they are left out (C25).
             body.retain(|key, v| !v.is_null() && !FILTERING_READ_ONLY.contains(&key.as_str()));
         }
         ChangeKind::DataProfiles => {
--- a/crates/desktop-core/tests/dlp_subset/main.rs
+++ b/crates/desktop-core/tests/dlp_subset/main.rs
@@ -634,9 +634,11 @@
             .request_line
             .starts_with("PUT /dlp/v2/api/data-filtering-profiles/filter-1 ")
     );
+    // The profile's name is the service's own field: a replace leaves it
+    // out, and the service keeps it (C25).
     assert_eq!(
         body(&write),
-        json!({"name": "Default", "file_based": false, "non_file_based": true})
+        json!({"file_based": false, "non_file_based": true})
     );
     assert_eq!(replaced.readback, Some(Readback::Found));
 }
```
