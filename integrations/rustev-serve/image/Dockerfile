# The rustev-serve image (spec 042 3.16). Build from the repository root:
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     -f integrations/rustev-serve/image/Dockerfile .
# Building, reproducibility, SBOM, signature, provenance, scanning and
# publication are spec 031's; this file defines the image only.
# Both bases are pinned by digest: the builder is the toolchain image spec
# 031's artifacts.sh builds the Linux CLI with, and the runtime is the base of
# spec 010's rustev-serve binding record (cc, because the binary links glibc).

FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS build
WORKDIR /src
COPY . .
RUN cargo build -p rustev-serve --bin rustev-serve --locked --release \
 && install -D -m 0555 target/release/rustev-serve /out/rustev-serve

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1
COPY --from=build --chown=0:0 /out/rustev-serve /usr/local/bin/rustev-serve
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/rustev-serve", "--config", "/etc/rustev/serve.json"]
