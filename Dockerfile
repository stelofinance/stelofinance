FROM nixos/nix:latest AS builder

WORKDIR /build
COPY . ./

# Build project using nix flake for deps and task to build
ENV NIX_BUILD_SHELL=1
RUN nix develop \
  --extra-experimental-features "nix-command flakes" \
  --command task build

FROM scratch

WORKDIR /

COPY --from=builder /build/bin/app ./app
COPY --from=builder /build/web/static ./web/static
COPY --from=builder /build/web/templates ./web/templates

COPY --from=builder /build/bin/migrate ./migrate

CMD ["/app"]
