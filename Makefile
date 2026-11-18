.PHONY: lint fix test

lint:
	@cargo fmt --all --check
	@cargo clippy --workspace --all-targets --all-features -- -D warnings

fix:
	@cargo clippy --fix --workspace --all-targets --all-features
	@cargo fmt --all

test:
	@cargo test --workspace --all-features -- --nocapture
