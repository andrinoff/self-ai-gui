BINARY := self-ai-gui
PORT := 8090

.PHONY: ui build dev-api dev-web test test-all fmt install clean

## Build the frontend into web/dist.
ui:
	cd web && npm ci --no-audit --no-fund && npm run build

## Build the single binary (frontend embedded).
build: ui
	cargo build --release
	cp target/release/$(BINARY) ./$(BINARY)

## Run the API alone (frontend served from web/dist if built).
dev-api:
	SELF_ADDR=127.0.0.1:$(PORT) cargo run

## Vite dev server with /api proxied to the Go/Rust server on :8080.
dev-web:
	cd web && npm run dev

## Unit tests: no sockets, no network, always quick.
test:
	cargo test --lib -- --skip integration_tests

## The full suite. The integration tests each start a fake model server, and
## some environments deadlock when they run together, so run one at a time:
##   cargo test a_reply_is_streamed -- --exact --nocapture
test-all:
	cargo test -- --test-threads=1

fmt:
	cargo fmt
	cd web && npx tsc --noEmit

install: clean
	sudo ./deploy/install.sh

clean:
	rm -f $(BINARY)
