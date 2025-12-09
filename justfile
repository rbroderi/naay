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

tag-release:
    $match = Select-String -Path pyproject.toml -Pattern '^version\s*=\s*"([^\"]+)"' | Select-Object -First 1;\
    if (-not $match) { throw "Could not find version in pyproject.toml" };\
    $version = $match.Matches[0].Groups[1].Value;\
    $tag = "v$version";\
    git rev-parse $tag 1>$null 2>$null;\
    if ($?) { throw "Tag $tag already exists" };\
    git tag -a $tag -m "Release $tag";\
    git push origin HEAD;\
    git push origin $tag

format:
    uv run ruff format .
    uv run ruff check . --fix
    uv run basedpyright --level error

check:
    uv run ruff check .
    uv run basedpyright --level error

lint:
    uv run ruff check . --fix

lint-unsafe:
    uv run ruff check . --fix --unsafe-fixes

metrics:
    uv run skylos . --quality
    uv run radon cc . -a -nb
    uv run radon mi . -nb

quality:
    uv run ruff format .
    uv run ruff check . --fix
    uv run basedpyright --level error
    uv run radon cc . -a -nb
    uv run skylos . --quality --danger

pytest:
    uv run pytest
