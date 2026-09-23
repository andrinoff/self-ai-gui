# self-ai-gui

A personal chat assistant that remembers you. One binary: axum (hyper +
tokio) behind the API, a React transcript in front, and SQLite underneath for
both the conversations and the long-term memory. Model-agnostic: anything that
speaks the OpenAI Chat Completions protocol answers — OpenAI, OpenRouter,
Groq, or Ollama on the same box.

```
self, a chat that keeps what matters

  What should I drink while I work?        ← you, in a highlighter wash

  The thread you started on Tuesday is     ← self, typeset like a printed
  still open; tea first, coffee after      interview, serif on paper
  lunch. ¹ ²

  ¹ Allergic to peanuts.   ² Works at the studio.   ← the notes that informed
                                                     the reply, as footnotes
```

## How the memory works

Three moving parts, all in SQLite next to the data directory:

1. **The transcript.** Conversations and messages, replayed to the model
   (newest turns first, within a character budget).
2. **The notes.** Durable facts about you: preferences, projects, people,
   routines. Each note is one short third-person sentence. Notes are written
   three ways: you add one in the memory drawer; the assistant mines the
   conversation every `SELF_MEMORY_EVERY` turns; or you press *remember* under
   any reply. Duplicates collapse by a normalized form, and anything that
   smells like a credential is refused.
3. **The injection.** Each request builds the system prompt from the persona
   plus the notes: pinned notes always, the rest ranked by relevance to what
   you just said, stopping at a character budget. The reply records which
   notes were used, which the transcript shows as footnotes — you can see what
   the answer was built on and correct it.

The persona (a fairly opinionated prompt about answering first, formatting
quietly, and using memory without reciting it) lives in
`src/prompts.rs` and is overridable per conversation or via
`SELF_SYSTEM_PROMPT`.

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `SELF_ADDR` | `127.0.0.1:8080` | Listen address. Keep it private. |
| `SELF_DATA_DIR` | `./data` | SQLite file lives here. |
| `SELF_BASE_URL` | `https://api.openai.com/v1` | Any Chat Completions endpoint. |
| `SELF_API_KEY` | *(unset)* | Or `OPENAI_API_KEY`. Local servers skip it. |
| `SELF_MODEL` | `gpt-4o-mini` | Default model. |
| `SELF_MODELS` | *(unset)* | Comma list for the picker; unset means ask the provider. |
| `SELF_MEMORY` | `true` | Turn conversation mining off entirely. |
| `SELF_MEMORY_EVERY` | `2` | User turns between mining passes. |
| `SELF_MEMORY_BUDGET` | `1200` | Characters of notes in one prompt. |
| `SELF_MEMORY_MODEL` | *(the chat model)* | A cheaper model for mining. |
| `SELF_HISTORY_MESSAGES` / `SELF_HISTORY_BUDGET` | `24` / `24000` | How much of the transcript is replayed. |
| `SELF_SYSTEM_PROMPT` | *(built-in)* | Replace the persona. |
| `SELF_USER_NAME` | *(unset)* | Your name, used in the persona. |
| `SELF_TIMEOUT` | `120` | Seconds without bytes before giving up. |

## Run it

```bash
make build                                    # UI + release binary
SELF_API_KEY=sk-… ./self-ai-gui               # http://127.0.0.1:8080
```

Ollama instead of OpenAI:

```bash
SELF_BASE_URL=http://127.0.0.1:11434/v1 SELF_MODEL=llama3.2 ./self-ai-gui
```

## Develop

```bash
make dev-api     # the Rust server on :8080
make dev-web     # Vite on :5173, /api proxied
make test        # unit tests, no sockets
make fmt         # cargo fmt + tsc
```

`make test` deliberately runs the unit tests only: each integration test in
`src/integration_tests.rs` boots a fake model server on its own thread, and
some environments deadlock when several of those run together. Run them one
at a time when you need them:

```bash
cargo test a_reply_is_streamed_stored_and_told_what_it_remembers -- --exact --nocapture
```

The streaming path was also verified against a scripted model server over
real HTTP (start → deltas → done → memory, transcript stored, notes injected
into the next prompt).

## Deploy on Ubuntu, behind Caddy

Same pattern as your other boxes: loopback + Caddy + DNS-01, nothing public.
The chat listens on `127.0.0.1:8090` so it stays out of the way of whatever
else runs there.

```bash
make build
sudo ./deploy/install.sh
```

The installer creates a `self-ai` user, installs to `/opt/self-ai-gui`, stores
the data under `/var/lib/self-ai-gui`, and writes the API key to
`/etc/self-ai-gui/env` (mode 0640, root:self-ai — it never reaches the
journal or `systemctl show`). It asks for the base URL, key and default model
as it goes, or takes them from the environment.

Then put it on your domain:

```bash
sudo ./deploy/setup-domain.sh ai.andrinoff.com
```

That appends one site block to `/etc/caddy/Caddyfile` and leaves your other
sites untouched, reusing the Cloudflare token already in the file:

```
ai.andrinoff.com {
	tls {
		dns cloudflare …
	}

	encode zstd gzip
	reverse_proxy 127.0.0.1:8090
}
```

It validates, reloads Caddy, and checks that the A record resolves to this
box's Tailscale IP. Then open `https://ai.andrinoff.com` from any tailnet
device.

## API

```
GET  /api/health
GET  /api/config                     defaults, memory settings (no secrets)
GET  /api/models                     the picker list
GET|POST      /api/conversations
GET|PATCH|DELETE /api/conversations/{id}
GET|POST      /api/conversations/{id}/messages      POST streams the reply
POST /api/conversations/{id}/remember               mine this conversation now
GET|POST      /api/memories
PATCH|DELETE  /api/memories/{id}
```

The reply stream is server-sent events: `start` (with the memories being
used), `delta` (text), `done`, then `memory` (what was just learned) or
`error`.

## Layout

```
src/config.rs     environment
src/db.rs         SQLite: conversations, messages, notes
src/prompts.rs    the persona, the memory block, the mining prompt
src/memory.rs     normalization, relevance ranking, extraction parsing
src/upstream.rs   the Chat Completions client, SSE parsing
src/api.rs        routes and the reply stream
web/              React transcript (Vite build, embedded into the binary)
deploy/           systemd unit, install.sh, setup-domain.sh
```

## Notes and limits

- No authentication. It binds to loopback and is meant to sit behind your
  tailnet, like the rest of the house.
- Memory relevance is lexical (token overlap with a recency nudge), not
  embeddings: it works with any provider, and at 5–30 notes you will not
  notice the difference.
- The mining pass costs one extra (non-streaming) call per `SELF_MEMORY_EVERY`
  turns. Point `SELF_MEMORY_MODEL` at a cheap model to make it negligible.
