#!/bin/bash
# bump-version.sh - Version bump automation for forge-core
#
# This script:
# 1. Bumps the workspace version in Cargo.toml
# 2. Creates a git commit and tag
# 3. Pushes to origin
# 4. Creates a PR in automagik-forge with updated dependency references
#
# Usage: ./scripts/bump-version.sh [--dry-run] [patch|minor|major]

set -euo pipefail

# Configuration
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="${REPO_ROOT}/Cargo.toml"
AUTOMAGIK_FORGE_REPO="git@github.com:automagik-dev/forge.git"
AUTOMAGIK_FORGE_DIR="/tmp/automagik-forge-bump-$$"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Flags
DRY_RUN=false
BUMP_TYPE="patch"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        patch|minor|major)
            BUMP_TYPE="$1"
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--dry-run] [patch|minor|major]"
            echo ""
            echo "Options:"
            echo "  --dry-run    Preview changes without making them"
            echo "  patch        Bump patch version (0.8.2 -> 0.8.3) [default]"
            echo "  minor        Bump minor version (0.8.2 -> 0.9.0)"
            echo "  major        Bump major version (0.8.2 -> 1.0.0)"
            exit 0
            ;;
        *)
            echo -e "${RED}Error: Unknown argument: $1${NC}"
            echo "Usage: $0 [--dry-run] [patch|minor|major]"
            exit 1
            ;;
    esac
done

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[OK]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Get current version from workspace Cargo.toml
get_current_version() {
    grep -E '^\s*version\s*=\s*"[0-9]+\.[0-9]+\.[0-9]+"' "$CARGO_TOML" | head -1 | sed 's/.*"\([0-9]*\.[0-9]*\.[0-9]*\)".*/\1/'
}

# Calculate new version
calculate_new_version() {
    local current="$1"
    local bump_type="$2"

    IFS='.' read -r major minor patch <<< "$current"

    case "$bump_type" in
        major)
            echo "$((major + 1)).0.0"
            ;;
        minor)
            echo "${major}.$((minor + 1)).0"
            ;;
        patch)
            echo "${major}.${minor}.$((patch + 1))"
            ;;
    esac
}

# Update workspace version in forge-core
update_workspace_version() {
    local new_version="$1"

    log_info "Updating workspace version to ${new_version}..."

    if [[ "$DRY_RUN" == "true" ]]; then
        log_warn "[DRY-RUN] Would update ${CARGO_TOML}"
        return
    fi

    # Update workspace version in Cargo.toml (under [workspace.package])
    sed -i.bak -E "s/^(version\s*=\s*\")[0-9]+\.[0-9]+\.[0-9]+(\")/\1${new_version}\2/" "$CARGO_TOML"
    rm -f "${CARGO_TOML}.bak"

    log_success "Updated workspace version"
}

# Create git tag
create_git_tag() {
    local version="$1"
    local tag="v${version}"

    log_info "Creating git commit and tag ${tag}..."

    if [[ "$DRY_RUN" == "true" ]]; then
        log_warn "[DRY-RUN] Would create commit and tag: ${tag}"
        return
    fi

    # Check if tag already exists
    if git tag -l | grep -q "^${tag}$"; then
        log_error "Tag ${tag} already exists!"
        exit 1
    fi

    git add -A
    git commit -m "chore: bump version to ${version}"
    git tag -a "${tag}" -m "Release ${tag}"

    log_success "Created commit and tag ${tag}"
}

# Push changes to origin
push_changes() {
    local tag="$1"

    log_info "Pushing changes and tag to origin..."

    if [[ "$DRY_RUN" == "true" ]]; then
        log_warn "[DRY-RUN] Would push commits and tag ${tag} to origin"
        return
    fi

    git push origin HEAD
    git push origin "${tag}"

    log_success "Pushed to origin"
}

# Clone/update automagik-forge and update dependencies
update_automagik_forge() {
    local new_tag="$1"

    log_info "Updating automagik-forge dependencies..."

    if [[ "$DRY_RUN" == "true" ]]; then
        log_warn "[DRY-RUN] Would clone automagik-forge and update forge-core dependencies to ${new_tag}"
        log_warn "[DRY-RUN] Files that would be updated:"
        log_warn "[DRY-RUN]   - forge-app/Cargo.toml (7 dependencies)"
        log_warn "[DRY-RUN]   - forge-extensions/config/Cargo.toml (1 dependency)"
        log_warn "[DRY-RUN] Would create PR: 'chore: bump forge-core to ${new_tag}'"
        return
    fi

    # Clean up any existing temp directory
    rm -rf "$AUTOMAGIK_FORGE_DIR"

    # Clone fresh copy
    log_info "Cloning automagik-forge..."
    git clone --depth 1 -b dev "$AUTOMAGIK_FORGE_REPO" "$AUTOMAGIK_FORGE_DIR"

    cd "$AUTOMAGIK_FORGE_DIR"

    # Create branch for PR
    local branch_name="chore/bump-forge-core-${new_tag}"
    git checkout -b "$branch_name"

    # Update all forge-core dependency references
    local files_to_update=(
        "forge-app/Cargo.toml"
        "forge-extensions/config/Cargo.toml"
    )

    for file in "${files_to_update[@]}"; do
        if [[ -f "$file" ]]; then
            log_info "Updating ${file}..."
            # Replace tag = "vX.Y.Z" with new tag for forge-core.git references
            sed -i.bak -E "s|(forge-core\.git\", tag = \")v[0-9]+\.[0-9]+\.[0-9]+(\")|\1${new_tag}\2|g" "$file"
            rm -f "${file}.bak"
        fi
    done

    # Run cargo update to refresh Cargo.lock
    log_info "Running cargo update..."
    cargo update 2>/dev/null || log_warn "cargo update had warnings (non-fatal)"

    # Commit changes
    git add -A
    git commit -m "chore: bump forge-core to ${new_tag}"

    # Push branch
    log_info "Pushing branch ${branch_name}..."
    git push -u origin "$branch_name"

    # Create PR using gh CLI
    log_info "Creating pull request..."
    local pr_url
    pr_url=$(gh pr create \
        --title "chore: bump forge-core to ${new_tag}" \
        --body "## Summary
- Bumps forge-core dependency to ${new_tag}

## Changes
- Updated \`forge-app/Cargo.toml\` (7 dependencies)
- Updated \`forge-extensions/config/Cargo.toml\` (1 dependency)
- Refreshed \`Cargo.lock\`

## Testing
- [ ] Verify compilation: \`cargo build\`
- [ ] Run tests: \`cargo test\`

---
*Auto-generated by forge-core bump script*" \
        --base dev \
        --head "$branch_name" 2>&1) || true

    if [[ -n "$pr_url" ]]; then
        log_success "Pull request created: ${pr_url}"
    else
        log_warn "PR creation may have failed - check GitHub manually"
    fi

    # Return to original directory
    cd "$REPO_ROOT"

    # Cleanup
    log_info "Cleaning up temporary directory..."
    rm -rf "$AUTOMAGIK_FORGE_DIR"
}

# Verify prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."

    # Check we're in the right directory
    if [[ ! -f "$CARGO_TOML" ]]; then
        log_error "Cargo.toml not found. Are you in the forge-core directory?"
        exit 1
    fi

    # Check git is available
    if ! command -v git &> /dev/null; then
        log_error "git is not installed"
        exit 1
    fi

    # Check git is clean (for non-dry-run)
    if [[ "$DRY_RUN" == "false" ]]; then
        if ! git diff --quiet HEAD 2>/dev/null; then
            log_error "Working directory has uncommitted changes. Please commit or stash first."
            exit 1
        fi
    fi

    # Check gh CLI is available and authenticated
    if ! command -v gh &> /dev/null; then
        log_error "GitHub CLI (gh) is not installed. Install with: brew install gh"
        exit 1
    fi

    if ! gh auth status &> /dev/null; then
        log_error "GitHub CLI is not authenticated. Run: gh auth login"
        exit 1
    fi

    # Check we're on the right branch (dev or main)
    local current_branch
    current_branch=$(git branch --show-current)
    if [[ "$current_branch" != "dev" && "$current_branch" != "main" ]]; then
        log_warn "Currently on branch '${current_branch}'. Typically bumps are done on 'dev' or 'main'."
        if [[ "$DRY_RUN" == "false" ]]; then
            read -p "Continue anyway? (y/N): " confirm
            if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
                log_info "Aborted."
                exit 0
            fi
        fi
    fi

    log_success "Prerequisites check passed"
}

# Main execution
main() {
    cd "$REPO_ROOT"

    echo ""
    echo -e "${BLUE}=== Forge-Core Version Bump ===${NC}"
    echo ""
    log_info "Bump type: ${BUMP_TYPE}"
    [[ "$DRY_RUN" == "true" ]] && log_warn "Running in DRY-RUN mode (no changes will be made)"
    echo ""

    check_prerequisites

    local current_version
    current_version=$(get_current_version)
    local new_version
    new_version=$(calculate_new_version "$current_version" "$BUMP_TYPE")
    local new_tag="v${new_version}"

    echo ""
    log_info "Current version: ${current_version}"
    log_info "New version: ${new_version}"
    log_info "New tag: ${new_tag}"
    echo ""

    if [[ "$DRY_RUN" == "false" ]]; then
        read -p "Proceed with version bump? (y/N): " confirm
        if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
            log_info "Aborted."
            exit 0
        fi
        echo ""
    fi

    # Step 1: Update workspace version
    update_workspace_version "$new_version"

    # Step 2: Create git tag and commit
    create_git_tag "$new_version"

    # Step 3: Push to origin
    push_changes "$new_tag"

    # Step 4: Update automagik-forge and create PR
    update_automagik_forge "$new_tag"

    echo ""
    log_success "=== Version bump complete! ==="
    log_info "forge-core is now at version ${new_version} (tag: ${new_tag})"
    log_info "automagik-forge PR has been created for review"
    echo ""
}

main "$@"
