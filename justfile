# linelog

set shell := ["zsh", "-cu"]

# list recipes
default:
    @just --list

# install the git hooks (pre-commit and pre-push)
hooks:
    prek install

# every hook of both stages on every file: what CI runs
check:
    prek run --all-files
    prek run --all-files --stage pre-push

# rewrite files in place
fmt:
    cargo fmt --all

# fail on a formatting diff, without rewriting
fmt-check:
    cargo fmt --all -- --check

# clippy with warnings as errors, and the docs with broken links as errors
lint:
    cargo clippy --all-targets --all-features --locked -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked --quiet

# tests with line coverage, held to 100%
cover:
    cargo llvm-cov --locked --fail-under-lines 100

# what a log call costs, held to its ceiling
bench:
    cargo bench --locked --bench cost

# where a record's time goes, in zones of zonetrace, and what it allocates
profile *args:
    cargo run --release --locked --features zones --example profile -- {{args}}

# records from many threads and from tokio, through each sink
contend:
    cargo run --release --locked --example contend 2>/dev/null

# the oldest compiler the crate promises to build on (rust-version)
msrv:
    cargo +1.85.0 check --lib --locked
