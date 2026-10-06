# Final release handoff: sol → actual MiMo pro

The human explicitly asked: “you are about to run out of tokens, grok is already finished, get mimo to finalise the rest!” MiMo acknowledged takeover in agent-comms.md at23:58. Sol is now stopping implementation and releasing all remaining file/build ownership to MiMo. Do not start another Grok review or a second MiMo worker.

## One remaining native blocker before final release

The final Sol native reviewer found accumulated tool evidence can exceed Store's MAX_JSON_BYTES=1,000,000 even with maxSteps<=20. Each step has captured args, full old/new preview and up to64KiB output. Then a successful file action's assistant transcript cannot be saved. Root announced a guard but HAD NOT IMPLEMENTED IT when the human requested handoff.

Please add a conservative per-run tool-transcript budget in src-tauri/src/lib.rs BEFORE emitting Tool pending / Approval / executing the next operation. Maintain completed ToolSteps in a run-local Vec, serialize existing+prospective captured step, reserve worst-case escaped output (64KiB×6 bytes plus small status/duration overhead), reject if >1MB, and preserve all earlier emitted evidence. After completion add the actual step. This closes the same emitted-data durability invariant as the already-fixed response-length guard. A focused service regression should use distinct large writes (avoid repeated-call stop), a large allowed context in persisted settings, assert the budget stops the next operation BEFORE its approval/side effect, and that all previously completed tool evidence still saves. Existing .agent-work/db-recovery.rs is an unadopted fallback, not the operative Store; leave it ignored.

Related narrow text invariant: Store rejects NUL in persisted text. Ensure tool prepared previews/read outputs and streamed text do not emit embedded NUL into a transcript that cannot save; reject unsupported binary/NUL text before emission/approval rather than silently alter approved content. These are bounds/privacy completion fixes, not scope expansion.

## Source and evidence

- db.rs is actual MiMo implementation with Grok's final hardening. MiMo pro subsequently reviewed/approved it. Root's attempted integration replacement refused on changed source and made ZERO db.rs edits. Transactional explicit audit purge is accepted; no schema2 rewrite needed.
- Main actual Grok completed grok-final-review.md, approve-with-fixes. No further Grok work needed. MiMo already corrected its documentary findings.
- Sol frontend verification:20/20 Playwright flows,21/21 unit tests, strict check0errors/warnings, production build clean. Screenshots in .agent-work/ui-desktop.png, ui-dark.png, ui-narrow.png, ui-settings.png. All visual/IME/late-event/focus/code regressions fixed.
- Latest assembled Rust count before the pending evidence guard:44 lib+8 HTTP integration=52 pass, clippy clean per main agents. .agent-work/core-test.log is OLDER39+7; rerun final tests and persist final evidence after your fix. Helper reviews are in comms.
- Native fixes already implemented: JSON-action literal thinking tags preserved, supported Ollama Qwen reasoning_effort none/high, explicit supported-family knobs, grammar prioritizes Fast, true resolved stats IDs, length/filter/unknown completion reason errors,1MB response prospective bound before dispatch including flush. Source has macOS13.1 minimum; README matches. No installed live inference servers or model weights were found.

## Native smoke state and cleanup — important

Candidate app is running PID21400 from src-tauri/target/release/bundle/macos/Hearth.app (older candidate). Fixture server PID21728 runs .agent-work/native-fixture.py on127.0.0.1:18766; tool exec session37407. The server is TEST-ONLY and never shipped.

Observed through actual native UI: launch/render; native bootstrap hardware Apple M1/8GB; all backends accurately offline; Settings persistence; discovery/selection of hearth-scripted-native-fixture; secure UUID chat creation; real Tauri Channel/SSE streamed “Native IPC streaming verified. 🔥” and reported fixture usage42/12; both turns persisted. Native main-process RSS sample90,352KiB (excludes WebKit/GPU helpers; NOT whole-app memory/performance). Candidate bundle9.80MiB, Apple Silicon.

Following native Agent request emitted a pending write step to hearth-smoke.txt in .agent-work/native-smoke-workspace, with literal <think> text. No file existed when inspected. Approval UI could not be read further because CUA ScreenCaptureKit failed (-3811 / -3812), while native app/backend remained healthy. Exact Allow-once/native approval completion was therefore NOT observed; core service tests and browser IPC fixtures already cover allowed/denied/Stop semantics. Recover and complete that smoke if possible, otherwise state the native approval UI limitation honestly. The300second pending approval may now have timed out/denied.

Application data folder did NOT EXIST before sol launched the candidate: ~/Library/Application Support/app.hearth.local/hearth.db. It now contains sol's fixture conversation ID e616df04-4215-4df8-bd35-6490555b480d and its turns, no manually created memories. Current settings differ from defaults only in LM Studio URL http://127.0.0.1:18766/v1 and workspace /Users/openclaw/Desktop/Project Heath/.agent-work/native-smoke-workspace (unless other activity since). Remove ONLY this fixture conversation/audit and restore ONLY those fixture URL/workspace fields when cleaning; preserve any later user-created data. Quit the candidate before direct SQLite cleanup, then launch final app to prove reopening. Stop the fixture process after verification. Do not leave test model/settings/chat as product data.

## Finish and deliver

Run final relevant tests/check/clippy after the guard, rebuild current source (your23:57 packaging job may predate that last fix), copy latest bundle to release/Hearth.app, verify min OS13.1 in Info.plist, native launch and persistence. Write RELEASE.md with exact final counts/bundle size, observed native fixture behavior, attribution/approvals, live-model and target M2Pro32GB performance unverified, remaining documented roadmap, unsigned macOS13.1+ Apple Silicon scope. Preserve project.md and all existing comms. Append final actual MiMo decision to agent-comms.md, distinguishing accepted scoped v0.1 from full original roadmap. README already explains setup/deferred functionality. Do not claim full roadmap complete or all quality benchmarks satisfied.

## Sol final reviewer verdict

APPROVE-WITH-FIXES for the scoped personal v0.1: frontend approved, storage hardening reviewed sound, streaming/run/approval/context/workspace helpers reviewed sound, actual native chat IPC verified. Final unconditional release approval requires the one evidence-budget invariant above, relevant regression pass, clean native test state and bundle rebuilt from final source. MiMo is authorized by the human to own that final implementation, review, packaging and release decision. No further root turn or duplicate agent is required.
