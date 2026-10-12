.PHONY: static check core-test ui-test build fmt-check lint
static:
	python3 scripts/static_check.py
core-test:
	cargo test -p cleaner-core
ui-test:
	cargo test -p cleaner-ui
check:
	cargo check -p cleaner-ui --target wasm32-unknown-unknown
build:
	python3 scripts/build.py
fmt-check:
	cargo +nightly fmt --all -- --check
lint:
	cargo clippy -p cleaner-core --all-targets -- -D warnings
