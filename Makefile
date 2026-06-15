BUILD_ENV := rust

.PHONY: lint fix test

lint:
	@cargo fmt
	@cargo clippy --all-targets --all-features

fix:
	@cargo clippy --fix --workspace --tests
	@cargo fmt --all

test:
	@cargo test --workspace --all-features -- --nocapture
