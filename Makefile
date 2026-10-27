.PHONY: build fmt fmt-check lint clippy test doc check lint-deny lint-shell lint-actions

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

check: fmt-check clippy test doc lint-deny

# Dependency licenses, bans, sources and advisories (deny.toml). Fetches the advisory database.
lint-deny:
	cargo deny --locked check

# The CI and packaging scripts. -S info: CI's shellcheck may flag what a newer one doesn't.
lint-shell:
	shellcheck -S info packaging/ci/*.sh packaging/dist/*.sh scripts/*.sh scripts/*.bats

# GitHub workflows and dependabot.yml, and the action pin checker (tests need bats). jactionlint also runs
# shellcheck on `run:` scripts when shellcheck is on PATH. Not part of `check`: CI runs it in
# its own job (ci.yml, actions-lint).
lint-actions:
	jactionlint
	bats scripts/check-action-pins.bats
	scripts/check-action-pins.sh .github/workflows/*.yml
