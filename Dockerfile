# Dockerfile for building ReviveL Companion (Linux)
FROM rust:1.80-slim-bookworm as builder

# Install dependencies for Tauri on Linux
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    wget \
    file \
    pkg-config \
    libglib2.0-dev \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    patchelf \
    nodejs \
    npm \
    && rm -rf /var/lib/apt/lists/*

# Install Node (if not sufficient)
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && \
    apt-get install -y nodejs

WORKDIR /app

# Copy manifests first for caching
COPY package*.json ./
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock ./src-tauri/
COPY src-tauri/tauri.conf.json ./src-tauri/

# Install node deps
RUN npm ci

# Copy source
COPY . .

# Build
RUN npm run tauri build

FROM scratch as export
COPY --from=builder /app/src-tauri/target/release/bundle/ /bundles/
