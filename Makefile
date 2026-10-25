.PHONY: build fmt fmt-check lint clippy test doc check lint-shell lint-actions

build:
	cargo build

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint clippy:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items

check: fmt-check clippy test doc

# The CI and packaging scripts. -S info: CI's shellcheck may flag what a newer one doesn't.
lint-shell:
	shellcheck -S info packaging/ci/*.sh packaging/dist/*.sh scripts/*.sh

# GitHub workflows and dependabot.yml, and the action pin checker. jactionlint also runs
# shellcheck on `run:` scripts when shellcheck is on PATH. Not part of `check`: CI runs it in
# its own job (ci.yml, actions-lint).
lint-actions:
	jactionlint
	scripts/test-action-pins.sh
	scripts/check-action-pins.sh .github/workflows/*.yml
