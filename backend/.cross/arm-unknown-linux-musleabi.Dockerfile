FROM ghcr.io/cross-rs/arm-unknown-linux-musleabi:latest

# Install needed libs for yeslogic-fontconfig-sys
RUN apk update && apk add \
    fontconfig-dev \
    freetype-dev \
    expat-dev \
    libxml2-dev

# Ensure pkg-config can see them
ENV PKG_CONFIG_PATH="/usr/lib/pkgconfig:/usr/local/lib/pkgconfig"

