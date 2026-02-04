default:
    @just --list

build:
    cargo build --release

debug:
    cargo build

install: build
    cp target/release/tgrs /opt/homebrew/bin/tgrs

clean:
    cargo clean

check:
    cargo check

test:
    cargo test

fmt:
    cargo fmt

clippy:
    cargo clippy -- -W clippy::all
