.PHONY: build release test fmt lint clean install

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

fmt:
	cargo fmt

lint:
	cargo clippy

clean:
	cargo clean

install:
	./install.sh
