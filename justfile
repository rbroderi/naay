set shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    just --list

rust-build:
    Push-Location naay-py; uv run maturin develop; Pop-Location
