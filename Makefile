.PHONY: build release test fmt lint clean install hooks

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

hooks:
	ln -sf "$(PWD)/hooks/pre-push" .git/hooks/pre-push

install:
	./install.sh
