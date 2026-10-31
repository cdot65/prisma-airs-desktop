# C40 attempt-03 — 9/9/9/8/8, no blockers (the push to 9; the acceptance stands on attempt-02)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C39b's 49a5bac1): architecture 9, behavior 9, authority_security 9, validation 8, overall 8; blockers empty, one required fix, one evidence gap. Attempt-02's fixes were remediated in db959566 and are not raised again.

Accepted failures, remediated for attempt-04 (D034: a new generation, later chunks not reopened):

- R1: the driver's table judgement guessed the row's input by the longest text and took the starter pack's name for it, so a missing, collapsed or overlapping input still passed. Bind the input to the row's own input cell; missing, collapsed and overlapping probes must fail while the readable baselines pass; regenerate C40-N02's evidence.
- E1: the chunk brief requires C40-N01 to cover the Explain banner; the inventory text and the driver never enabled Explain, so no retained tree shows it. Add the native assertion and retain evidence of the banner with Explain on and its removal with it off, in both themes.
