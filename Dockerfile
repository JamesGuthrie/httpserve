FROM rust:1.52 as builder

RUN USER=root cargo new --bin httpserve
WORKDIR ./httpserve
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

COPY ./ ./

RUN rm ./target/release/deps/httpserve*
RUN cargo build --release

FROM ubuntu

RUN apt update && apt install -y libssl-dev

COPY --from=builder /httpserve/target/release/httpserve /

ENTRYPOINT ["/httpserve"]
