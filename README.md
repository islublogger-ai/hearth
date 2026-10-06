# Hearth

A native macOS home for local conversations, visible memory and small, approval-based agent tasks. Hearth connects to an inference server you run in LM Studio, oMLX or Ollama. It does not host model weights itself.

This repository implements the scoped v0.1 personal release agreed in [BUILD-PLAN.md](BUILD-PLAN.md). The original [project.md](project.md) remains the longer-term roadmap. Reviews and decisions are in [agent-comms.md](agent-comms.md); the earlier `docs/CONTRACT.md` is a historical proposal, superseded by the build plan and the actual shared types.

## Run the app

Open `release/Hearth.app` when the packaged release is present. The build requires macOS 13.1 or newer on Apple Silicon and is unsigned; signing and notarization are separate distribution work.

To build from source on macOS, install Node.js, Rust stable and Xcode Command Line Tools, then:

```sh
npm ci
npm run desktop
```

The scripts also recognize the workspace-local Rust toolchain under `.tools/` when available. Build an app bundle with `npm run package`; its original output is `src-tauri/target/release/bundle/macos/Hearth.app`.

## Connect a local model

Start the server and explicitly load a model in your backend application. Open Hearth Settings, enable the matching endpoint, save, then rediscover servers and select a reported model. Defaults are:

| Server | Endpoint |
| --- | --- |
| LM Studio | `http://127.0.0.1:1234/v1` |
| oMLX | `http://127.0.0.1:8000/v1` |
| Ollama | `http://127.0.0.1:11434/v1` |

Only unauthenticated loopback HTTP endpoints are supported in this release. Hearth never silently switches servers, loads a model or performs a generation probe. Loaded-state metadata varies by backend; an unknown state remains unknown. Different servers hold separate copies of model weights.

Use a model and context size appropriate for your Mac. Hearth reads actual system memory and reported model metadata, but does not calculate a complete KV-cache memory requirement. Context counts are conservative estimates until the backend reports usage. A fit label cannot guarantee that inference will fit.

Fast/Deep requests family-specific reasoning controls where supported. Ollama Qwen3 uses `reasoning_effort`; oMLX supports Qwen template controls. LM Studio's OpenAI-compatible reasoning controls require live verification. GPT-OSS Fast requests low reasoning. Agent mode prioritizes valid JSON actions and requests the fastest reasoning settings (for GPT-OSS, `reasoning_effort: "low"` — low reasoning, not zero); a server may still perform internal reasoning. Backend-side templates and cache behavior are outside Hearth's request-byte stability guarantee.

## Included

- Persistent conversations, search across titles and message content, pin/rename/delete, JSON and Markdown exports, edit/regenerate, Stop and separate thinking display.
- Real SSE streaming, fragmented UTF-8 handling, partial-response errors, bounded requests, cancellation and backend usage/timing statistics.
- Manual memory creation/editing, category, pin, disable, delete/undo and candidate approval. Only relevant enabled active notes and pins enter a bounded lexical memory budget.
- Agent JSON actions: calculator, list directory, read file and write file. Choose an existing absolute workspace in Settings. File paths are anchored to that workspace; symlinks and special files are rejected. Every write shows the captured content preview and requires one exact approval for that run.
- Light/dark/system themes, keyboard navigation, command palette, context inspection and tool timeline.

Writes are atomic replacements with an approved snapshot check. An external process writing an existing file during the final comparison-to-rename interval remains a documented concurrency limitation; exclusive new-file creation rejects an existing destination.

Agent prompt-token statistics describe the latest model call; completion tokens and decode time accumulate across the run. Approval waits are excluded from throughput. Missing/estimated usage is labelled and unavailable speed is hidden.

## Data and privacy

Native data lives in `~/Library/Application Support/app.hearth.local/hearth.db` with SQLite WAL sidecars. Exports go to the `Exports/` folder beside it. The database is local and is not encrypted by Hearth. File tools operate only in the workspace you configure.

Incognito conversations stay in application memory, exclude saved memory and are neither persisted nor exported. Any explicitly approved file operation still affects its chosen workspace. Your inference server may independently log requests; Hearth does not control those logs. No analytics, cloud inference, automatic extraction, external-network tools or background model loads are included.

## Browser preview

```sh
npm run dev
```

Open `http://127.0.0.1:1420`. The preview stores conversations, memory and settings in that browser's local storage, separately from native data. Sending clearly explains that generation needs the desktop app; there is no simulated model response.

## Checks

```sh
npm run verify
npm run test:e2e
npm run test:core
npm run lint:core
```

Playwright uses `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH` when set, otherwise installed macOS Chrome or managed Chromium. Install managed Chromium with `npx playwright install chromium` if needed. Native integration tests use explicitly named HTTP fixtures; those fixtures are never shipped as inference backends.

See `RELEASE.md` for observed test results, native launch measurements and review decisions. Live model quality, target M2 Pro 32 GB performance and interoperability with installed inference-server versions remain unverified until those systems are available.

## Deferred roadmap

Embeddings/reranking/vector retrieval, automatic memory extraction and summaries, advanced model profiles and tokenizers, full raw Harmony streaming, automated model lifecycle, shell/Python/web/MCP tools, attachments, conversation branching, quick-ask, background jobs, eval-quality targets, signing and notarization remain follow-on work. The source specification describes these goals; this release does not represent them as working features.

Protocol references: [Ollama OpenAI compatibility](https://docs.ollama.com/api/openai-compatibility), [LM Studio chat completions](https://lmstudio.ai/docs/developer/openai-compat/chat-completions), [oMLX request implementation](https://github.com/jundot/omlx/blob/main/omlx/api/openai_models.py).
