# C04 and the "Academy hosted content and learner sync" workload

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "Academy hosted content and learner sync" to C04. C04 ships Academy
sign-in, the protected learner session and the learner transport, but no
learner screen issues learner calls yet: no catalog, lesson or sync
workload exists for an operator to run or for a measurement to observe.

This record registers the numeric budgets and methods for that workload now,
before C05 implements Academy content and before any measurement, under the
T20 governance (no retroactive limits):

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| learner-call-overhead | p95 | 75 ms | 100 | Fixture build against the synthetic Academy endpoint answering immediately: from presentation dispatch of a learner query to its result in the tRPC link, minus the endpoint's own handling time recorded in its log. |
| catalog-to-lesson | p95 | 1500 ms | 20 | Fixture build, signed in, synthetic endpoint with the C05 literal catalog and a 50 KiB lesson: from activating a lesson in the catalog to its heading and first paragraph being exposed in the accessibility tree. |
| academy-process-tree-resident | max | 450 MiB | 20 | Process-tree RSS (as C03) sampled once per second for 20 s after opening ten lessons in sequence. |

C05 measures these on its prepared fixture bytes and may not change them
without a versioned rationale and reviewer acknowledgement. The C03
`budgets.json` hash is unchanged by this record.
