# The greedy baseline as a leaderboard entry: built from this repository, as an
# outside entry is built from its own. The image runs the coordinator, which
# speaks arcadia-protocol on standard input and output.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked --bin baseline-greedy

FROM debian:bookworm-slim
COPY --from=build /src/target/release/baseline-greedy /usr/local/bin/coordinator
USER 65534:65534
ENTRYPOINT ["/usr/local/bin/coordinator"]
