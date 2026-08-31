NIX_FILES := $(shell find . -name '*.nix' -not -path './.git/*')

.PHONY: fmt fmt-check lint test pytest build venv

fmt:
	nixfmt $(NIX_FILES)
	cargo fmt

fmt-check:
	nixfmt --check $(NIX_FILES)
	cargo fmt --check

# Provision the kitest-py venv the Python-linking crates build against.
# Pin the interpreter to the nix python3 on PATH so uv does not create the venv
# from a cached managed python of a different version than maturin builds against.
venv:
	uv --directory crates/kitest-py sync --python "$$(command -v python3)"

lint: venv
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test
	$(MAKE) pytest

pytest: venv
	uv --directory crates/kitest-py run maturin develop
	uv --directory crates/kitest-py run pytest

build:
	cargo build
