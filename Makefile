.PHONY: help install up test test-rust test-web test-smoke build build-rust build-wasm build-web fmt clippy check

# Default target
all: build

help:
	@echo "anygamepad commands:"
	@echo "  make install    - Install project dependencies (npm packages in apps/web)"
	@echo "  make up         - Start local development server for the web app (Vite)"
	@echo "  make test       - Run all test suites (Rust tests, clippy, fmt, and TypeScript checks)"
	@echo "  make test-smoke - Run end-to-end smoke tests in browser (Playwright)"
	@echo "  make build      - Build everything (Rust crates, WASM bindings, and web production bundle)"
	@echo "  make fmt        - Check/format Rust codebase"
	@echo "  make clippy     - Lint Rust codebase with clippy"

## Install dependencies
install:
	npm --prefix apps/web install

## Run local development server
up:
	npm --prefix apps/web run dev

## Testing
test: test-rust clippy fmt test-web

test-rust:
	cargo test --workspace

test-web:
	npm --prefix apps/web run check

test-smoke:
	npm --prefix apps/web run test:smoke

fmt:
	cargo fmt -- --check

clippy:
	cargo clippy --workspace -- -D warnings

check: test

## Building
build: build-rust build-wasm build-web

build-rust:
	cargo build --workspace

build-wasm:
	npm --prefix apps/web run wasm

build-web:
	npm --prefix apps/web run build
	npm --prefix apps/web run size
