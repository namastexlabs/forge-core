#!/bin/bash
# link-dev-branch.sh - Link automagik-forge to current forge-core development branch
#
# This script allows you to test forge-core changes in automagik-forge without
# needing to merge/tag/release. It updates the git rev references in automagik-forge
# to point to the current HEAD of forge-core.
#
# Usage: ./scripts/link-dev-branch.sh [path-to-automagik-forge]

set -euo pipefail

# Configuration
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AUTOMAGIK_FORGE_DIR="${1:-../automagik-forge}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[OK]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Get current commit hash from forge-core
cd "$REPO_ROOT"
CURRENT_COMMIT=$(git rev-parse HEAD)
CURRENT_BRANCH=$(git branch --show-current)

log_info "forge-core state:"
echo "  Branch: $CURRENT_BRANCH"
echo "  Commit: $CURRENT_COMMIT"
echo ""

# Validate automagik-forge directory
if [[ ! -d "$AUTOMAGIK_FORGE_DIR" ]]; then
    log_error "automagik-forge not found at: $AUTOMAGIK_FORGE_DIR"
    echo "Usage: $0 [path-to-automagik-forge]"
    exit 1
fi

cd "$AUTOMAGIK_FORGE_DIR"
log_info "Found automagik-forge at: $(pwd)"

# Check for uncommitted changes in automagik-forge
if ! git diff --quiet || ! git diff --cached --quiet; then
    log_warn "automagik-forge has uncommitted changes"
    read -p "Continue anyway? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        log_error "Aborted by user"
        exit 1
    fi
fi

# Find and update all Cargo.toml files that reference forge-core
log_info "Updating forge-core git rev references..."

# Files that need updating
FILES=(
    "forge-app/Cargo.toml"
    "forge-extensions/config/Cargo.toml"
)

UPDATED_COUNT=0
for file in "${FILES[@]}"; do
    if [[ -f "$file" ]]; then
        OLD_REV=$(grep -oP 'forge-core\.git.*?rev = "\K[a-f0-9]+' "$file" | head -1 || echo "none")

        if [[ "$OLD_REV" == "none" ]]; then
            log_warn "No forge-core reference found in $file"
            continue
        fi

        log_info "Updating $file..."
        echo "  Old rev: $OLD_REV"
        echo "  New rev: $CURRENT_COMMIT"

        # Update the git rev in the file
        sed -i "s|forge-core\.git\", rev = \"[a-f0-9]*\"|forge-core.git\", rev = \"$CURRENT_COMMIT\"|g" "$file"

        ((UPDATED_COUNT++))
    else
        log_warn "File not found: $file"
    fi
done

if [[ $UPDATED_COUNT -eq 0 ]]; then
    log_error "No files were updated"
    exit 1
fi

log_success "Updated $UPDATED_COUNT file(s)"
echo ""

# Update Cargo.lock
log_info "Updating Cargo.lock..."
cargo update 2>&1 | grep -E "(Updating|Adding|Removing)" || true
log_success "Cargo.lock updated"
echo ""

# Show changes
log_info "Changes made:"
git diff forge-app/Cargo.toml forge-extensions/config/Cargo.toml | head -50

echo ""
log_success "automagik-forge is now linked to forge-core @ $CURRENT_COMMIT"
echo ""
log_info "Next steps:"
echo "  1. cd $AUTOMAGIK_FORGE_DIR"
echo "  2. cargo build  # Test the build"
echo "  3. npm run dev  # Test in development"
echo ""
log_warn "Remember: These changes are for local testing only!"
log_warn "Do NOT commit these changes - they reference an unreleased forge-core commit"
