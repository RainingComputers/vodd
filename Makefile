CARGO ?= cargo
NPM ?= npm
NPX ?= npx

NODE_STAMP := node_modules/.install-stamp
BROWSER_STAMP := node_modules/.browser-stamp
SUBMODULE := tests/vendor/OpenCL-CTS/test_conformance

.DEFAULT_GOAL := help
.PHONY: help fmt lint test test-rust test-browser submodules clean

help:
	@echo "fmt           format Rust with cargo fmt and templates with prettier"
	@echo "lint          check formatting, run clippy, check template formatting"
	@echo "test          run the Rust suites and the browser suite"
	@echo "test-rust     run the Rust suites only"
	@echo "test-browser  run the Playwright suite only"
	@echo "submodules    fetch the vendored Khronos repositories"
	@echo "clean         remove build output and installed node packages"

fmt: $(NODE_STAMP)
	$(CARGO) fmt
	$(NPM) run format

lint: $(NODE_STAMP)
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(NPM) run format:check

test: test-rust test-browser

test-rust: $(SUBMODULE)
	$(CARGO) test

test-browser: $(BROWSER_STAMP)
	$(NPM) run test:browser

submodules: $(SUBMODULE)

$(SUBMODULE):
	git submodule update --init --recursive

$(NODE_STAMP): package.json package-lock.json
	$(NPM) ci
	@touch $@

$(BROWSER_STAMP): $(NODE_STAMP)
	$(NPX) playwright install chromium
	@touch $@

clean:
	$(CARGO) clean
	rm -rf node_modules
