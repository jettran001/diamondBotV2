# Dockerfile
FROM rust:1.70 as builder

WORKDIR /app
COPY . .

# Thêm các dependency cụ thể mà project cần
RUN apt-get update && apt-get install -y \
    libssl-dev \
    pkg-config

# Cài đặt đúng toolchain và dependencies
RUN rustup default stable && \
    rustup target add wasm32-unknown-unknown && \
    cargo install wasm-pack

# Tối ưu build
RUN cargo build --release

# Image giai đoạn runtime nhẹ hơn
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binaries và các tài nguyên cần thiết
COPY --from=builder /app/target/release/snipebot_rust /app/snipebot
COPY --from=builder /app/wasm/target/wasm32-unknown-unknown/release/*.wasm /app/wasm/
COPY --from=builder /app/abi /app/abi
COPY .env.example /app/.env

# Tạo thư mục logs và data
RUN mkdir -p /app/logs /app/data

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1

# Run as non-root user
RUN useradd -m snipebot
USER snipebot

# Expose ports
EXPOSE 8080

CMD ["/app/snipebot"]
