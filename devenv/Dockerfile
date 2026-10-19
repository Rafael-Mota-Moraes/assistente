# Ambiente de desenvolvimento do projeto (adaptado do projeto game). Tudo que o hub e o app
# precisam para compilar e testar mora aqui dentro; nada é instalado no computador.
# Use pelo script ./dev.
FROM ubuntu:24.04

ARG USER_UID=1000
ARG USER_GID=1000
ARG RUST_VERSION=1.95.0
ARG NODE_VERSION=24.21.0
ARG PNPM_VERSION=11.28.3
ARG JUST_VERSION=1.58.0

ENV DEBIAN_FRONTEND=noninteractive

# Bibliotecas do Tauri (WebKitGTK), drivers Mesa, fontes, shellcheck e tmux (testes do hubd)
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl wget file git pkg-config xz-utils \
        libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
        libgl1-mesa-dri libegl-mesa0 fonts-dejavu-core fonts-liberation \
        shellcheck tmux bubblewrap \
    && rm -rf /var/lib/apt/lists/*

# ai-jail (mesma versão da VM): testes de isolamento por perfil (F3.2)
ARG AI_JAIL_VERSION=2.10.0
RUN cd /tmp \
    && curl -fsSL "https://github.com/akitaonrails/ai-jail/releases/download/v${AI_JAIL_VERSION}/ai-jail-linux-x86_64.tar.gz" \
        -o ai-jail.tar.gz \
    && echo "cd4b4b5dad69d6fb11a0cd9a17e6c2e1848cac4ba3d8da0a507a6eb535dc0714  ai-jail.tar.gz" | sha256sum -c - \
    && tar -xzf ai-jail.tar.gz \
    && install -m755 ai-jail /usr/local/bin/ai-jail \
    && rm -f ai-jail ai-jail.tar.gz

# Node.js (tarball oficial, checksum verificado) e pnpm
RUN cd /tmp \
    && curl -fsSLO "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
    && curl -fsSLO "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" \
    && grep " node-v${NODE_VERSION}-linux-x64.tar.xz\$" SHASUMS256.txt | sha256sum -c - \
    && tar -xJf "node-v${NODE_VERSION}-linux-x64.tar.xz" -C /usr/local --strip-components=1 \
    && rm -f node-v*.tar.xz SHASUMS256.txt \
    && npm install -g "pnpm@${PNPM_VERSION}"

# just (binário oficial)
RUN curl -fsSL "https://github.com/casey/just/releases/download/${JUST_VERSION}/just-${JUST_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
        | tar -xz -C /usr/local/bin just

# Usuário com o mesmo UID/GID do host: arquivos criados no projeto ficam com o dono certo
RUN userdel -r ubuntu 2>/dev/null || true \
    && groupadd -g "${USER_GID}" dev \
    && useradd -m -u "${USER_UID}" -g "${USER_GID}" -s /bin/bash dev \
    && mkdir -p /opt/rustup /opt/cargo/registry /opt/cargo/git \
    && chown -R dev:dev /opt/rustup /opt/cargo

USER dev
# APPIMAGE_EXTRACT_AND_RUN: linuxdeploy (AppImage) funciona sem FUSE dentro do container
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:/usr/local/bin:/usr/bin:/bin \
    APPIMAGE_EXTRACT_AND_RUN=1
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}" -c rustfmt,clippy \
    && mkdir -p /home/dev/.local/share/pnpm /home/dev/.cache /home/dev/.config

# Cache do pnpm no volume da home. Sem isto o pnpm cria .pnpm-store/ dentro do projeto
# (a pasta montada fica noutro sistema de arquivos).
ENV pnpm_config_store_dir=/home/dev/.local/share/pnpm/store

WORKDIR /home/dev
