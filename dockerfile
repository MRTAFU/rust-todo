# Docker Official の Rust イメージを使います。
FROM rust:1.68.1 AS builder

ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

# /todo でビルドを行うことにします。
WORKDIR /todo

# Cargo.toml のみを先にイメージに追加する。
COPY Cargo.toml Cargo.toml

# ビルドするために何もしないソースコードを入れておく。
RUN mkdir src
RUN echo "fn main(){}" > src/main.rs

# 上記で作成したコードと依存クレートをビルドする。
RUN cargo build --release

# アプリケーションのコードをイメージにコピーします。
COPY ./src ./src
COPY ./templates ./templates

# 先程ビルドした生成物のうち、アプリケーションのもののみを削除する
RUN rm -f target/release/deps/todo*

# 改めてアプリケーションをビルドします
RUN cargo build --release

# 新しくリリース用のイメージを用意します。
FROM debian:10.4

# buiilder イメージからtodo飲みをコピーして /usr/local/bin に配置します。
COPY --from=builder /todo/target/release/todo /usr/local/bin

# コンテナ起動時に Web アプリを実行します
CMD ["todo"]
