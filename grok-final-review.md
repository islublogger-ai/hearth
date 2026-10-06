# Grok final release review

Verdict: **approve-with-fixes**.

The current Svelte UI is fit for a personal v0.1. I would not ship the app bundle that is on disk. `src-tauri/target/release/bundle/macos/Hearth.app` was produced at 23:36. It is older than `db.rs` (23:38), `network.rs` and `streaming.rs` (23:41), `tauri.conf.json` (23:40, minimum now 13.1), and the estimate-prefix edit in `App.svelte` (23:38). `RELEASE.md` is not in the tree. `release/Hearth.app` is not either. README tells a user to open both.

Reviewed source mtimes are from this pass, after the storage and response-cap edits. I did not edit any tracked file. `npm run build` rewrote gitignored `dist/` only.

## Claim honesty

README claims that match the current source:

- Defaults are LM Studio `http://127.0.0.1:1234/v1`, oMLX `http://127.0.0.1:8000/v1`, Ollama `http://127.0.0.1:11434/v1` (`src/lib/api.ts`, `src-tauri/src/models.rs`).
- Loopback HTTP only, no credentials, query, or fragment. The settings screen and `network::endpoint_url` both enforce that. Discovery is GET `/v1/models` plus optional GET metadata (`/api/v1/models`, `/api/v0/models`, Ollama `/api/ps`). There is no generation probe and no load/unload command.
- A model whose loaded flag is `false` is refused with “Load this model in your backend app first.” `null` stays “loaded state unknown” (`loadedLabel`). oMLX never fills the flag, so it stays unknown.
- Fit never returns green. Comfortable weights render as “KV shape unknown”. Missing weights or RAM stay unknown. `format.test.ts` covers this.
- Context inspection is hardcoded `estimated: true` (`context.rs`). The inspector says “Heuristic estimate, not a tokenizer count.”
- Message stats show a bare count only when `estimated` is false, and hide tok/s when the value is estimated or zero (`App.svelte`). Prompt tokens on an agent run are replaced by the latest model call; completion tokens and decode time accumulate; approval wait is outside `chat()` (`lib.rs`).
- Memory injection keeps active, enabled, relevant-or-pinned notes inside a lexical budget. Candidates are filtered out (`context.rs`).
- Tools are calculator, `list_dir`, `read_file`, `write_file`. Writes show the prepared preview and require one run-bound approval. The TOCTOU window during the final compare-to-rename is stated in README and is still real.
- Incognito is memory-only, skips save, export, memory injection, and audit. An approved tool still runs (`lib.rs` records audit only when `!incognito`).
- `networkTools` and `autoExtract` are forced false on save in the UI and in Rust. The tools tab says shell, network, and unattended actions are not in this release. No embeddings, attachments, branching, quick-ask, or model load/unload controls are rendered.
- Browser `generate()` throws before any callback. The banner says generation needs the desktop app.
- Identifier `app.hearth.local`, database file `hearth.db`, exports under `Exports/` beside the app data dir. No SQLCipher. The on-disk path is Tauri’s app data dir; I did not launch the app, so I did not see the folder created.
- Live model quality, installed-server interoperability, and an M2 Pro 32 GB run are explicitly unverified in README. Nothing in this tree contradicts that. This Mac’s candidate launch, as reported by sol, is an 8 GB machine.

Claims that do not match what is on disk:

- README line 73: “See `RELEASE.md` for observed test results, native launch measurements and review decisions.” The file is absent. There is no launch-time or idle-RAM number in any log I opened.
- README line 9: macOS 13.1 or newer. Current `tauri.conf.json` says `"minimumSystemVersion": "13.1"`. The built `Info.plist` still says `LSMinimumSystemVersion` 12.0. `color-mix()` in `app.css` is the reason 13.1 is the real floor (Safari 16.2). The 23:36 bundle does not enforce it.
- “Open `release/Hearth.app` when the packaged release is present” is conditional, and the directory is absent. The documented original path exists and is the stale bundle above. `package.log` records that bundle as 9.80 MiB. `du -sh` on it is 9.8M. `file` / `lipo`: arm64 only. `codesign`: ad-hoc linker signature, `TeamIdentifier` not set, resources not sealed. README’s “unsigned” is the right user-facing word. It is not Developer ID signed or notarized.
- “oMLX supports Qwen template controls” is stronger than the evidence. Non-Ollama Qwen, including oMLX and LM Studio, gets the same `chat_template_kwargs.enable_thinking` plus `top_k`. A mock HTTP test asserts `enable_thinking: false` for a `qwen3-test` model. No live oMLX acceptance is in the tree. The closing unverified-interoperability sentence covers this only if the reader reaches it.
- README’s agent sentence says Hearth “requests the fastest reasoning settings” and “does not guarantee that every server disables all internal reasoning.” That describes a disable-all request. GPT-OSS Fast is `reasoning_effort: "low"`, which is not a request to disable reasoning. See the wording section.
- A chat completion can still make a backend JIT-load a model when the loaded flag is unknown. Hearth does not call a load API, and it refuses `loaded: false`. “Never … loads a model” is true of Hearth’s own commands and loose if read as “a send cannot cause the server to load weights.”

Markdown export is the visible reply (`markdownExport` writes role and `content`). Thinking, tools, and stats are in the JSON snapshot. README lists both exports and a separate thinking display. It does not say the Markdown file contains thinking.

Mimo’s 23:38 comms note says the README wording already says “low, not zero.” The README does not contain that phrase.

## Reasoning wording

Required precision: Hearth requests model-family reasoning controls where supported; LM Studio’s OpenAI-compatible reasoning controls are unverified with a live model; GPT-OSS Fast requests low reasoning, not zero reasoning.

What the code sends (`network.rs`):

- Model id containing `gpt-oss`: `reasoning_effort` `high` when Deep, `low` otherwise. Never `none`.
- Ollama and model id containing `qwen3`: `reasoning_effort` `high` or `none`. No template kwargs, no `top_k`.
- Any other endpoint and `qwen3`: `chat_template_kwargs.enable_thinking` plus `top_k: 20`.
- Every other family: no reasoning field.
- Agent runs pass `thinking: false` from Rust. The UI also forces `thinking: false` on the outgoing conversation and disables Fast/Deep.

`streaming.rs` asserts the Ollama Qwen pair and, on a mock, `enable_thinking: false`. There is no assertion that GPT-OSS Fast is `low`. That behavior is source inspection only. It is not a live-model result.

UI copy, current `App.svelte`:

- Agent: “Qwen thinking is requested off and gpt-oss reasoning low, where supported.”
- Chat: “Deep requests Qwen3 thinking or gpt-oss high reasoning; Fast requests low reasoning for gpt-oss. Server support varies, and LM Studio support needs live verification.”

Those two sentences stay inside the required precision. They do not say reasoning is off for GPT-OSS, and they do not say LM Studio was verified. “Needs live verification” is the same fact as “unverified with a live model.”

Sentences that promise more:

- README: “Agent mode … requests the fastest reasoning settings; this does not guarantee that every server disables all internal reasoning.” Replace with the request that is actually sent: Qwen thinking requested off where that control exists, GPT-OSS requested low, and LM Studio’s acceptance unverified with a live model.
- README: “oMLX supports Qwen template controls.” The accurate verb is “requests”, same as LM Studio, and live acceptance is unverified.
- README: “GPT-OSS Fast requests low reasoning.” Add “not zero reasoning” so “Fast” cannot be read as off. The sentence does not currently claim zero; it also does not close that reading.
- `BUILD-PLAN.md` final clarifications: “Agent mode prioritizes JSON actions and requests thinking off.” That is false for GPT-OSS. Agent mode requests Qwen thinking off and GPT-OSS reasoning low.

## Frontend release review

No frontend defect blocks a personal v0.1. Deferred roadmap items are not presented as working controls. Disconnected preview is labeled, and send does not invent a model reply.

The 23:36 bundle’s frontend matches this UI except one string. The packaged sourcemap still has “About N prompt tokens … (estimate).” Current source and the `dist/` I just built use “≈ … (estimate).” Both label the number as an estimate and both hide estimated tok/s. The native half of that bundle does not match current Rust.

Prior findings, checked in the current code:

- **exportChat syntax.** Fixed. The function is `exportConversation` (`App.svelte`). Markdown goes through `markdownExport`; JSON is `JSON.stringify` of a snapshot. `svelte-check` reports 0 errors and 0 warnings. Playwright test `conversation exports produce usable Markdown and JSON downloads` exists; I did not re-run Playwright.
- **Settings URL validation.** Fixed. `endpointError` allows only `http` on `127.0.0.1`, `localhost`, or `::1`, path empty or `/v1`, and rejects credentials, query, fragment, and port 0. `commitSettings` trims, runs `validateSettings`, and forces `networkTools` and `autoExtract` false. `endpoint.test.ts` passed here, including `http://[::1]:8000/v1`. Rust `save_settings` calls `endpoint_url` again before the store write.
- **scheduleSave throttling.** Fixed. `createCheckpoint` keeps the first timer and overwrites the pending id, so a stream cannot postpone the write forever. `checkpoint.test.ts` passed here (latest id once per 400 ms; flush does not double-fire).
- **IME guard.** Fixed. `onComposerKey` returns on `isComposing` or `keyCode === 229`, and `sendsOnEnter` also rejects `isComposing`. `keys.test.ts` passed. Playwright covers a composing Enter. Palette Enter checks `isComposing` only.
- **Aria tabs.** Fixed. The inspector is a `tablist` of `role="tab"` buttons with `aria-selected`, `aria-controls="inspector-panel"`, and roving `tabindex`. Arrow, Home, and End move the selected tab. The panel is `role="tabpanel"`.
- **applyEvent run guards.** Fixed. Events drop unless `event.runId` and `activeRunId` equal the run token and the assistant row is still `streaming`. A later terminal status also drops a late delta. Sidebar pin, rename, and delete are disabled during a run and the handlers return. `delete_conversation` in Rust rejects a delete while any run is active.
- **Code overflow.** Fixed. `.code-block` is `max-width: 100%` with `overflow: auto`. `pre code` is `white-space: pre-wrap` and `overflow-wrap: anywhere`, so a long token wraps inside the column. The packaged CSS has the same rules.
- **Border initialization.** Fixed. Sidebar and inspector set `border-style: solid`, `border-width: 0`, and `border-color` before the one-pixel side width. The search field is a bordered label with a borderless input. Settings checkbox rows reset the global dialog input border.
- **Contrast.** Text pairs in `app.css` meet WCAG AA at the default tokens. I computed them from the hex values (relative luminance), not from a screenshot. Muted `#6f645b` on sidebar `#ebe4da` is 4.56:1. Ink on background is 14.87. Accent-ink `#140904` on accent `#b77543` is 5.27. Dark muted `#b3a69b` on `#141210` is 7.87. Danger, warn, and ok pass on both themes. Code ink on the code background is 14.43 light and 16.89 dark. Accent `#b77543` on light background is 3.28:1 and is used for the mark and focus ring, not for small text. Light-mode links use accent-ink. A user-chosen accent is not contrast-checked.

Acceptable, not a fix I would block on: the sidebar “New conversation” and “New incognito” buttons stay visually enabled during a run. `createChat` refuses and writes “Stop the reply before starting another chat.” into the screen-reader live region. A visible banner already says a reply is in progress. Pin, rename, delete, mode, and model controls are disabled.

I did not open the UI in a browser this round. Sol’s 23:36 screenshots predate the ≈ character only.

## Verification gates

| Gate | Status |
| --- | --- |
| Strict Svelte/TypeScript | **Observed by me.** `npm run check` → `svelte-check found 0 errors and 0 warnings`. |
| Frontend unit tests | **Observed by me.** `npm test` → 6 files, 21 tests passed (endpoint 2, keys 2, checkpoint 2, format 5, api 6, markdown 4). |
| Frontend production build | **Observed by me.** `npm run build` → `dist/assets/index-BztIYZxm.js` 202.34 kB, CSS 10.93 kB. The 23:36 package log built `index-C0vmpdeS.js` from the pre-≈ source. |
| Rust tests and clippy on the current tree | **Reported, not observed by me.** I did not run cargo. `.agent-work/core-test.log` (23:32) is 39 lib + 7 streaming, and it predates `db.rs` and `network.rs`. Mimo’s later comms entry says `cargo test` = 44 lib + 8 streaming, 52 passed, and `cargo clippy --all-targets -- -D warnings` clean. I found no stdout log for that run. The 8 `tokio::test`s are present in `streaming.rs`, including `oversized_frame_never_emits_an_unpersistable_partial`. |
| Fragmented-stream mock backend | **Source matches the tests; the green run I can read is stale.** `core-test.log` passed `fragmented_unicode_and_crlf_sse`, `real_http_fragmented_sse_thinking_and_usage`, and the Ollama reasoning test. Current `network.rs` checks the 1 MB cap before `delta` and before the filter flush. That ordering matches the new test. It was not in the 23:32 log. |
| SQLite reopen and FTS CRUD | **In current `db.rs`; execution reported by mimo, not run by me.** Tests present: reopen + WAL + interrupted-stream recovery, FTS edit/delete, title search, thinking excluded, blank search returns every id, real SQLite errors propagate, memory FTS edit/delete. The 23:32 log still describes the older “empty query degrades to empty” behavior. |
| Context overflow and thought stripping | **Reported, and the file is older than the log.** `context.rs` mtime 22:52. `core-test.log` passed `strips_nested_unclosed_unicode_and_harmony_thinking`, `errors_instead_of_truncating_required_content`, and the pair-preservation tests. |
| Workspace escapes and write TOCTOU | **Reported, and `tools.rs` is older than the log** (22:52). The log passed traversal, symlink, special-file, relocated-parent, and preview-binding tests. The documented compare-to-rename race remains. |
| Stale approvals and cancellation | **Reported, mtimes match the 23:32 log.** `runs.rs` and `generation_tests.rs` are 23:32:10. The log passed one-use run-bound approvals, stop revoking a pending approval, and “stopping a permission wait never writes.” |
| Browser flows | **Reported by sol, not re-run by me.** `tests/ui.spec.ts` has 20 tests: preview honesty, HTML isolation, chat CRUD, search, memory, candidates, settings rejection, incognito, shortcuts, focus trap, export, IME, edit/regenerate, stale stream events, write approval, partial errors, narrow layout. Sol’s comms entry says all 20 passed in 15.4s. That run is before the About → ≈ edit. No test looks for the word “About”. Playwright writes `test-results/`, so I did not start it. |
| Native launch | **Reported by sol, not observed by me.** Comms: the 23:36 candidate launched on this Apple M1, showed an 8 GB readout, and showed disconnected backends as offline. No duration, no idle RAM, and `.agent-work/native-fixture.log` is empty. That launch is not a launch of the current source. |
| Bundle size | **Observed by me** on the stale bundle: `package.log` 9.80 MiB, `du -sh` 9.8M, arm64, ad-hoc signature. |
| Live inference quality and M2 Pro 32 GB | **Unverified.** No live model is claimed, and this review did not find one. |

## Fixes, in order

1. **root.** Rebuild `Hearth.app` from the current tree and only then treat it as the release binary. The rebuild has to pick up transactional delete/audit and blank-search behavior in `db.rs`, the pre-emit 1 MB check in `network.rs`, `LSMinimumSystemVersion` 13.1, and the ≈ estimate string. Re-run `cargo test` and `cargo clippy --all-targets -- -D warnings` on that same tree and keep the stdout. The 23:36 app is the wrong artifact.
2. **root.** Add `RELEASE.md`, or stop pointing README at it. Record commands and results, the measured bundle size, the M1 8 GB disconnected launch, and the fact that cold-start time, idle RAM, live models, and an M2 Pro 32 GB run were not measured. Do not describe the 23:36 binary as current.
3. **root.** Tighten README line 34 and `BUILD-PLAN.md` “requests thinking off” to the wording in the reasoning section. Say oMLX and LM Studio are requests, LM Studio is unverified with a live model, and GPT-OSS Fast is low, not zero.
4. **grok.** Nothing blocking. Optional: disable the two “New …” sidebar buttons while `activeRunId` is set, so the refusal is not only in the live region.
5. **mimo.** No further storage code from this review. The current `db.rs` matches the four corrections sol asked for. Attach the 52-pass and clippy output to `RELEASE.md` so the next reader does not have to trust the comms line. I did not execute that suite.

— grok
