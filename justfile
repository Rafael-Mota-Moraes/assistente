# Receitas do projeto. Rode sempre dentro do ambiente isolado: ./dev just <receita>
set shell := ["bash", "-euo", "pipefail", "-c"]

# Lista as receitas
default:
    @just --list

# Instala as dependências do frontend (rápido quando já estão instaladas)
deps:
    pnpm --dir app install --frozen-lockfile

# Todas as verificações: rode antes de dizer que algo está pronto
check: deps
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    pnpm --dir app typecheck
    pnpm --dir app lint
    pnpm --dir app test
    pnpm --dir app build
    just shellcheck
    just idade-crates

# Formata Rust e frontend
fmt: deps
    cargo fmt --all
    pnpm --dir app exec biome check --write .

# Testes (Rust + frontend)
test: deps
    cargo test --workspace
    pnpm --dir app test

# Lint dos scripts bash
shellcheck:
    shellcheck dev instalar.sh bin/assistente vm/provisionar.sh notebook/instalar.sh notebook/dorme

# Roda o hubd com a config de exemplo em http://127.0.0.1:7878 (fora da VM, para desenvolver)
hubd *args:
    cargo run -p hubd -- --config config/assistente.exemplo.toml {{args}}

# Abre o app Tauri com recarga automática (janela via X11). Fala com o hubd de `just hubd`.
app-dev: deps
    ASSISTENTE_HUB_URL=http://127.0.0.1:7878 pnpm --dir app tauri dev

# Instaladores .deb e .AppImage do app em target/release/bundle/
build: deps
    pnpm --dir app tauri build

# Binário do hubd para a VM (target/release/hubd)
build-hubd:
    cargo build --release -p hubd

# Crates do Cargo.lock publicados há pelo menos 14 dias (cadeia de suprimentos)
idade-crates:
    node devenv/check-crate-age.mjs
