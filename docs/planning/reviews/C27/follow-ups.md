# C27 follow-ups (optional suggestions from the passing attempt-05)

Not applied after the review, because any change to a scoped file invalidates
the receipt; both are taken up when the controller regenerates the C27 receipt
after C26 completes (C27.md "Safety and recovery").

1. `docs/planning/evidence/C27/runs/verify-coverage.py`: parse `# env-dep:`
   lines before skipping comments and refuse an empty dependency-file set. The
   reviewer independently confirmed the only env-deps are the manifest
   directory and package version 2.0.0, both already bound.
2. `docs/architecture/ui-v2/screens/boards.md`: distinguish semantic fixture
   states from the literal frozen props (the prototype uses `step=cases` and
   `verify=idle|done|fail`; the table says `packs`, `running`, `warn`) and
   document the event setup that produces the running and warning states so
   C29 reproduces them directly.
