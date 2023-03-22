## Builder
####################################################################################################
FROM rust:bullseye AS builder

WORKDIR /backend

COPY ./ .

RUN apt-get update && apt-get install -y curl && \
    apt-get install -y build-essential && \
    curl -fsSL https://deb.nodesource.com/setup_18.x | bash - && \
    apt-get install -y nodejs

RUN cargo install sqlx-cli && \
    sqlx database reset -y && \
    cargo build --release && \
    cd static/ && npm install && cd .. && \
    cp target/release/findex_cloud /usr/bin/findex_cloud

####################################################################################################
## Final image
####################################################################################################
FROM debian:bullseye

ENV DEBIAN_FRONTEND=noninteractive

WORKDIR /backend

RUN apt-get update && \
    apt-get install --no-install-recommends -qq -y \
    libssl-dev && \
    rm -fr /var/lib/apt/lists/*

RUN mkdir static

COPY --from=builder /usr/bin/findex_cloud* /usr/bin/

COPY --from=builder /backend/static/ /backend/static/

ENV DATABASE_URL=sqlite://database.sqlite

RUN touch database.sqlite

CMD ["findex_cloud"]