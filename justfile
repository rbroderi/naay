set shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    just --list

rust-build:
    Push-Location naay-py; uv run maturin develop; Pop-Location

rust-build-release:
    $env:RUSTFLAGS="-C target-cpu=native";\
    Push-Location naay-py;\
    uv run maturin develop --release;\
    Pop-Location

profile-flamegraph:
    Push-Location naay-py;\
    C:\\Users\\richa\\.cargo\\bin\\cargo.exe flamegraph --release;\
    Pop-Location
