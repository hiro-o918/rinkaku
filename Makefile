.PHONY: test lint format help

test: ## Run all tests
	cargo test --all-features

lint: ## Check formatting and run clippy with warnings as errors
	cargo fmt --all --check
	cargo clippy --all-targets --all-features -- -D warnings

format: ## Auto-format the codebase
	cargo fmt --all

help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-10s\033[0m %s\n", $$1, $$2}'
