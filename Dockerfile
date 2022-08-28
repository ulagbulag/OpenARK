# Configure environment variables
ARG \
    ALPINE_VERSION="3.16" \
    PACKAGE="netai-cloud"

# Be ready for serving
FROM docker.io/alpine:${ALPINE_VERSION} as server

# Metadata Configuration
LABEL \
    ALPINE_VERSION=${ALPINE_VERSION} \
    AUTHOR="kerryeon" \
    LICENSE="GPL v3.0 with a classpath linking exception" \
    PACKAGE=${PACKAGE}

# Server Configuration
EXPOSE 80/tcp
WORKDIR /usr/local/bin
CMD [ "/bin/sh" ]

# Install dependencies
RUN apk add --no-cache \
    libgcc

# Be ready for building
FROM docker.io/rust:1-alpine${ALPINE_VERSION} as builder

# Builder environment variables
ENV \
    RUSTFLAGS="-C target-feature=-crt-static"

# Install dependencies
RUN apk add --no-cache \
    musl-dev \
    openssl-dev

# Load source files
ADD . /src
WORKDIR /src

# Build it!
RUN mkdir /out \
    && cargo build --all --workspace --release \
    && find ./target/release/ -maxdepth 1 -type f -perm +a=x -print0 | xargs -0 -I {} mv {} /out \
    && mv ./LICENSE /LICENSE \
    && rm -rf /src

# Copy executable files
FROM server
COPY --from=builder /out/* /usr/local/bin/
COPY --from=builder /LICENSE /usr/share/licenses/${PACKAGE}/LICENSE