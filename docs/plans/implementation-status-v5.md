# Implementation status v5 (T001..T034)

Generated 2026-10-18 by a scratch script (not in repo): reads `docs/plans/plan-architecture-v5.json`, runs every verify command via Git Bash from the repo root at the final state, compares exit code with `expect_exit_code`, records sha256 (first 12 hex) of stdout+stderr.

Totals: 181 verify commands, 145 match, 36 mismatch. All mismatches are explained below (none is a real failure).

| Task | Status | Title | Files | Verify match/total | Mismatching commands |
|---|---|---|---|---|---|
| T001 | done | Rename package asl-core to lens-core (directory, manifests, temporary aliases) | `crates/asl-core`, `Cargo.toml`, `crates/lens-core/Cargo.toml`, `src-tauri/Cargo.toml` | 4/6 | #1,#6 |
| T002 | done | Finish rename: drop temporary aliases, use lens_core identifiers everywhere | `crates/lens-core/Cargo.toml`, `src-tauri/Cargo.toml`, `src-tauri/src/main.rs`, `crates/lens-core/tests/ingest.rs`, `crates/lens-core/examples/smoke.rs` | 6/7 | #7 |
| T003 | done | Add consts module defining every section 2 constant once | `crates/lens-core/src/consts.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/src/engine.rs`, `crates/lens-core/src/reader.rs` | 6/7 | #7 |
| T004 | done | Injectable Clock and Change Detector | `crates/lens-core/src/clock.rs`, `crates/lens-core/src/change_detector.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/tests/change_detector.rs` | 8/9 | #9 |
| T005 | done | Reader hardening 1/2: strict UTF-8, MAX_LINE_BYTES boundary, partial tail | `crates/lens-core/src/reader.rs`, `crates/lens-core/tests/reader.rs`, `crates/lens-core/tests/ingest.rs` | 4/5 | #5 |
| T006 | done | Data model and JSON Schema conformance | `crates/lens-core/src/model.rs`, `crates/lens-core/schema/lens.schema.json`, `crates/lens-core/tests/schema.rs`, `crates/lens-core/Cargo.toml` | 4/5 | #5 |
| T007 | done | Project Resolver: keys, display, git root, WSL path conversion | `crates/lens-core/src/project.rs`, `crates/lens-core/tests/project.rs` | 5/6 | #6 |
| T008 | done | WSL helper crate: CLI parsing and exit codes | `Cargo.toml`, `crates/lens-wsl-helper/Cargo.toml`, `crates/lens-wsl-helper/src/main.rs`, `crates/lens-wsl-helper/src/cli.rs`, `crates/lens-wsl-helper/tests/cli.rs` | 3/4 | #4 |
| T009 | done | Clock offset correction | `crates/lens-core/src/clock_offset.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/tests/clock_offset.rs` | 3/4 | #4 |
| T010 | done | Redaction rules R1-R4 and truncation (single sanitize entry) | `crates/lens-core/src/redact.rs`, `crates/lens-core/tests/redact.rs` | 4/5 | #5 |
| T011 | done | Child process spawn wrapper: Stdio::null and Job Object | `crates/lens-core/src/process.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/Cargo.toml`, `crates/lens-core/tests/child_stdin.rs` | 5/6 | #6 |
| T012 | done | Claude Code parsing 1/2: type-only probe, uid/path rules, mixed sessions | `crates/lens-core/src/claude.rs`, `crates/lens-core/src/engine.rs`, `crates/lens-core/tests/claude_parse.rs` | 4/5 | #5 |
| T013 | done | Codex rollout parsing 1/2: session_meta rules and fork boundary | `crates/lens-core/src/codex.rs`, `crates/lens-core/src/engine.rs`, `crates/lens-core/tests/codex_parse.rs` | 4/5 | #5 |
| T014 | done | Reader hardening 2/2: file replacement, Windows share mode, read-only open | `crates/lens-core/src/reader.rs`, `crates/lens-core/tests/reader.rs` | 5/6 | #6 |
| T015 | done | NDJSON receiver 1/2: hello, protocol, shape and order checks | `crates/lens-core/src/ndjson.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/tests/ndjson.rs` | 3/4 | #4 |
| T016 | done | settings.json persistence | `crates/lens-core/src/settings.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/tests/settings.rs` | 3/4 | #4 |
| T017 | done | WSL distro discovery and running-state polling | `crates/lens-core/src/distro.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/Cargo.toml`, `crates/lens-core/tests/distro.rs` | 3/4 | #4 |
| T018 | done | Claude Code parsing 2/2: title order, activity set, turn signals | `crates/lens-core/src/claude.rs`, `crates/lens-core/tests/claude_parse.rs` | 4/5 | #5 |
| T019 | done | Codex rollout parsing 2/2: body source, thread_source, title and turn signals | `crates/lens-core/src/codex.rs`, `crates/lens-core/src/engine.rs`, `crates/lens-core/tests/codex_parse.rs` | 3/4 | #4 |
| T020 | done | NDJSON receiver 2/2: line limits, malformed input and double redaction | `crates/lens-core/src/ndjson.rs`, `crates/lens-core/tests/ndjson.rs` | 3/4 | #4 |
| T021 | done | Status and stale decision table | `crates/lens-core/src/session.rs`, `crates/lens-core/tests/status_table.rs` | 4/5 | #5 |
| T022 | done | Initial scan policy and engine wiring (Clock, Change Detector, status table) | `crates/lens-core/src/engine.rs`, `crates/lens-core/src/reader.rs`, `crates/lens-core/tests/initial_scan.rs` | 4/5 | #5 |
| T023 | done | Aggregator 1/2: session table, hierarchy, ordering, REEVAL | `crates/lens-core/src/aggregator.rs`, `crates/lens-core/src/engine.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/tests/aggregator.rs` | 4/5 | #5 |
| T024 | done | Helper serve loop: normalized NDJSON output | `crates/lens-wsl-helper/src/serve.rs`, `crates/lens-wsl-helper/src/main.rs`, `crates/lens-wsl-helper/Cargo.toml`, `crates/lens-wsl-helper/tests/serve.rs` | 5/6 | #6 |
| T025 | done | Read-only guarantee end-to-end | `crates/lens-core/tests/readonly.rs`, `crates/lens-core/Cargo.toml` | 4/5 | #5 |
| T026 | done | Aggregator 2/2: own-repo listing, outgoing sanitization, change events | `crates/lens-core/src/aggregator.rs`, `crates/lens-core/tests/aggregator.rs` | 3/4 | #4 |
| T027 | done | Helper supervisor 1/3: launch, heartbeat timeout, backoff, stopped state | `crates/lens-core/src/supervisor.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/tests/supervisor_lifecycle.rs` | 3/5 | #4,#5 |
| T028 | done | IPC layer 1/2: read commands list_projects, get_project_detail, get_health | `crates/lens-core/src/ipc.rs`, `crates/lens-core/src/lib.rs`, `crates/lens-core/tests/ipc.rs` | 3/4 | #4 |
| T029 | done | Helper supervisor 2/3: exit-code and version policy | `crates/lens-core/src/supervisor.rs`, `crates/lens-core/tests/supervisor_exit.rs` | 3/4 | #4 |
| T030 | done | Helper supervisor 3/3: ingest path, clock offset and settings reflection | `crates/lens-core/src/supervisor.rs`, `crates/lens-core/tests/supervisor_ingest.rs` | 3/4 | #4 |
| T031 | done | IPC layer 2/2: write commands, Ev events, payload sanitization | `crates/lens-core/src/ipc.rs`, `crates/lens-core/tests/ipc.rs` | 3/4 | #4 |
| T032 | done | Tauri wiring: commands, events, capabilities (get_snapshot kept as shim) | `src-tauri/src/main.rs`, `src-tauri/capabilities/default.json` | 6/8 | #1,#8 |
| T033 | done | Frontend UI: types, one row per project, relative time, remove get_snapshot shim | `src/types.ts`, `src/App.tsx`, `src/format.ts`, `src-tauri/src/main.rs` | 11/12 | #12 |
| T034 | done | Closing traceability audit and whole-workspace gates | `crates/lens-core/tests/traceability.rs`, `crates/lens-core/tests/traceability.json` | 5/5 | - |

## Mismatch explanations

Numbering `#n` = 1-based position of the command within the task `verify` list. Proof for `rg` checks: each match location is listed with the later task(s) whose `changes` claim that file (later than the task being checked), i.e. the matching code was added after that task's stop point.

### T001 #1 (expected exit 0, got 1, sha e3b0c44298fc)

`cargo metadata --format-version 1 --no-deps | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{const m=JSON.parse(s);const p=m.packages.find(x=>x.name==='lens-core');const l=p&&p.targe`

Verdict: superseded by T002. T001 expected the temporary lib target `asl_core`; T002 removes that alias by design. Current lib target name: `lens_core`; T002's own equivalent check (expects `lens_core`) matches now: True.

### T001 #6 (expected exit 1, got 0, sha fc95ca160086)

`rg -n "lens_core" crates src-tauri/src`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `src-tauri/src/main.rs:4` `use lens_core::aggregator::{origin_for_source, Aggregator, SessionInpu` -> added by T002, T032, T033
- `src-tauri/src/main.rs:5` `use lens_core::change_detector::ChangeDetector;` -> added by T002, T032, T033
- `src-tauri/src/main.rs:6` `use lens_core::clock::{Clock, SystemClock};` -> added by T002, T032, T033
- `src-tauri/src/main.rs:7` `use lens_core::distro::{self, DistroMonitor, SystemLxssRegistry, WslEx` -> added by T002, T032, T033
- `src-tauri/src/main.rs:8` `use lens_core::engine::{default_sources, Engine};` -> added by T002, T032, T033
- `src-tauri/src/main.rs:9` `use lens_core::ipc::{self, AggregatorView, Phase, RealPathProbe};` -> added by T002, T032, T033
- `src-tauri/src/main.rs:10` `use lens_core::model::v5::{self, Detail, Ev, IpcError, ProjectActivity` -> added by T002, T032, T033
- `src-tauri/src/main.rs:11` `use lens_core::model::{Agent, Completeness, Confidence, StatusValue};` -> added by T002, T032, T033
- `src-tauri/src/main.rs:12` `use lens_core::session::{OpenTool, SessionState, TurnEnd};` -> added by T002, T032, T033
- `src-tauri/src/main.rs:13` `use lens_core::settings::{RealFs, SettingsStore};` -> added by T002, T032, T033
- `src-tauri/src/main.rs:14` `use lens_core::supervisor::{Supervisor, WslLauncher};` -> added by T002, T032, T033
- `src-tauri/src/main.rs:55` `// ---- commands (thin wrappers over lens_core::ipc) ----` -> added by T002, T032, T033
- `src-tauri/src/main.rs:117` `fn local_health(h: &lens_core::model::SourceHealth) -> SourceHealth {` -> added by T002, T032, T033
- `src-tauri/src/main.rs:118` `use lens_core::model::SourceState as L;` -> added by T002, T032, T033
- `src-tauri/src/main.rs:209` `let wsl_launcher: Arc<dyn lens_core::supervisor::Launcher> = Arc::new(` -> added by T002, T032, T033
- `crates/lens-wsl-helper/src/serve.rs:7` `use lens_core::aggregator::{origin_for_source, Aggregator};` -> added by T024
- `crates/lens-wsl-helper/src/serve.rs:8` `use lens_core::clock::{Clock, SystemClock};` -> added by T024
- `crates/lens-wsl-helper/src/serve.rs:9` `use lens_core::consts::{HB_INTERVAL_SECS, HOT_POLL_SECS, MAX_LINE_BYTE` -> added by T024
- `crates/lens-wsl-helper/src/serve.rs:10` `use lens_core::model::v5::{self, HelperErrorCode, Msg, SourceHealth, S` -> added by T024
- `crates/lens-wsl-helper/src/serve.rs:11` `use lens_core::model::{Agent, SourceState as CoreState};` -> added by T024
- `crates/lens-wsl-helper/src/serve.rs:12` `use lens_core::redact::{sanitize, Field};` -> added by T024
- `crates/lens-wsl-helper/src/serve.rs:13` `use lens_core::{Engine, SourceConfig};` -> added by T024
- `crates/lens-wsl-helper/tests/serve.rs:2` `use lens_core::clock::FakeClock;` -> added by T024
- `crates/lens-core/tests/aggregator.rs:4` `use lens_core::aggregator::{origin_for_source, Aggregator, SessionInpu` -> added by T023, T026
- `crates/lens-core/tests/aggregator.rs:5` `use lens_core::clock::FakeClock;` -> added by T023, T026
- `crates/lens-core/tests/aggregator.rs:6` `use lens_core::model::v5::{SessionProject, UNATTRIBUTED_KEY};` -> added by T023, T026
- `crates/lens-core/tests/aggregator.rs:7` `use lens_core::model::{Agent, Attribution, Confidence, SessionKind, St` -> added by T023, T026
- `crates/lens-core/tests/aggregator.rs:8` `use lens_core::session::{SessionState, TurnEnd};` -> added by T023, T026
- `crates/lens-core/tests/aggregator.rs:180` `agg.take_events().iter().filter(|e| matches!(e, lens_core::model::v5::` -> added by T023, T026
- `crates/lens-core/tests/aggregator.rs:265` `lens_core::model::v5::Ev::ProjectsChanged { order, .. } => {` -> added by T023, T026
- `crates/lens-wsl-helper/src/cli.rs:2` `use lens_core::consts::PROTOCOL_VERSION;` -> added by T008
- `crates/lens-core/tests/change_detector.rs:1` `use lens_core::change_detector::{backoff_secs, Backoff, ChangeDetector` -> added by T004
- `crates/lens-core/tests/change_detector.rs:2` `use lens_core::clock::FakeClock;` -> added by T004
- `crates/lens-core/examples/smoke.rs:5` `use lens_core::model::{SessionKind, StatusValue};` -> added by T002
- `crates/lens-core/examples/smoke.rs:6` `use lens_core::{default_sources, Engine};` -> added by T002
- `crates/lens-core/tests/claude_parse.rs:4` `use lens_core::consts::MAX_LINE_BYTES;` -> added by T012, T018
- `crates/lens-core/tests/claude_parse.rs:5` `use lens_core::engine::{Engine, SourceConfig};` -> added by T012, T018
- `crates/lens-core/tests/claude_parse.rs:6` `use lens_core::model::*;` -> added by T012, T018
- `crates/lens-core/tests/child_stdin.rs:1` `use lens_core::clock::{Clock, FakeClock};` -> added by T011
- `crates/lens-core/tests/child_stdin.rs:2` `use lens_core::process::{spawn_with, SpawnedChild};` -> added by T011
- `crates/lens-core/tests/codex_parse.rs:4` `use lens_core::engine::{Engine, SourceConfig};` -> added by T013, T019
- `crates/lens-core/tests/codex_parse.rs:5` `use lens_core::model::*;` -> added by T013, T019
- `crates/lens-core/tests/clock_offset.rs:1` `use lens_core::clock_offset::ClockOffset;` -> added by T009
- `crates/lens-core/tests/distro.rs:5` `use lens_core::clock::FakeClock;` -> added by T017
- `crates/lens-core/tests/distro.rs:6` `use lens_core::distro::*;` -> added by T017
- `crates/lens-core/tests/distro.rs:7` `use lens_core::model::v5::SourceState;` -> added by T017
- `crates/lens-core/tests/ingest.rs:3` `use lens_core::engine::{Engine, SourceConfig};` -> added by T002, T005
- `crates/lens-core/tests/ingest.rs:4` `use lens_core::model::*;` -> added by T002, T005
- `crates/lens-core/tests/ingest.rs:5` `use lens_core::reader::{self, Cursor, Limits};` -> added by T002, T005
- `crates/lens-core/tests/initial_scan.rs:5` `use lens_core::clock::FakeClock;` -> added by T022
- `crates/lens-core/tests/initial_scan.rs:6` `use lens_core::consts::{HEAD_SCAN_BYTES, INITIAL_SCAN_DAYS, TAIL_SCAN_` -> added by T022
- `crates/lens-core/tests/initial_scan.rs:7` `use lens_core::engine::{Engine, SourceConfig};` -> added by T022
- `crates/lens-core/tests/initial_scan.rs:8` `use lens_core::model::*;` -> added by T022
- `crates/lens-core/tests/initial_scan.rs:154` `let denied: lens_core::engine::DirOpener =` -> added by T022
- `crates/lens-core/tests/ipc.rs:3` `use lens_core::ipc::{` -> added by T028, T031
- `crates/lens-core/tests/ipc.rs:7` `use lens_core::model::v5::{Ev, IpcError};` -> added by T028, T031
- `crates/lens-core/tests/ipc.rs:8` `use lens_core::settings::{RealFs, SettingsFs, SettingsStore};` -> added by T028, T031
- `crates/lens-core/tests/ipc.rs:10` `use lens_core::model::v5::{` -> added by T028, T031
- `crates/lens-core/tests/ipc.rs:13` `use lens_core::model::{Agent, TimeSource};` -> added by T028, T031
- `crates/lens-core/tests/ipc.rs:224` `use lens_core::model::v5::{CurrentTask, Health, Hierarchy, Status, Uni` -> added by T028, T031
- `crates/lens-core/tests/ipc.rs:225` `use lens_core::model::{Completeness, Confidence, SessionKind, StatusVa` -> added by T028, T031
- `crates/lens-core/tests/ndjson.rs:1` `use lens_core::ndjson::{Accepted, Outcome, Receiver};` -> added by T015, T020
- `crates/lens-core/tests/project.rs:3` `use lens_core::engine::{Engine, SourceConfig};` -> added by T007
- `crates/lens-core/tests/project.rs:4` `use lens_core::model::v5::UNATTRIBUTED_KEY;` -> added by T007
- `crates/lens-core/tests/project.rs:5` `use lens_core::model::{Agent, Attribution};` -> added by T007
- `crates/lens-core/tests/project.rs:6` `use lens_core::project::{normalize, ProjectResolver};` -> added by T007
- `crates/lens-core/tests/readonly.rs:6` `use lens_core::engine::{Engine, SourceConfig};` -> added by T025
- `crates/lens-core/tests/readonly.rs:7` `use lens_core::model::*;` -> added by T025
- `crates/lens-core/tests/readonly.rs:84` `let denied: lens_core::engine::DirOpener =` -> added by T025
- `crates/lens-core/tests/reader.rs:3` `use lens_core::reader::{self, Cursor, Limits};` -> added by T005, T014
- `crates/lens-core/tests/settings.rs:4` `use lens_core::consts::HELPER_PATH;` -> added by T016
- `crates/lens-core/tests/settings.rs:5` `use lens_core::settings::{RealFs, SettingsFs, SettingsStore, SETTINGS_` -> added by T016
- `crates/lens-core/tests/schema.rs:2` `use lens_core::model::v5::*;` -> added by T006
- `crates/lens-core/tests/schema.rs:3` `use lens_core::model::{` -> added by T006
- `crates/lens-core/tests/redact.rs:1` `use lens_core::redact::{sanitize, Field};` -> added by T010
- `crates/lens-core/tests/supervisor_exit.rs:4` `use lens_core::clock::FakeClock;` -> added by T029
- `crates/lens-core/tests/supervisor_exit.rs:5` `use lens_core::model::v5::SourceState;` -> added by T029
- `crates/lens-core/tests/supervisor_exit.rs:6` `use lens_core::settings::Settings;` -> added by T029
- `crates/lens-core/tests/supervisor_exit.rs:7` `use lens_core::supervisor::*;` -> added by T029
- `crates/lens-core/tests/status_table.rs:4` `use lens_core::model::{Confidence, SessionKind, Status, StatusValue};` -> added by T021
- `crates/lens-core/tests/status_table.rs:5` `use lens_core::session::{aggregate_updating, evaluate, SessionState, T` -> added by T021
- `crates/lens-core/tests/supervisor_lifecycle.rs:5` `use lens_core::aggregator::{origin_for_source, Aggregator, SessionInpu` -> added by T027
- `crates/lens-core/tests/supervisor_lifecycle.rs:6` `use lens_core::clock::FakeClock;` -> added by T027
- `crates/lens-core/tests/supervisor_lifecycle.rs:7` `use lens_core::model::v5::{SessionProject, SourceState};` -> added by T027
- `crates/lens-core/tests/supervisor_lifecycle.rs:8` `use lens_core::model::{Agent, Attribution, SessionKind};` -> added by T027
- `crates/lens-core/tests/supervisor_lifecycle.rs:9` `use lens_core::session::SessionState;` -> added by T027
- `crates/lens-core/tests/supervisor_lifecycle.rs:10` `use lens_core::settings::Settings;` -> added by T027
- `crates/lens-core/tests/supervisor_lifecycle.rs:11` `use lens_core::supervisor::*;` -> added by T027
- `crates/lens-core/tests/supervisor_ingest.rs:4` `use lens_core::clock::FakeClock;` -> added by T030
- `crates/lens-core/tests/supervisor_ingest.rs:5` `use lens_core::distro::{Distro, DistroMonitor, LxssRegistry, RunningPr` -> added by T030
- `crates/lens-core/tests/supervisor_ingest.rs:6` `use lens_core::model::v5::wsl_source_id;` -> added by T030
- `crates/lens-core/tests/supervisor_ingest.rs:7` `use lens_core::model::Agent;` -> added by T030
- `crates/lens-core/tests/supervisor_ingest.rs:8` `use lens_core::settings::Settings;` -> added by T030
- `crates/lens-core/tests/supervisor_ingest.rs:9` `use lens_core::supervisor::*;` -> added by T030
- `crates/lens-core/tests/supervisor_ingest.rs:167` `impl lens_core::distro::Launcher for NoLaunch {` -> added by T030

### T002 #7 (expected exit 1, got 0, sha c8dd12fce765)

`rg -n "fn (section2_values_match_design)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/src/consts.rs:36` `fn section2_values_match_design() {` -> added by T003

### T003 #7 (expected exit 1, got 0, sha 3291df5e1b6f)

`rg -n "fn (f4_invalid_utf8_line_dropped|f16a_sk_key|schema_file_equals_design_block|hot_poll|f18_windows_key_case_insensitive|f10a_threshold_cases|f29a_usage_errors_exit_2)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-wsl-helper/tests/cli.rs:17` `fn f29a_usage_errors_exit_2() {` -> added by T008
- `crates/lens-core/tests/change_detector.rs:13` `fn hot_poll() {` -> added by T004
- `crates/lens-core/tests/clock_offset.rs:10` `fn f10a_threshold_cases() {` -> added by T009
- `crates/lens-core/tests/project.rs:10` `fn f18_windows_key_case_insensitive() {` -> added by T007
- `crates/lens-core/tests/redact.rs:8` `fn f16a_sk_key() {` -> added by T010
- `crates/lens-core/tests/reader.rs:14` `fn f4_invalid_utf8_line_dropped() {` -> added by T005, T014
- `crates/lens-core/tests/schema.rs:100` `fn schema_file_equals_design_block() {` -> added by T006

### T004 #9 (expected exit 1, got 0, sha 62c8e96b9cbe)

`rg -n "fn (f9_child_stdin_null_exits_zero_read_zero|f21_seven_day_boundary|f2_child_idle_after_parent_end_turn|f11a_heartbeat_silence_29_vs_30)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/aggregator.rs:53` `fn f2_child_idle_after_parent_end_turn() {` -> added by T023, T026
- `crates/lens-core/tests/child_stdin.rs:41` `fn f9_child_stdin_null_exits_zero_read_zero() {` -> added by T011
- `crates/lens-core/tests/initial_scan.rs:63` `fn f21_seven_day_boundary() {` -> added by T022
- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T005 #5 (expected exit 1, got 0, sha 04bd12fddcf4)

`rg -n "fn (f3_api_request_blob_not_fully_parsed|f1a_fork_ordinal_excludes_inherited|f13a_protocol_mismatch_stops|f7_file_replaced_on_fileid_change_or_shrink)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/claude_parse.rs:51` `fn f3_api_request_blob_not_fully_parsed() {` -> added by T012, T018
- `crates/lens-core/tests/codex_parse.rs:62` `fn f1a_fork_ordinal_excludes_inherited() {` -> added by T013, T019
- `crates/lens-core/tests/ndjson.rs:6` `fn f13a_protocol_mismatch_stops() {` -> added by T015, T020
- `crates/lens-core/tests/reader.rs:99` `fn f7_file_replaced_on_fileid_change_or_shrink() {` -> added by T014

### T006 #5 (expected exit 1, got 0, sha 0123a8c2400f)

`rg -n "fn (f27_persist_exact_json|f13a_protocol_mismatch_stops|version1_distro_is_unsupported_and_not_started|f2_child_idle_after_parent_end_turn|first_line_is_hello_protocol_1)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/aggregator.rs:53` `fn f2_child_idle_after_parent_end_turn() {` -> added by T023, T026
- `crates/lens-wsl-helper/tests/serve.rs:58` `fn first_line_is_hello_protocol_1() {` -> added by T024
- `crates/lens-core/tests/distro.rs:55` `fn version1_distro_is_unsupported_and_not_started() {` -> added by T017
- `crates/lens-core/tests/ndjson.rs:6` `fn f13a_protocol_mismatch_stops() {` -> added by T015, T020
- `crates/lens-core/tests/settings.rs:28` `fn f27_persist_exact_json() {` -> added by T016

### T007 #6 (expected exit 1, got 0, sha b9eb4da9c491)

`rg -n "fn (f2_child_idle_after_parent_end_turn)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/aggregator.rs:53` `fn f2_child_idle_after_parent_end_turn() {` -> added by T023, T026

### T008 #4 (expected exit 1, got 0, sha b80c9d5bc410)

`rg -n "fn (first_line_is_hello_protocol_1|f11a_heartbeat_silence_29_vs_30)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-wsl-helper/tests/serve.rs:58` `fn first_line_is_hello_protocol_1() {` -> added by T024
- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T009 #4 (expected exit 1, got 0, sha e67619b6da83)

`rg -n "fn (f13a_protocol_mismatch_stops|f2_child_idle_after_parent_end_turn|f11a_heartbeat_silence_29_vs_30)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/aggregator.rs:53` `fn f2_child_idle_after_parent_end_turn() {` -> added by T023, T026
- `crates/lens-core/tests/ndjson.rs:6` `fn f13a_protocol_mismatch_stops() {` -> added by T015, T020
- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T010 #5 (expected exit 1, got 0, sha efba92daf1b0)

`rg -n "fn (f13a_protocol_mismatch_stops|f2_child_idle_after_parent_end_turn|first_line_is_hello_protocol_1|f31_leaked_token_redacted_before_ipc)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-wsl-helper/tests/serve.rs:58` `fn first_line_is_hello_protocol_1() {` -> added by T024
- `crates/lens-core/tests/aggregator.rs:53` `fn f2_child_idle_after_parent_end_turn() {` -> added by T023, T026
- `crates/lens-core/tests/ndjson.rs:6` `fn f13a_protocol_mismatch_stops() {` -> added by T015, T020
- `crates/lens-core/tests/supervisor_ingest.rs:94` `fn f31_leaked_token_redacted_before_ipc() {` -> added by T030

### T011 #6 (expected exit 1, got 0, sha e6271b210c82)

`rg -n "fn (version1_distro_is_unsupported_and_not_started|f11a_heartbeat_silence_29_vs_30)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/distro.rs:55` `fn version1_distro_is_unsupported_and_not_started() {` -> added by T017
- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T012 #5 (expected exit 1, got 0, sha dc14cc679db6)

`rg -n "fn (title_fallback_order)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/claude_parse.rs:169` `fn title_fallback_order() {` -> added by T018

### T013 #5 (expected exit 1, got 0, sha cadcc20615af)

`rg -n "fn (f17_response_item_only_body)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/codex_parse.rs:143` `fn f17_response_item_only_body() {` -> added by T019

### T014 #6 (expected exit 1, got 0, sha d66527ddbd09)

`rg -n "fn (f21_seven_day_boundary)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/initial_scan.rs:63` `fn f21_seven_day_boundary() {` -> added by T022

### T015 #4 (expected exit 1, got 0, sha b0a690dca7c6)

`rg -n "fn (f13d_oversized_line)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/ndjson.rs:67` `fn f13d_oversized_line() {` -> added by T020

### T016 #4 (expected exit 1, got 0, sha bcec89bfeb4a)

`rg -n "fn (f11a_heartbeat_silence_29_vs_30|f23_unc_wsl_localhost_path_invalid)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/ipc.rs:136` `fn f23_unc_wsl_localhost_path_invalid() {` -> added by T028, T031
- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T017 #4 (expected exit 1, got 0, sha bcec89bfeb4a)

`rg -n "fn (f11a_heartbeat_silence_29_vs_30|f23_unc_wsl_localhost_path_invalid)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/ipc.rs:136` `fn f23_unc_wsl_localhost_path_invalid() {` -> added by T028, T031
- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T018 #5 (expected exit 1, got 0, sha 7a46b18a0f6b)

`rg -n "fn (t31_rule3_open_turn_recent_write_window)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/status_table.rs:46` `fn t31_rule3_open_turn_recent_write_window() {` -> added by T021

### T019 #4 (expected exit 1, got 0, sha 7a46b18a0f6b)

`rg -n "fn (t31_rule3_open_turn_recent_write_window)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/status_table.rs:46` `fn t31_rule3_open_turn_recent_write_window() {` -> added by T021

### T020 #4 (expected exit 1, got 0, sha 49344c5493fc)

`rg -n "fn (f11a_heartbeat_silence_29_vs_30)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T021 #5 (expected exit 1, got 0, sha 510b3dbdae2d)

`rg -n "fn (f21_seven_day_boundary|f2_child_idle_after_parent_end_turn)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/aggregator.rs:53` `fn f2_child_idle_after_parent_end_turn() {` -> added by T023, T026
- `crates/lens-core/tests/initial_scan.rs:63` `fn f21_seven_day_boundary() {` -> added by T022

### T022 #5 (expected exit 1, got 0, sha 410b1bdacb08)

`rg -n "fn (f2_child_idle_after_parent_end_turn|f22_hash_and_mtime_unchanged_after_scan|first_line_is_hello_protocol_1)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-wsl-helper/tests/serve.rs:58` `fn first_line_is_hello_protocol_1() {` -> added by T024
- `crates/lens-core/tests/aggregator.rs:53` `fn f2_child_idle_after_parent_end_turn() {` -> added by T023, T026
- `crates/lens-core/tests/readonly.rs:51` `fn f22_hash_and_mtime_unchanged_after_scan() {` -> added by T025

### T023 #5 (expected exit 1, got 0, sha 3aaf15b0c1a8)

`rg -n "fn (f15_own_repo_session_is_listed)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/aggregator.rs:184` `fn f15_own_repo_session_is_listed() {` -> added by T026

### T024 #6 (expected exit 1, got 0, sha b48be8959f65)

`rg -n "fn (every_f_id_maps_to_existing_test_fn)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/traceability.rs:55` `fn every_f_id_maps_to_existing_test_fn() {` -> added by T034

### T025 #5 (expected exit 1, got 0, sha b48be8959f65)

`rg -n "fn (every_f_id_maps_to_existing_test_fn)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/traceability.rs:55` `fn every_f_id_maps_to_existing_test_fn() {` -> added by T034

### T026 #4 (expected exit 1, got 0, sha fc12d4b62e67)

`rg -n "fn (f11a_heartbeat_silence_29_vs_30|f25a_unknown_key_project_not_found)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/ipc.rs:70` `fn f25a_unknown_key_project_not_found() {` -> added by T028, T031
- `crates/lens-core/tests/supervisor_lifecycle.rs:77` `fn f11a_heartbeat_silence_29_vs_30() {` -> added by T027

### T027 #4 (expected exit 1, got 0, sha 0ceedd363104)

`rg -n "fn (f13a_hello_protocol_2_no_restart|f31_leaked_token_redacted_before_ipc)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/supervisor_exit.rs:91` `fn f13a_hello_protocol_2_no_restart() {` -> added by T029
- `crates/lens-core/tests/supervisor_ingest.rs:94` `fn f31_leaked_token_redacted_before_ipc() {` -> added by T030

### T027 #5 (expected exit 1, got 0, sha 15912cd0bbcc)

`rg -n "ipc::|supervisor::" src-tauri/src`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `src-tauri/src/main.rs:9` `use lens_core::ipc::{self, AggregatorView, Phase, RealPathProbe};` -> added by T032, T033
- `src-tauri/src/main.rs:14` `use lens_core::supervisor::{Supervisor, WslLauncher};` -> added by T032, T033
- `src-tauri/src/main.rs:62` `ipc::list_projects(&AggregatorView { aggregator: &agg, phase, health }` -> added by T032, T033
- `src-tauri/src/main.rs:69` `ipc::get_project_detail(&AggregatorView { aggregator: &agg, phase, hea` -> added by T032, T033
- `src-tauri/src/main.rs:77` `ipc::get_health(&AggregatorView { aggregator: &agg, phase, health })` -> added by T032, T033
- `src-tauri/src/main.rs:83` `ipc::set_distro_enabled(&mut lock(&state.store), &known, &distro, enab` -> added by T032, T033
- `src-tauri/src/main.rs:94` `ipc::set_source_root(&mut lock(&state.store), &known, &RealPathProbe, ` -> added by T032, T033
- `src-tauri/src/main.rs:105` `let _ = app.emit("project_detail_changed", ipc::ev_project_detail_chan` -> added by T032, T033
- `src-tauri/src/main.rs:209` `let wsl_launcher: Arc<dyn lens_core::supervisor::Launcher> = Arc::new(` -> added by T032, T033
- `src-tauri/src/main.rs:323` `emit_ev(&app, &ipc::ev_health_changed(health));` -> added by T032, T033

### T028 #4 (expected exit 1, got 0, sha c2a220a0853c)

`rg -n "fn (f23_unc_wsl_localhost_path_invalid)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/ipc.rs:136` `fn f23_unc_wsl_localhost_path_invalid() {` -> added by T031

### T029 #4 (expected exit 1, got 0, sha 15912cd0bbcc)

`rg -n "ipc::|supervisor::" src-tauri/src`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `src-tauri/src/main.rs:9` `use lens_core::ipc::{self, AggregatorView, Phase, RealPathProbe};` -> added by T032, T033
- `src-tauri/src/main.rs:14` `use lens_core::supervisor::{Supervisor, WslLauncher};` -> added by T032, T033
- `src-tauri/src/main.rs:62` `ipc::list_projects(&AggregatorView { aggregator: &agg, phase, health }` -> added by T032, T033
- `src-tauri/src/main.rs:69` `ipc::get_project_detail(&AggregatorView { aggregator: &agg, phase, hea` -> added by T032, T033
- `src-tauri/src/main.rs:77` `ipc::get_health(&AggregatorView { aggregator: &agg, phase, health })` -> added by T032, T033
- `src-tauri/src/main.rs:83` `ipc::set_distro_enabled(&mut lock(&state.store), &known, &distro, enab` -> added by T032, T033
- `src-tauri/src/main.rs:94` `ipc::set_source_root(&mut lock(&state.store), &known, &RealPathProbe, ` -> added by T032, T033
- `src-tauri/src/main.rs:105` `let _ = app.emit("project_detail_changed", ipc::ev_project_detail_chan` -> added by T032, T033
- `src-tauri/src/main.rs:209` `let wsl_launcher: Arc<dyn lens_core::supervisor::Launcher> = Arc::new(` -> added by T032, T033
- `src-tauri/src/main.rs:323` `emit_ev(&app, &ipc::ev_health_changed(health));` -> added by T032, T033

### T030 #4 (expected exit 1, got 0, sha 15912cd0bbcc)

`rg -n "ipc::|supervisor::" src-tauri/src`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `src-tauri/src/main.rs:9` `use lens_core::ipc::{self, AggregatorView, Phase, RealPathProbe};` -> added by T032, T033
- `src-tauri/src/main.rs:14` `use lens_core::supervisor::{Supervisor, WslLauncher};` -> added by T032, T033
- `src-tauri/src/main.rs:62` `ipc::list_projects(&AggregatorView { aggregator: &agg, phase, health }` -> added by T032, T033
- `src-tauri/src/main.rs:69` `ipc::get_project_detail(&AggregatorView { aggregator: &agg, phase, hea` -> added by T032, T033
- `src-tauri/src/main.rs:77` `ipc::get_health(&AggregatorView { aggregator: &agg, phase, health })` -> added by T032, T033
- `src-tauri/src/main.rs:83` `ipc::set_distro_enabled(&mut lock(&state.store), &known, &distro, enab` -> added by T032, T033
- `src-tauri/src/main.rs:94` `ipc::set_source_root(&mut lock(&state.store), &known, &RealPathProbe, ` -> added by T032, T033
- `src-tauri/src/main.rs:105` `let _ = app.emit("project_detail_changed", ipc::ev_project_detail_chan` -> added by T032, T033
- `src-tauri/src/main.rs:209` `let wsl_launcher: Arc<dyn lens_core::supervisor::Launcher> = Arc::new(` -> added by T032, T033
- `src-tauri/src/main.rs:323` `emit_ev(&app, &ipc::ev_health_changed(health));` -> added by T032, T033

### T031 #4 (expected exit 1, got 0, sha 15912cd0bbcc)

`rg -n "ipc::|supervisor::" src-tauri/src`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `src-tauri/src/main.rs:9` `use lens_core::ipc::{self, AggregatorView, Phase, RealPathProbe};` -> added by T032, T033
- `src-tauri/src/main.rs:14` `use lens_core::supervisor::{Supervisor, WslLauncher};` -> added by T032, T033
- `src-tauri/src/main.rs:62` `ipc::list_projects(&AggregatorView { aggregator: &agg, phase, health }` -> added by T032, T033
- `src-tauri/src/main.rs:69` `ipc::get_project_detail(&AggregatorView { aggregator: &agg, phase, hea` -> added by T032, T033
- `src-tauri/src/main.rs:77` `ipc::get_health(&AggregatorView { aggregator: &agg, phase, health })` -> added by T032, T033
- `src-tauri/src/main.rs:83` `ipc::set_distro_enabled(&mut lock(&state.store), &known, &distro, enab` -> added by T032, T033
- `src-tauri/src/main.rs:94` `ipc::set_source_root(&mut lock(&state.store), &known, &RealPathProbe, ` -> added by T032, T033
- `src-tauri/src/main.rs:105` `let _ = app.emit("project_detail_changed", ipc::ev_project_detail_chan` -> added by T032, T033
- `src-tauri/src/main.rs:209` `let wsl_launcher: Arc<dyn lens_core::supervisor::Launcher> = Arc::new(` -> added by T032, T033
- `src-tauri/src/main.rs:323` `emit_ev(&app, &ipc::ev_health_changed(health));` -> added by T032, T033

### T032 #1 (expected exit 0, got 1, sha e3b0c44298fc)

`test "$(rg -U -o 'generate_handler!\[[^]]*\]' src-tauri/src/main.rs | tr -d ' \r\n' | sed 's/,\]/]/')" = 'generate_handler![list_projects,get_project_detail,get_health,set_distro_enabled,set_source_ro`

Verdict: superseded by T033. T032 kept `get_snapshot` as a temporary shim (title); T033 removes it and its check (exactly 5 handlers) matches now: True. `rg -c get_snapshot src src-tauri/src`: no matches.

### T032 #8 (expected exit 1, got 0, sha d8641e58278e)

`rg -n "list_projects" src`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `src/App.tsx:116` `invokeRetry<ProjectActivity[]>("list_projects", cancelled)` -> added by T033

### T033 #12 (expected exit 1, got 0, sha b48be8959f65)

`rg -n "fn (every_f_id_maps_to_existing_test_fn)\b" crates`

Verdict: valid only at its own stop point; superseded by later task(s). Matches now found:

- `crates/lens-core/tests/traceability.rs:55` `fn every_f_id_maps_to_existing_test_fn() {` -> added by T034


## All verify commands

| Task | # | Command | Exp | Got | OK | sha256[:12] |
|---|---|---|---|---|---|---|
| T001 | 1 | `cargo metadata --format-version 1 --no-deps \| node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',...` | 0 | 1 | NO | e3b0c44298fc |
| T001 | 2 | `rg -n "crates/asl-core" Cargo.toml src-tauri/Cargo.toml` | 1 | 1 | yes | e3b0c44298fc |
| T001 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | f754a41702ae |
| T001 | 4 | `cargo check -p agent-session-lens` | 0 | 0 | yes | a7642a086742 |
| T001 | 5 | `cargo test --workspace` | 0 | 0 | yes | f751f9bfd1a7 |
| T001 | 6 | `rg -n "lens_core" crates src-tauri/src` | 1 | 0 | NO | fc95ca160086 |
| T002 | 1 | `cargo metadata --format-version 1 --no-deps \| node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',...` | 0 | 0 | yes | e3b0c44298fc |
| T002 | 2 | `rg -n "asl[-_]core" Cargo.toml crates src-tauri/Cargo.toml src-tauri/src` | 1 | 1 | yes | e3b0c44298fc |
| T002 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | d7796ed1025b |
| T002 | 4 | `cargo build -p lens-core --examples` | 0 | 0 | yes | a610b32e66fc |
| T002 | 5 | `cargo check -p agent-session-lens` | 0 | 0 | yes | a7642a086742 |
| T002 | 6 | `cargo test --workspace` | 0 | 0 | yes | e6952b369899 |
| T002 | 7 | `rg -n "fn (section2_values_match_design)\b" crates` | 1 | 0 | NO | c8dd12fce765 |
| T003 | 1 | `test "$(cargo test -p lens-core --lib -- --list 2>/dev/null \| rg -c '^(consts::tests::section2_values_matc...` | 0 | 0 | yes | e3b0c44298fc |
| T003 | 2 | `cargo test -p lens-core --lib consts::tests` | 0 | 0 | yes | 8d90d2b2f42b |
| T003 | 3 | `test "$(rg -c 'pub const [A-Z0-9_]+:' crates/lens-core/src/consts.rs)" = 27` | 0 | 0 | yes | e3b0c44298fc |
| T003 | 4 | `rg -n "4194304\|4_194_304\|8388608\|8_388_608\|524288\|524_288" crates/lens-core/src --glob '!consts.rs'` | 1 | 1 | yes | e3b0c44298fc |
| T003 | 5 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | 6507b5fc9660 |
| T003 | 6 | `cargo test --workspace` | 0 | 0 | yes | 50cfb7e68de4 |
| T003 | 7 | `rg -n "fn (f4_invalid_utf8_line_dropped\|f16a_sk_key\|schema_file_equals_design_block\|hot_poll\|f18_window...` | 1 | 0 | NO | 3291df5e1b6f |
| T004 | 1 | `test "$(cargo test -p lens-core --test change_detector -- --list 2>/dev/null \| rg -c '^(hot_poll\|reconcil...` | 0 | 0 | yes | e3b0c44298fc |
| T004 | 2 | `cargo test -p lens-core --test change_detector hot_poll` | 0 | 0 | yes | 3d9f57a20534 |
| T004 | 3 | `cargo test -p lens-core --test change_detector reconcile` | 0 | 0 | yes | 274be0c6774e |
| T004 | 4 | `cargo test -p lens-core --test change_detector overflow` | 0 | 0 | yes | fc583e4beed9 |
| T004 | 5 | `cargo test -p lens-core --test change_detector backoff` | 0 | 0 | yes | d726450cf891 |
| T004 | 6 | `cargo test -p lens-core --test change_detector debounce` | 0 | 0 | yes | 61f9c3c320d5 |
| T004 | 7 | `rg -n "thread::sleep\|Instant::now\|SystemTime::now" crates/lens-core/src/change_detector.rs` | 1 | 1 | yes | e3b0c44298fc |
| T004 | 8 | `cargo test --workspace` | 0 | 0 | yes | 0146ada395fe |
| T004 | 9 | `rg -n "fn (f9_child_stdin_null_exits_zero_read_zero\|f21_seven_day_boundary\|f2_child_idle_after_parent_end...` | 1 | 0 | NO | 62c8e96b9cbe |
| T005 | 1 | `test "$(cargo test -p lens-core --test reader -- --list 2>/dev/null \| rg -c '^(f4_invalid_utf8_line_droppe...` | 0 | 0 | yes | e3b0c44298fc |
| T005 | 2 | `cargo test -p lens-core --test reader` | 0 | 0 | yes | 1f21c539d62b |
| T005 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | 43caea2d709c |
| T005 | 4 | `cargo test --workspace` | 0 | 0 | yes | bb9f1ea9056b |
| T005 | 5 | `rg -n "fn (f3_api_request_blob_not_fully_parsed\|f1a_fork_ordinal_excludes_inherited\|f13a_protocol_mismatc...` | 1 | 0 | NO | 04bd12fddcf4 |
| T006 | 1 | `test "$(cargo test -p lens-core --test schema -- --list 2>/dev/null \| rg -c '^(schema_file_equals_design_b...` | 0 | 0 | yes | e3b0c44298fc |
| T006 | 2 | `cargo test -p lens-core --test schema` | 0 | 0 | yes | 83582aaedfd9 |
| T006 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | 96d55f9c09ce |
| T006 | 4 | `cargo test --workspace` | 0 | 0 | yes | ece2274de4f7 |
| T006 | 5 | `rg -n "fn (f27_persist_exact_json\|f13a_protocol_mismatch_stops\|version1_distro_is_unsupported_and_not_sta...` | 1 | 0 | NO | 0123a8c2400f |
| T007 | 1 | `test "$(cargo test -p lens-core --test project -- --list 2>/dev/null \| rg -c '^(f18_windows_key_case_insen...` | 0 | 0 | yes | e3b0c44298fc |
| T007 | 2 | `cargo test -p lens-core --test project` | 0 | 0 | yes | 24874948dd9b |
| T007 | 3 | `rg -n "Command\|std::process" crates/lens-core/src/project.rs` | 1 | 1 | yes | e3b0c44298fc |
| T007 | 4 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | 564101c046bf |
| T007 | 5 | `cargo test --workspace` | 0 | 0 | yes | d1fff884bc85 |
| T007 | 6 | `rg -n "fn (f2_child_idle_after_parent_end_turn)\b" crates` | 1 | 0 | NO | b9eb4da9c491 |
| T008 | 1 | `test "$(cargo test -p lens-wsl-helper --test cli -- --list 2>/dev/null \| rg -c '^(f29a_usage_errors_exit_2...` | 0 | 0 | yes | e3b0c44298fc |
| T008 | 2 | `cargo test -p lens-wsl-helper --test cli` | 0 | 0 | yes | 1e4a8418f8d9 |
| T008 | 3 | `cargo test --workspace` | 0 | 0 | yes | bfe0ae0a12a8 |
| T008 | 4 | `rg -n "fn (first_line_is_hello_protocol_1\|f11a_heartbeat_silence_29_vs_30)\b" crates` | 1 | 0 | NO | b80c9d5bc410 |
| T009 | 1 | `test "$(cargo test -p lens-core --test clock_offset -- --list 2>/dev/null \| rg -c '^(f10a_threshold_cases\...` | 0 | 0 | yes | e3b0c44298fc |
| T009 | 2 | `cargo test -p lens-core --test clock_offset` | 0 | 0 | yes | cbb26829a967 |
| T009 | 3 | `cargo test --workspace` | 0 | 0 | yes | 4d75c9a39ec0 |
| T009 | 4 | `rg -n "fn (f13a_protocol_mismatch_stops\|f2_child_idle_after_parent_end_turn\|f11a_heartbeat_silence_29_vs_...` | 1 | 0 | NO | e67619b6da83 |
| T010 | 1 | `test "$(cargo test -p lens-core --test redact -- --list 2>/dev/null \| rg -c '^(f16a_sk_key\|f16b_github_to...` | 0 | 0 | yes | e3b0c44298fc |
| T010 | 2 | `cargo test -p lens-core --test redact` | 0 | 0 | yes | 405509ab9a88 |
| T010 | 3 | `rg -c "pub fn sanitize" crates/lens-core/src/redact.rs` | 0 | 0 | yes | 4355a46b19d3 |
| T010 | 4 | `cargo test --workspace` | 0 | 0 | yes | 09bb929f4ef0 |
| T010 | 5 | `rg -n "fn (f13a_protocol_mismatch_stops\|f2_child_idle_after_parent_end_turn\|first_line_is_hello_protocol_...` | 1 | 0 | NO | efba92daf1b0 |
| T011 | 1 | `test "$(cargo test -p lens-core --test child_stdin -- --list 2>/dev/null \| rg -c '^(f9_child_stdin_null_ex...` | 0 | 0 | yes | e3b0c44298fc |
| T011 | 2 | `cargo test -p lens-core --test child_stdin` | 0 | 0 | yes | bc72c4ecee83 |
| T011 | 3 | `rg -n "Command::new" crates src-tauri/src --glob '*.rs' --glob '!process.rs' --glob '!**/tests/**'` | 1 | 1 | yes | e3b0c44298fc |
| T011 | 4 | `rg -n "stdin\(Stdio::null\(\)\)" crates/lens-core/src/process.rs` | 0 | 0 | yes | b4543fe6f81f |
| T011 | 5 | `cargo test --workspace` | 0 | 0 | yes | d2f450e4f55a |
| T011 | 6 | `rg -n "fn (version1_distro_is_unsupported_and_not_started\|f11a_heartbeat_silence_29_vs_30)\b" crates` | 1 | 0 | NO | e6271b210c82 |
| T012 | 1 | `test "$(cargo test -p lens-core --test claude_parse -- --list 2>/dev/null \| rg -c '^(f3_api_request_blob_n...` | 0 | 0 | yes | e3b0c44298fc |
| T012 | 2 | `cargo test -p lens-core --test claude_parse` | 0 | 0 | yes | 8839b205ab14 |
| T012 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | 18e3e9143ddb |
| T012 | 4 | `cargo test --workspace` | 0 | 0 | yes | a7470ab14ed2 |
| T012 | 5 | `rg -n "fn (title_fallback_order)\b" crates` | 1 | 0 | NO | dc14cc679db6 |
| T013 | 1 | `test "$(cargo test -p lens-core --test codex_parse -- --list 2>/dev/null \| rg -c '^(f1a_fork_ordinal_exclu...` | 0 | 0 | yes | e3b0c44298fc |
| T013 | 2 | `cargo test -p lens-core --test codex_parse` | 0 | 0 | yes | bab7c713d2ed |
| T013 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | a815f879c786 |
| T013 | 4 | `cargo test --workspace` | 0 | 0 | yes | 8c467c9bb17d |
| T013 | 5 | `rg -n "fn (f17_response_item_only_body)\b" crates` | 1 | 0 | NO | cadcc20615af |
| T014 | 1 | `test "$(cargo test -p lens-core --test reader -- --list 2>/dev/null \| rg -c '^(f7_file_replaced_on_fileid_...` | 0 | 0 | yes | e3b0c44298fc |
| T014 | 2 | `cargo test -p lens-core --test reader` | 0 | 0 | yes | 75fe6e6b7373 |
| T014 | 3 | `rg -n "\.(write\|append\|create\|create_new\|truncate)\(true\)\|File::create\|fs::write" crates/lens-core/s...` | 1 | 1 | yes | e3b0c44298fc |
| T014 | 4 | `rg -n "share_mode\(" crates/lens-core/src/reader.rs` | 0 | 0 | yes | 7ccf59af7cb9 |
| T014 | 5 | `cargo test --workspace` | 0 | 0 | yes | 70c418b64d8f |
| T014 | 6 | `rg -n "fn (f21_seven_day_boundary)\b" crates` | 1 | 0 | NO | d66527ddbd09 |
| T015 | 1 | `test "$(cargo test -p lens-core --test ndjson -- --list 2>/dev/null \| rg -c '^(f13a_protocol_mismatch_stop...` | 0 | 0 | yes | e3b0c44298fc |
| T015 | 2 | `cargo test -p lens-core --test ndjson` | 0 | 0 | yes | 35e2459bb8d7 |
| T015 | 3 | `cargo test --workspace` | 0 | 0 | yes | 1ef807a56f83 |
| T015 | 4 | `rg -n "fn (f13d_oversized_line)\b" crates` | 1 | 0 | NO | b0a690dca7c6 |
| T016 | 1 | `test "$(cargo test -p lens-core --test settings -- --list 2>/dev/null \| rg -c '^(f27_persist_exact_json\|f...` | 0 | 0 | yes | e3b0c44298fc |
| T016 | 2 | `cargo test -p lens-core --test settings` | 0 | 0 | yes | 8d4f590f0678 |
| T016 | 3 | `cargo test --workspace` | 0 | 0 | yes | ab54bde71a76 |
| T016 | 4 | `rg -n "fn (f11a_heartbeat_silence_29_vs_30\|f23_unc_wsl_localhost_path_invalid)\b" crates` | 1 | 0 | NO | bcec89bfeb4a |
| T017 | 1 | `test "$(cargo test -p lens-core --test distro -- --list 2>/dev/null \| rg -c '^(version1_distro_is_unsuppor...` | 0 | 0 | yes | e3b0c44298fc |
| T017 | 2 | `cargo test -p lens-core --test distro` | 0 | 0 | yes | 605ad6af0784 |
| T017 | 3 | `cargo test --workspace` | 0 | 0 | yes | cfa568027477 |
| T017 | 4 | `rg -n "fn (f11a_heartbeat_silence_29_vs_30\|f23_unc_wsl_localhost_path_invalid)\b" crates` | 1 | 0 | NO | bcec89bfeb4a |
| T018 | 1 | `test "$(cargo test -p lens-core --test claude_parse -- --list 2>/dev/null \| rg -c '^(title_fallback_order\...` | 0 | 0 | yes | e3b0c44298fc |
| T018 | 2 | `cargo test -p lens-core --test claude_parse` | 0 | 0 | yes | ab3b6894c63e |
| T018 | 3 | `rg -n "smoke\|real-logs" crates/lens-core/tests/claude_parse.rs` | 1 | 1 | yes | e3b0c44298fc |
| T018 | 4 | `cargo test --workspace` | 0 | 0 | yes | ede36aed149f |
| T018 | 5 | `rg -n "fn (t31_rule3_open_turn_recent_write_window)\b" crates` | 1 | 0 | NO | 7a46b18a0f6b |
| T019 | 1 | `test "$(cargo test -p lens-core --test codex_parse -- --list 2>/dev/null \| rg -c '^(f17_response_item_only...` | 0 | 0 | yes | e3b0c44298fc |
| T019 | 2 | `cargo test -p lens-core --test codex_parse` | 0 | 0 | yes | 66519c743975 |
| T019 | 3 | `cargo test --workspace` | 0 | 0 | yes | 13a265a2f746 |
| T019 | 4 | `rg -n "fn (t31_rule3_open_turn_recent_write_window)\b" crates` | 1 | 0 | NO | 7a46b18a0f6b |
| T020 | 1 | `test "$(cargo test -p lens-core --test ndjson -- --list 2>/dev/null \| rg -c '^(f13d_oversized_line\|f13e_m...` | 0 | 0 | yes | e3b0c44298fc |
| T020 | 2 | `cargo test -p lens-core --test ndjson` | 0 | 0 | yes | d0be83116ed6 |
| T020 | 3 | `cargo test --workspace` | 0 | 0 | yes | 78f9e441cc90 |
| T020 | 4 | `rg -n "fn (f11a_heartbeat_silence_29_vs_30)\b" crates` | 1 | 0 | NO | 49344c5493fc |
| T021 | 1 | `test "$(cargo test -p lens-core --test status_table -- --list 2>/dev/null \| rg -c '^(t31_rule3_open_turn_r...` | 0 | 0 | yes | e3b0c44298fc |
| T021 | 2 | `cargo test -p lens-core --test status_table` | 0 | 0 | yes | 484b090071de |
| T021 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | a9431dd9193a |
| T021 | 4 | `cargo test --workspace` | 0 | 0 | yes | 545e378e029f |
| T021 | 5 | `rg -n "fn (f21_seven_day_boundary\|f2_child_idle_after_parent_end_turn)\b" crates` | 1 | 0 | NO | 510b3dbdae2d |
| T022 | 1 | `test "$(cargo test -p lens-core --test initial_scan -- --list 2>/dev/null \| rg -c '^(f21_seven_day_boundar...` | 0 | 0 | yes | e3b0c44298fc |
| T022 | 2 | `cargo test -p lens-core --test initial_scan` | 0 | 0 | yes | e59cb84d7f77 |
| T022 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | 5b1b4d375164 |
| T022 | 4 | `cargo test --workspace` | 0 | 0 | yes | 109a388d8c63 |
| T022 | 5 | `rg -n "fn (f2_child_idle_after_parent_end_turn\|f22_hash_and_mtime_unchanged_after_scan\|first_line_is_hell...` | 1 | 0 | NO | 410b1bdacb08 |
| T023 | 1 | `test "$(cargo test -p lens-core --test aggregator -- --list 2>/dev/null \| rg -c '^(f2_child_idle_after_par...` | 0 | 0 | yes | e3b0c44298fc |
| T023 | 2 | `cargo test -p lens-core --test aggregator` | 0 | 0 | yes | b707c99aee86 |
| T023 | 3 | `cargo test -p lens-core --test ingest` | 0 | 0 | yes | 0d6f9204e256 |
| T023 | 4 | `cargo test --workspace` | 0 | 0 | yes | a4a992cbe368 |
| T023 | 5 | `rg -n "fn (f15_own_repo_session_is_listed)\b" crates` | 1 | 0 | NO | 3aaf15b0c1a8 |
| T024 | 1 | `test "$(cargo test -p lens-wsl-helper --test serve -- --list 2>/dev/null \| rg -c '^(first_line_is_hello_pr...` | 0 | 0 | yes | e3b0c44298fc |
| T024 | 2 | `cargo test -p lens-wsl-helper --test serve` | 0 | 0 | yes | ad14e428b7cd |
| T024 | 3 | `cargo test -p lens-wsl-helper --features fault_inject --test serve f29e_panic_exits_101` | 0 | 0 | yes | 67acc5f4c8af |
| T024 | 4 | `test "$(cargo test -p lens-wsl-helper --features fault_inject --test serve -- --list 2>/dev/null \| rg -c '...` | 0 | 0 | yes | e3b0c44298fc |
| T024 | 5 | `cargo test --workspace` | 0 | 0 | yes | dfbdf7cfd722 |
| T024 | 6 | `rg -n "fn (every_f_id_maps_to_existing_test_fn)\b" crates` | 1 | 0 | NO | b48be8959f65 |
| T025 | 1 | `test "$(cargo test -p lens-core --test readonly -- --list 2>/dev/null \| rg -c '^(f22_hash_and_mtime_unchan...` | 0 | 0 | yes | e3b0c44298fc |
| T025 | 2 | `cargo test -p lens-core --test readonly` | 0 | 0 | yes | 53d33bad3585 |
| T025 | 3 | `rg -n "\.(write\|append\|create\|create_new)\(true\)\|File::create" crates src-tauri/src --glob '*.rs' --gl...` | 1 | 1 | yes | e3b0c44298fc |
| T025 | 4 | `cargo test --workspace` | 0 | 0 | yes | ae19d019551a |
| T025 | 5 | `rg -n "fn (every_f_id_maps_to_existing_test_fn)\b" crates` | 1 | 0 | NO | b48be8959f65 |
| T026 | 1 | `test "$(cargo test -p lens-core --test aggregator -- --list 2>/dev/null \| rg -c '^(f15_own_repo_session_is...` | 0 | 0 | yes | e3b0c44298fc |
| T026 | 2 | `cargo test -p lens-core --test aggregator` | 0 | 0 | yes | 11a0439531fa |
| T026 | 3 | `cargo test --workspace` | 0 | 0 | yes | 27e0bc8cff41 |
| T026 | 4 | `rg -n "fn (f11a_heartbeat_silence_29_vs_30\|f25a_unknown_key_project_not_found)\b" crates` | 1 | 0 | NO | fc12d4b62e67 |
| T027 | 1 | `test "$(cargo test -p lens-core --test supervisor_lifecycle -- --list 2>/dev/null \| rg -c '^(f11a_heartbea...` | 0 | 0 | yes | e3b0c44298fc |
| T027 | 2 | `cargo test -p lens-core --test supervisor_lifecycle` | 0 | 0 | yes | af1cb85b681c |
| T027 | 3 | `cargo test --workspace` | 0 | 0 | yes | f093a68c714e |
| T027 | 4 | `rg -n "fn (f13a_hello_protocol_2_no_restart\|f31_leaked_token_redacted_before_ipc)\b" crates` | 1 | 0 | NO | 0ceedd363104 |
| T027 | 5 | `rg -n "ipc::\|supervisor::" src-tauri/src` | 1 | 0 | NO | 15912cd0bbcc |
| T028 | 1 | `test "$(cargo test -p lens-core --test ipc -- --list 2>/dev/null \| rg -c '^(f25a_unknown_key_project_not_f...` | 0 | 0 | yes | e3b0c44298fc |
| T028 | 2 | `cargo test -p lens-core --test ipc` | 0 | 0 | yes | 6851f8e681f8 |
| T028 | 3 | `cargo test --workspace` | 0 | 0 | yes | b1e4a4ff324f |
| T028 | 4 | `rg -n "fn (f23_unc_wsl_localhost_path_invalid)\b" crates` | 1 | 0 | NO | c2a220a0853c |
| T029 | 1 | `test "$(cargo test -p lens-core --test supervisor_exit -- --list 2>/dev/null \| rg -c '^(f13a_hello_protoco...` | 0 | 0 | yes | e3b0c44298fc |
| T029 | 2 | `cargo test -p lens-core --test supervisor_exit` | 0 | 0 | yes | 5889c6567999 |
| T029 | 3 | `cargo test --workspace` | 0 | 0 | yes | a6cf31ce2331 |
| T029 | 4 | `rg -n "ipc::\|supervisor::" src-tauri/src` | 1 | 0 | NO | 15912cd0bbcc |
| T030 | 1 | `test "$(cargo test -p lens-core --test supervisor_ingest -- --list 2>/dev/null \| rg -c '^(f31_leaked_token...` | 0 | 0 | yes | e3b0c44298fc |
| T030 | 2 | `cargo test -p lens-core --test supervisor_ingest` | 0 | 0 | yes | c7993bbf7648 |
| T030 | 3 | `cargo test --workspace` | 0 | 0 | yes | a43ccff30622 |
| T030 | 4 | `rg -n "ipc::\|supervisor::" src-tauri/src` | 1 | 0 | NO | 15912cd0bbcc |
| T031 | 1 | `test "$(cargo test -p lens-core --test ipc -- --list 2>/dev/null \| rg -c '^(f23_unc_wsl_localhost_path_inv...` | 0 | 0 | yes | e3b0c44298fc |
| T031 | 2 | `cargo test -p lens-core --test ipc` | 0 | 0 | yes | 3b01628b6e17 |
| T031 | 3 | `cargo test --workspace` | 0 | 0 | yes | 1985e13d3872 |
| T031 | 4 | `rg -n "ipc::\|supervisor::" src-tauri/src` | 1 | 0 | NO | 15912cd0bbcc |
| T032 | 1 | `test "$(rg -U -o 'generate_handler!\[[^]]*\]' src-tauri/src/main.rs \| tr -d ' \r\n' \| sed 's/,\]/]/')" = ...` | 0 | 1 | NO | e3b0c44298fc |
| T032 | 2 | `node -e "const c=require('./src-tauri/capabilities/default.json');const p=c.permissions.map(x=>typeof x==='...` | 0 | 0 | yes | e3b0c44298fc |
| T032 | 3 | `node -e "const c=require('./src-tauri/tauri.conf.json').app.security.csp;const bad=(c.match(/https?:\/\/[^\...` | 0 | 0 | yes | e3b0c44298fc |
| T032 | 4 | `cargo build -p agent-session-lens` | 0 | 0 | yes | dccad65a474a |
| T032 | 5 | `cargo clippy -p agent-session-lens -- -D warnings -A clippy::collapsible_match -A clippy::large_enum_varian...` | 0 | 0 | yes | 0674b29b50c3 |
| T032 | 6 | `npm run build` | 0 | 0 | yes | 2e1bc6b5b926 |
| T032 | 7 | `cargo test --workspace` | 0 | 0 | yes | 38156571c4fa |
| T032 | 8 | `rg -n "list_projects" src` | 1 | 0 | NO | d8641e58278e |
| T033 | 1 | `node -e 'import("./src/format.ts").then(m=>{const ok=m.relativeTime(180)==="3\u5206\u524d"&&m.relativeTime(...` | 0 | 0 | yes | e3b0c44298fc |
| T033 | 2 | `rg -n "dangerouslySetInnerHTML\|innerHTML\|get_snapshot" src src-tauri/src` | 1 | 1 | yes | e3b0c44298fc |
| T033 | 3 | `test "$(rg -U -o 'generate_handler!\[[^]]*\]' src-tauri/src/main.rs \| tr -d ' \r\n' \| sed 's/,\]/]/')" = ...` | 0 | 0 | yes | e3b0c44298fc |
| T033 | 4 | `rg -n "asl[-_]core" src` | 1 | 1 | yes | e3b0c44298fc |
| T033 | 5 | `test "$(rg -c 'export (interface\|type) (ProjectActivity\|UnifiedSession\|SourceHealth\|Settings\|IpcError\...` | 0 | 0 | yes | e3b0c44298fc |
| T033 | 6 | `npm run build` | 0 | 0 | yes | 07ab11d299da |
| T033 | 7 | `cargo build -p agent-session-lens` | 0 | 0 | yes | 73596b79c235 |
| T033 | 8 | `rg -n "\x{505C}\x{6B62}\x{4E2D}" src/App.tsx` | 0 | 0 | yes | c38c4c6eabd8 |
| T033 | 9 | `rg -n "\(WSL: " src/App.tsx` | 0 | 0 | yes | 074dd0e1e82e |
| T033 | 10 | `rg -n "\x{66F4}\x{65B0}\x{4E2D}" src/App.tsx src/format.ts` | 0 | 0 | yes | 67710c514904 |
| T033 | 11 | `cargo test --workspace` | 0 | 0 | yes | 4ec3aedc6ebe |
| T033 | 12 | `rg -n "fn (every_f_id_maps_to_existing_test_fn)\b" crates` | 1 | 0 | NO | b48be8959f65 |
| T034 | 1 | `test "$(cargo test -p lens-core --test traceability -- --list 2>/dev/null \| rg -c '^(every_f_id_maps_to_ex...` | 0 | 0 | yes | e3b0c44298fc |
| T034 | 2 | `cargo test -p lens-core --test traceability` | 0 | 0 | yes | 118be7f8e5d6 |
| T034 | 3 | `cargo test --workspace` | 0 | 0 | yes | 330b7da525bd |
| T034 | 4 | `cargo clippy --workspace --all-targets -- -D warnings -A clippy::collapsible_match -A clippy::large_enum_va...` | 0 | 0 | yes | 3fb4c357e099 |
| T034 | 5 | `npm run build` | 0 | 0 | yes | bcee1121852e |

<!-- BEGIN final-state-verification (generated by scripts/verify-final-state.py) -->

## Final-state re-baselined verification

Generated by `python scripts/verify-final-state.py --write-md` (stdlib only; plan commands run via Git Bash with `PYTHONIOENCODING=utf-8`). The pinned plan JSON is untouched. A command passes if its exit code equals the planned one (match) or it mismatches but a mechanical proof shows a later task superseded it (superseded). Script exit code at generation: **0**.

Totals: 181 commands, 145 match, 36 superseded-with-proof, 0 unexplained.

Proof rules: `rg` checks = the same rg (run with `--sort path --with-filename`) now exits 0 and every matching file is claimed by a later task's `changes[]`; T001 lib check = `cargo metadata` lib target is `lens_core` (T002 removed the alias); T032 handler check = `generate_handler!` has exactly 5 commands without `get_snapshot` (T033 removed it). sha256 is over the proof assertion output (sorted, deterministic). Proof exit is 0 for every row.

| Task | # | Exit (planned -> now) | Proof | Proof exit | Proof output sha256 | Evidence |
|---|---|---|---|---|---|---|
| T001 | 1 | 0 -> 1 | cargo metadata lib name | 0 | `a0bcba62927b153c0eec968a5bb8a5f537778f7e4cb5d36f2116407e53727289` | lib target(s) ['lens_core'] per cargo metadata; T002 changes crates/lens-core/Cargo.toml: True |
| T001 | 6 | 1 -> 0 | rg matches now; owned by later task | 0 | `4ff77019e4879743edb359adec24809850d5087f03428f09b4736a042bbab6ce` | 95 hit(s); first crates/lens-core/examples/smoke.rs:5 owned by T002 |
| T002 | 7 | 1 -> 0 | rg matches now; owned by later task | 0 | `c8dd12fce765fd084b16726d4725b29e4090264e2e8e4fa1035827b9f3fae15b` | 1 hit(s); first crates/lens-core/src/consts.rs:36 owned by T003 |
| T003 | 7 | 1 -> 0 | rg matches now; owned by later task | 0 | `1b58b322a6b727eaba8846700909c5de78e8e4035f345394c13e321f3dd4cb7e` | 7 hit(s); first crates/lens-core/tests/change_detector.rs:13 owned by T004 |
| T004 | 9 | 1 -> 0 | rg matches now; owned by later task | 0 | `62c8e96b9cbef7e63a1c378baa516adffc707f370866deec36a4517bf4105a91` | 4 hit(s); first crates/lens-core/tests/aggregator.rs:53 owned by T023,T026 |
| T005 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `04bd12fddcf41e56809a2d85770c6a6f14c1cbffadfdae4cccaf09e595fd028c` | 4 hit(s); first crates/lens-core/tests/claude_parse.rs:51 owned by T012,T018 |
| T006 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `1e4a81299f4355cc488300ed4cb58cad39caf0d6db9b06e3da7aca3958828d18` | 5 hit(s); first crates/lens-core/tests/aggregator.rs:53 owned by T023,T026 |
| T007 | 6 | 1 -> 0 | rg matches now; owned by later task | 0 | `b9eb4da9c4913a437824af8a0273473e16f3f704c6f010d76a7d71931198419e` | 1 hit(s); first crates/lens-core/tests/aggregator.rs:53 owned by T023,T026 |
| T008 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `2f3ca7577f1c263c14b8183abe7eb3540166238e02bdaa597a1dd7c17202f104` | 2 hit(s); first crates/lens-core/tests/supervisor_lifecycle.rs:77 owned by T027 |
| T009 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `e67619b6da833c5fd796f3bfefbb7d5fa6dedf693e58298f06e0c8aa076a4354` | 3 hit(s); first crates/lens-core/tests/aggregator.rs:53 owned by T023,T026 |
| T010 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `c58c700d2de545a6aa0a05a7fbb199df8bdf681ee87cfd5f5bdff33f548b89ac` | 4 hit(s); first crates/lens-core/tests/aggregator.rs:53 owned by T023,T026 |
| T011 | 6 | 1 -> 0 | rg matches now; owned by later task | 0 | `e6271b210c828d993c14118aa02e5fc0f19913521968643db4871eaf61f34a4a` | 2 hit(s); first crates/lens-core/tests/distro.rs:55 owned by T017 |
| T012 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `dc14cc679db6cf68c7b7cac2f7e4edf81563a6758397868c5be9e4e58f1fd5f8` | 1 hit(s); first crates/lens-core/tests/claude_parse.rs:169 owned by T018 |
| T013 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `cadcc20615af746c6f63d9792b2c1c7bcc300acd6410f0466ae0571cd5834800` | 1 hit(s); first crates/lens-core/tests/codex_parse.rs:143 owned by T019 |
| T014 | 6 | 1 -> 0 | rg matches now; owned by later task | 0 | `d66527ddbd0991483754207f7ca7c58c04e8369acf923a1bbd0ad3a3d225afd1` | 1 hit(s); first crates/lens-core/tests/initial_scan.rs:63 owned by T022 |
| T015 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `b0a690dca7c662f981c2cb3b762a132babb177a4cb0afec9d57d89d78201d250` | 1 hit(s); first crates/lens-core/tests/ndjson.rs:67 owned by T020 |
| T016 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `bcec89bfeb4a29b210277380bd94597250860d98d6eefc56cab6c0b0883ccfe8` | 2 hit(s); first crates/lens-core/tests/ipc.rs:136 owned by T028,T031 |
| T017 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `bcec89bfeb4a29b210277380bd94597250860d98d6eefc56cab6c0b0883ccfe8` | 2 hit(s); first crates/lens-core/tests/ipc.rs:136 owned by T028,T031 |
| T018 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `7a46b18a0f6bd0bbdb19d7d94813d738fcc0a28072b90da84d1c419f02b13418` | 1 hit(s); first crates/lens-core/tests/status_table.rs:46 owned by T021 |
| T019 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `7a46b18a0f6bd0bbdb19d7d94813d738fcc0a28072b90da84d1c419f02b13418` | 1 hit(s); first crates/lens-core/tests/status_table.rs:46 owned by T021 |
| T020 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `49344c5493fc47c8f7cf9f1129bf8e5c4187abcd70ea33ae92c77a82640c7f3f` | 1 hit(s); first crates/lens-core/tests/supervisor_lifecycle.rs:77 owned by T027 |
| T021 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `510b3dbdae2dc494a5fdf1df7c3f801e2fa3543a2755344559ad1e5216c3afd8` | 2 hit(s); first crates/lens-core/tests/aggregator.rs:53 owned by T023,T026 |
| T022 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `5d1cd36b184583937ed4ebd37a450c63a5b3f1e0a0375e33f9882b5d44072f22` | 3 hit(s); first crates/lens-core/tests/aggregator.rs:53 owned by T023,T026 |
| T023 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `3aaf15b0c1a8d248f8011ee4f10e9d5635052d0b30136c485c0794149d1d1bdf` | 1 hit(s); first crates/lens-core/tests/aggregator.rs:184 owned by T026 |
| T024 | 6 | 1 -> 0 | rg matches now; owned by later task | 0 | `b48be8959f65517b1ebd869c194675e3ae91c740e60b1b13044087f4402355ae` | 1 hit(s); first crates/lens-core/tests/traceability.rs:55 owned by T034 |
| T025 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `b48be8959f65517b1ebd869c194675e3ae91c740e60b1b13044087f4402355ae` | 1 hit(s); first crates/lens-core/tests/traceability.rs:55 owned by T034 |
| T026 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `fc12d4b62e67e669921c7b28e6ecd2712cd6da59b9e22ea9901af88c35f78a87` | 2 hit(s); first crates/lens-core/tests/ipc.rs:70 owned by T028,T031 |
| T027 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `0ceedd363104cbb8d191aa1866719d5b3a3976b0d0f210885d812345245ce6b7` | 2 hit(s); first crates/lens-core/tests/supervisor_exit.rs:91 owned by T029 |
| T027 | 5 | 1 -> 0 | rg matches now; owned by later task | 0 | `fe7ffd88dc9ac91403c8baaca55fc45bb181f3f49d2d25b479e366f5ebe7b855` | 10 hit(s); first src-tauri/src/main.rs:9 owned by T032,T033 |
| T028 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `c2a220a0853c9a1c42c1d0a45d6c0d03a3c967483b38da42103deb2c4609a840` | 1 hit(s); first crates/lens-core/tests/ipc.rs:136 owned by T031 |
| T029 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `fe7ffd88dc9ac91403c8baaca55fc45bb181f3f49d2d25b479e366f5ebe7b855` | 10 hit(s); first src-tauri/src/main.rs:9 owned by T032,T033 |
| T030 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `fe7ffd88dc9ac91403c8baaca55fc45bb181f3f49d2d25b479e366f5ebe7b855` | 10 hit(s); first src-tauri/src/main.rs:9 owned by T032,T033 |
| T031 | 4 | 1 -> 0 | rg matches now; owned by later task | 0 | `fe7ffd88dc9ac91403c8baaca55fc45bb181f3f49d2d25b479e366f5ebe7b855` | 10 hit(s); first src-tauri/src/main.rs:9 owned by T032,T033 |
| T032 | 1 | 0 -> 1 | generate_handler set | 0 | `1c133f7d397756ca16b771141a0bd22f6b7a695042a755d1009f3b8f07ab879e` | generate_handler has 5 commands (list_projects,get_project_detail,get_health,set_distro_enabled,set_source_root), no get_snapshot; T033 claims the removal: True |
| T032 | 8 | 1 -> 0 | rg matches now; owned by later task | 0 | `d8641e58278e6ab49c52040bd128ea800180a80e95a3e94133ad4414faacdfe2` | 1 hit(s); first src/App.tsx:116 owned by T033 |
| T033 | 12 | 1 -> 0 | rg matches now; owned by later task | 0 | `b48be8959f65517b1ebd869c194675e3ae91c740e60b1b13044087f4402355ae` | 1 hit(s); first crates/lens-core/tests/traceability.rs:55 owned by T034 |

## Unplanned path-set comparison

Fileset (77 files, excluding target/, node_modules/, dist/) minus the union of plan `tasks[].changes[].path` (60 distinct paths) leaves 18 path(s); the script fails if any lacks a justification.

| Path | Justification |
|---|---|
| `Cargo.lock` | generated by cargo (never hand-edited) |
| `crates/lens-core/src/runtime.rs` | v3-origin file carried along by T001's whole-tree git mv; not edited by any planned task (clippy-lint fixes only) |
| `crates/lens-core/src/util.rs` | v3-origin file carried along by T001's whole-tree git mv; not edited by any planned task (clippy-lint fixes only) |
| `docs/architecture-merged.md` | design documents (inputs to the plan, not code) |
| `docs/architecture-v2.md` | design documents (inputs to the plan, not code) |
| `docs/architecture-v3.md` | design documents (inputs to the plan, not code) |
| `docs/architecture-v4.md` | design documents (inputs to the plan, not code) |
| `docs/architecture-v5.md` | design documents (inputs to the plan, not code) |
| `docs/architecture.md` | design documents (inputs to the plan, not code) |
| `docs/gougi/2026-10-17-architecture-v1-v2-review.md` | design review notes (inputs to the plan, not code) |
| `docs/implementation-v5.md` | implementation summary doc (mentions verification script and new tests) |
| `docs/knowledge-kit-usage.md` | knowledge-kit usage doc (tooling, not product code) |
| `docs/plans/implementation-status-v5.md` | status doc (this script generates its final-state section) |
| `docs/plans/plan-architecture-v5.json` | the pinned plan itself (read-only input of this script) |
| `docs/specification.md` | design document (input to the plan, not code) |
| `scripts/verify-final-state.py` | this verification script (new, out of plan) |
| `src/format.test.ts` | node:test unit tests for src/format.ts (new, out of plan) |
| `tsconfig.json` | excludes src/**/*.test.ts from tsc (no node types in project) |

<!-- END final-state-verification -->
