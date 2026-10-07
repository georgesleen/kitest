NIX_FILES := $(shell find . -name '*.nix' -not -path './.git/*')

.PHONY: fmt fmt-check lint test pytest build stubs stubs-check docs docs-serve

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
	$(MAKE) pytest

# Build the extension with cargo and link it into the package source, where
# Python finds kitest._kitest beside kitest/__init__.py. No venv: pytest and
# every other Python dependency come from the nix dev shell.
TARGET_DIR := $(abspath $(or $(CARGO_TARGET_DIR),target))
pytest:
	cargo build -p kitest-py --lib --features pyo3/extension-module
	ln -sf $(TARGET_DIR)/debug/libkitest_py.so crates/kitest-py/python/kitest/_kitest.so
	cd crates/kitest-py && PYTHONPATH=python pytest

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

# Build the docs site into site/, with the Rust API under site/api/rust/.
# rustdoc gets its own target dir, so the site holds no stale crate docs.
docs:
	mkdocs build --strict
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --lib -p kitest --target-dir $(TARGET_DIR)/site
	rm -rf site/api/rust
	mkdir -p site/api
	cp -r $(TARGET_DIR)/site/doc site/api/rust

docs-serve:
	mkdocs serve
