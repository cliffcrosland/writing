all:
	cargo build

check:
	cargo check

release:
	cargo build --release

start:
	cargo build
	./target/debug/backend

start_release:
	cargo build --release
	./target/release/backend

lint:
	cargo clippy

fmt:
	cargo fmt

clean:
	cargo clean

open_docs:
	cargo doc --open

test:
	# Using 8 test threads, one per test database.
	cargo test -- --test-threads=8

test_check:
	cargo check --tests

test_lint:
	cargo clippy --tests

test_print:
	# Using 8 test threads, one per test database.
	cargo test -- --test-threads=8 --nocapture

test_release:
	# Using 8 test threads, one per test database.
	cargo test --release -- --test-threads=8

test_release_print:
	# Using 8 test threads, one per test database.
	cargo test --release -- --test-threads=8 --nocapture
