# justfile for comfy-quant
# Run with `just <recipe-name>`

# Default recipe (show available commands)
default:
    @just --list

# Format + clippy
lint:
    cargo +nightly fmt --all
    cargo clippy --workspace --all-targets --all-features --tests --benches -- -D warnings

# Auto-fix lint issues
fix-lint:
    cargo clippy --fix --allow-dirty --workspace

# Check unused dependencies
check-deps:
    cargo +nightly udeps --workspace --all-features

# Run all tests
test:
    cargo nextest run --all-features --workspace --no-tests=pass --no-fail-fast

# Release new version (usage: just release v0.1.0)
release version:
    git tag {{version}}
    git commit -m "chore(release): prepare {{version}}"
    git tag -f {{version}}
    @echo "Run 'git push origin main --tags' to publish"
