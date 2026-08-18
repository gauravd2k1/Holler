.PHONY: dev down test test-backend lint fmt check-seams

dev:
	docker compose up --build

down:
	docker compose down

test: test-backend

test-backend:
	cd backend && go test ./...

lint:
	cd backend && go vet ./...

fmt:
	cd backend && gofmt -l .

# Producer/consumer seam check (see .github/workflows/ci.yml's `rust-seams`
# job for the full rationale). The Rust crates here are NOT one cargo
# workspace: edge/*, apps/pos/src-tauri and each test bridge under tests/
# have their own Cargo.lock. So `cargo check` inside the crate you edited
# says nothing about the crates that CALL it, and a changed `*_impl`
# signature breaks its consumers silently — eight times so far, each one
# found at the tail of the slowest CI job or by hand.
#
# This target compiles every consumer that lives outside its producer's
# workspace, --all-targets so their test binaries count too. Run it after
# changing any pub signature in edge/ or apps/pos/src-tauri.
check-seams:
	cargo check --all-targets --manifest-path apps/pos/src-tauri/Cargo.toml
	cargo check --all-targets --manifest-path tests/e2e-scenario/harness/Cargo.toml
	cargo check --all-targets --manifest-path tests/integration/kds-lan-bridge/Cargo.toml
