# C41 remediation after attempt-02 (9/8/9/8/8, accepted at 8 under D034; the push to 9)

| Finding | Remediation | Where |
|---|---|---|
| R1 the row parser trimmed names and values on every render, losing a trailing space as it was typed and rewriting a padded query value when another row was edited | Rows are verbatim: a name is the text before the separator and a value the text after it, as written; a header row keeps the colon's spacing on the row (`sep`) and is written back with it, a new row with `: `; a param's separator is `=` alone, so `q=  padded  ` stays padded. Test: a document with a padded query value; "hello world" typed character by character into a new param and a new header; the padded value untouched; Send and Save carry `q=  padded  \nnote=hello world` and `Accept: application/json\nX-Note: hello world` exactly; text mode shows the same lines | `request-rows.ts`, `api-client.test.tsx` |
