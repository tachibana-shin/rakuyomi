ARG CROSS_BASE_IMAGE=ghcr.io/cross-rs/arm-unknown-linux-musleabihf:0.2.5
FROM $CROSS_BASE_IMAGE

# rquickjs-sys ships no pre-generated bindings for this target, so it runs
# bindgen at build time. The image has libclang but no clang driver, and the
# target's musl headers live in the cross sysroot, so install the clang
# driver and point bindgen at the sysroot headers. Using the bindgen-only
# environment variable keeps the host-side build scripts (which also run in
# this container) on the glibc headers.
RUN apt-get update && apt-get install -y --no-install-recommends clang

ENV BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/local/arm-unknown-linux-musleabihf/include"
