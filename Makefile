NIX_FILES := $(shell find . -name '*.nix' -not -path './.git/*')

.PHONY: fmt fmt-check lint test build

fmt:
	nixfmt $(NIX_FILES)
	cargo fmt

fmt-check:
	nixfmt --check $(NIX_FILES)
	cargo fmt --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test

build:
	cargo build
