set shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    just --list

rust-build:
    Push-Location naay-py; uv run maturin develop; Pop-Location

rust-build-release:
    $env:RUSTFLAGS="-C target-cpu=native -C debug-assertions=no";\
    $env:CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS="false";\
    $env:CARGO_PROFILE_RELEASE_LTO="thin";\
    $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS="1";\
    Push-Location naay-py;\
    uv run maturin develop --release --strip;\
    Pop-Location

profile-flamegraph:
    Push-Location naay-py;\
    C:\\Users\\richa\\.cargo\\bin\\cargo.exe flamegraph --release;\
    Pop-Location
