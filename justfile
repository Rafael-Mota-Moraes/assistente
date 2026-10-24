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
    just teste-dorme
    just teste-backup
    just teste-atualizar
    just teste-trabalhador
    just teste-journal
    just teste-limpar-disco
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
    shellcheck dev instalar.sh atualizar.sh bin/assistente bin/teste-backup.sh vm/provisionar.sh vm/configurar-agentes.sh vm/atualizar.sh vm/teste-atualizar.sh vm/fora-do-jail vm/trabalhador vm/teste-trabalhador.sh vm/teste-journal.sh vm/limpar-disco vm/teste-limpar-disco.sh notebook/instalar.sh notebook/dorme notebook/teste-dorme.sh

# Loop do modo dormir com agentes falsos, sem IA (F6.1)
teste-dorme:
    notebook/teste-dorme.sh

# Backup/restauração com incus falso, sem VM (F7.1)
teste-backup:
    bash bin/teste-backup.sh

# Trabalhador do quadro com opencode e fora-do-jail falsos (F11.6)
teste-trabalhador:
    bash vm/teste-trabalhador.sh

# Journal do hubd sem sudo: o grupo systemd-journal na conta (F14.2)
teste-journal:
    bash vm/teste-journal.sh

# Limpeza do disco da VM com df e docker falsos (F1.7)
teste-limpar-disco:
    bash vm/teste-limpar-disco.sh

# Atualizador da VM com repositório e serviço falsos, sem VM (F8.5)
teste-atualizar:
    bash vm/teste-atualizar.sh

# Roda o hubd com a config de exemplo em http://127.0.0.1:7878, sem túnel (fora da VM)
hubd *args:
    cargo run -p hubd -- --config config/assistente.exemplo.toml --listen 127.0.0.1:7878 --no-tunnel {{args}}

# Abre o app Tauri com recarga automática (janela via X11). Fala com o hubd de `just hubd`.
app-dev: deps
    ASSISTENTE_HUB_URL=http://127.0.0.1:7878 pnpm --dir app tauri dev

# Instaladores .deb e .AppImage do app em target/release/bundle/
build: deps
    pnpm --dir app tauri build

# Binário do hubd para a VM (target/release/hubd)
build-hubd:
    cargo build --release -p hubd

# Build da UI que o hubd serve em /ui/ (F8.3): app/dist com o commit do HEAD em commit.txt
build-ui: deps
    pnpm --dir app build
    git rev-parse --short HEAD > app/dist/commit.txt

# Crates do Cargo.lock publicados há pelo menos 14 dias (cadeia de suprimentos)
idade-crates:
    node devenv/check-crate-age.mjs
