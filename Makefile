# Forge-Core Makefile
# Version management and release automation

SHELL := /bin/bash
.DEFAULT_GOAL := help

.PHONY: help bump bump-patch bump-minor bump-major version dry-run clean check test

help: ## Show this help message
	@echo "forge-core version management"
	@echo ""
	@echo "Usage: make <target>"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-15s %s\n", $$1, $$2}'

version: ## Show current version
	@grep -E '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/Current version: \1/'

bump: ## Bump patch version (0.8.2 -> 0.8.3)
	@./scripts/bump-version.sh patch

bump-patch: ## Bump patch version (alias for bump)
	@./scripts/bump-version.sh patch

bump-minor: ## Bump minor version (0.8.2 -> 0.9.0)
	@./scripts/bump-version.sh minor

bump-major: ## Bump major version (0.8.2 -> 1.0.0)
	@./scripts/bump-version.sh major

dry-run: ## Preview version bump without making changes
	@./scripts/bump-version.sh --dry-run patch

check: ## Run cargo check
	@cargo check

test: ## Run cargo test
	@cargo test

clean: ## Clean build artifacts and temp files
	@cargo clean
	@rm -rf /tmp/automagik-forge-bump-*
	@echo "Cleaned build artifacts and temp files"
