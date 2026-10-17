# C04 remediation after Codex review attempt-02

Attempt-02 scored architecture 9, behavior 8, authority_security 9,
validation 8, overall 8 with one remaining required fix (RF04 guidance for
HTTP 5xx mutations). Its records are unchanged in attempt-02/.

| Finding | Cause | Change | Acceptance |
|---|---|---|---|
| RF04 remaining: a 5xx mutation error carried `outcome: 'unknown'` but not the possible-effect guidance | the link added the outcome to error envelopes but kept the server's message as the error message | for an Unknown mutation error envelope the link leads with "Academy may have applied this change. Check its current state before trying again." and keeps the server's message (`data.serverMessage`), code and status | desktop-link suite: the HTTP 5xx mutation case now asserts the guidance, `outcome: 'unknown'`, the preserved `INTERNAL_SERVER_ERROR`/500/`serverMessage` and exactly one dispatch, alongside the lost-reply, timeout and cancellation cases; NotSent, Rejected and the ordinary domain error (`shop-buy.error.json`) are unchanged; probe "a server error on a mutation lacks the possibly-applied guidance" |
