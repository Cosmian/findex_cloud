## Builder
####################################################################################################
FROM rust:bullseye AS builder

WORKDIR /backend

COPY ./ .

RUN apt-get update && apt-get install -y curl build-essential clang && \
    curl -fsSL https://deb.nodesource.com/setup_18.x | bash - && \
    apt-get install -y nodejs

RUN touch data/database.sqlite

RUN cargo install sqlx-cli && \
    cp .env.example .env && \
    sqlx database reset -y && \
    cargo build --release --features lmmd,dynamodb && \
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
    libssl-dev ca-certificates && \
    rm -fr /var/lib/apt/lists/*

RUN mkdir static

COPY --from=builder /usr/bin/findex_cloud* /usr/bin/

COPY --from=builder /backend/static/ /backend/static/

ENV DATABASE_URL=sqlite://data/database.sqlite

RUN mkdir data
RUN touch data/database.sqlite

EXPOSE 8080
CMD ["findex_cloud"]
