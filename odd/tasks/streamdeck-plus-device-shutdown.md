# Stream Deck cooperative device shutdown

## Objective and evidence
Stop application-owned device communication on accepted window close or application shutdown. Current main.rs discards HID worker ownership and GLib polling source; event_api.rs has unconditional retry waits and connected loops, and report uploads lack stop checks. This demonstrates missing cooperative teardown, not writes after process termination.

## Scope and constraints
Cancel future/pending artwork reports, wake retry waits, finish the current synchronous HID call, join the HID worker, release its device, and remove UI polling. Keep USB disconnect/reconnect retry and artwork replay while application remains open. No display clearing (it would send more data). Preserve latest-only events, padding, shortcut compatibility and all pre-existing dirty work. Do not expand into cancellation of arbitrary external action subprocesses or block shutdown joining that worker. No HID hardware interaction during tests, no installation/staging/commits/publishing. Commit identities pending explicit user authorization. Engram unavailable: mirror odd/streamdeck-plus-device-shutdown/tasks pending.

## Tasks
- [x] S1: Implement owned cancellable HID lifecycle and idempotent GTK teardown, including configuration-error path, with deterministic stop/write regression tests and README behavior notes. Status: implemented and writer checks passed; commit pending authorization. Route: delegated writer; multiple files and lifecycle/test changes. Allowed: src/event_api.rs, src/main.rs, README.md.
- [x] S2: Independently verify lifecycle behavior, reconnect preservation and prior UI/shortcut regressions. Status: passed; no scoped regressions. Route: delegated verifier; native assessment unavailable unless fresh assess proves otherwise.

## Acceptance and checks
Stop queued uploads before transmission; check cancellation after preparation and between reports/targets. No report admitted after stop; synchronous call already in progress cannot be retracted. Worker ends while discovering, retrying or idle; interrupt retry delays. Repeat shutdown safely. Retain source ownership and remove polling during cleanup. Configuration error cannot leave unowned HID worker. Do not duplicate workers across repeated activation.
Test-first with fakes and channel/barrier synchronization, not hardware or sleeps for ordering. Observe meaningful RED/GREEN for cancellation and cleanup. Preserve disconnect/reconnect separately from stop. Run cargo fmt --check, cargo test --offline --lib event_api::tests -- --nocapture, cargo test --offline --bin streamdeck-plus-monitor -- --nocapture, cargo test --offline, cargo clippy --offline --all-targets --all-features -- -D warnings, cargo build --offline, git diff --check. Require actual executed GTK markers, disclose skipped checks. Real device teardown timing/visual acceptance remain manual.

## Delivery and progress
Forecast: 250–400 authored diff lines for coherent shutdown work unit, ask-on-risk. Size is advisory, never code-golf or omit tests. Separate from previous UI follow-up. S1 changed event_api.rs, main.rs and README.md. Worker reports dirty-baseline changed-line growth +609 (not isolated authored-patch size), exceeding forecast; mostly lifecycle tests/seams and ownership. No commit or delivery operation pending; review workload must be resolved before any authorized commit/PR. Existing branch feature/streamdeck-plus-event-monitor.

## S1 evidence
Observed test-first compilation RED for missing lifecycle APIs, not runtime-assertion RED. Final 10 event API and 14 binary tests passed; serialized GTK exercised close/shutdown polling removal and repeated activation. Full suite 63 library +14 binary passed. Fmt, clippy, build and diff check passed; intermediate inference error corrected. Config-error startup ordering structurally checked only. Discovery/retry/idle stop, queued uploads, post-preparation stop, remaining reports/targets, consumer loss, repeated stop and reconnect covered with fakes. No hardware runs. A synchronous HID call can delay joining; actual hardware duration unverified.
Fresh native assess unassessable and inspect blocked (package-local-binary-missing); no lineage/mutation. Independent verifier required. Mirror remains pending, no commits/staging/installations.

## S2 independent evidence
All exact checks passed: fmt, 10 event API tests (also final spot rerun), 14 binary tests with executed GTK, full 63 library +14 binary tests, clippy, build, diff check. Cancellation and report admission use the same mutex; a report already admitted may enter the synchronous write after cancellation, but later reports are rejected. GTK exercised close then shutdown repeat cleanup, polling removal and activation reuse plus prior latest-only/shortcut/geometry checks. Shutdown-only with live polling not separately exercised. Configuration ordering and Drop structurally inspected, not full startup tests. No hardware timing/visual validation, no native review, no commits/staging/tool installation.

## Next step
Implementation and automated independent verification complete. Test graceful close and reconnect on real Stream Deck+; retained display pixels are expected. Blocking HID initialization/open/read/write may delay joining; actual driver timing remains unverified. Review unavailable until separately authorized tooling repair; commits and mirror still pending.
