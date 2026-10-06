# Image du CLI `xc` : un binaire unique, sans Node ni Chromium.
#
#   docker build -t sira .
#   docker run --rm -v "$PWD:/collection" sira run /collection --env prod --reporter-junit /collection/junit.xml
#
# Le dossier monté doit être inscriptible pour que les rapports (--reporter-*) y soient écrits.

FROM rust:1.94-bookworm AS build
WORKDIR /src
# Le workspace compte l'application Tauri : son manifeste doit être présent pour que cargo le charge, mais elle n'est
# pas compilée (seul xc-cli l'est).
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY crates crates
COPY app/src-tauri app/src-tauri
RUN cargo build --release --locked -p xc-cli

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/xc /usr/local/bin/xc
WORKDIR /collection
ENTRYPOINT ["/usr/local/bin/xc"]
CMD ["--help"]
