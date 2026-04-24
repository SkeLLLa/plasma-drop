.PHONY: build fmt fmt-check lint clippy test doc check

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
