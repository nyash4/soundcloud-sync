# syntax=docker/dockerfile:1

FROM node:24-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-alpine AS server
RUN apk add --no-cache build-base cmake perl
WORKDIR /server
COPY server/ ./
RUN cargo build --release --locked

FROM alpine:3
RUN adduser -D -H app && mkdir -p /data && chown app /data
COPY --from=server /server/target/release/scsync-server /usr/local/bin/scsync-server
COPY --from=web /web/dist /app/web
ENV HOST=0.0.0.0 \
    PORT=8080 \
    DB_PATH=/data/scsync.db \
    STATIC_DIR=/app/web \
    TRUST_PROXY=true
USER app
VOLUME /data
EXPOSE 8080
CMD ["scsync-server"]
