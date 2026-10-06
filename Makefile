NIX_FILES := $(shell find . -name '*.nix' -not -path './.git/*')

.PHONY: fmt fmt-check lint test pytest build venv stubs stubs-check

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

# `uv run` auto-syncs before each command, which reinstalls a cached build of
# the kitest package (cache keyed on the unchanging 0.0.0 version) and clobbers
# the fresh extension maturin just built. --no-sync keeps maturin's install.
pytest: venv
	uv --directory crates/kitest-py run --no-sync maturin develop
	uv --directory crates/kitest-py run --no-sync pytest

build:
	cargo build

# Regenerate crates/kitest-py/python/kitest/_kitest/__init__.pyi from the binding.
stubs:
	cargo run -q -p kitest-py --bin stub_gen

# Fail if the stubs on disk differ from what the binding generates, or the
# generator wrote a file git does not track. Staged stubs count as current.
stubs-check:
	$(MAKE) stubs
	@git diff --exit-code -- crates/kitest-py/python/kitest/_kitest/ \
		&& test -z "$$(git ls-files --others --exclude-standard -- crates/kitest-py/python/kitest/_kitest/)" \
		|| { echo "stubs are stale: run 'make stubs' and commit the result" >&2; exit 1; }
