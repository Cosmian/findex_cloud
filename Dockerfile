## Builder
####################################################################################################
FROM gitlab.cosmian.com:5000/core/ci-rust-20-04:20220929153432 AS builder

WORKDIR /backend

COPY ./ .

RUN apt-get update && apt-get install -y curl && \
    apt-get install -y build-essential && \
    curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash - && \
    sudo apt-get install -y nodejs

RUN sqlx database reset -y && \
    cargo build --release && \
    cd static/ && npm install && cd .. && \
    cp target/release/findex_cloud /usr/bin/findex_cloud

####################################################################################################
## Final image
####################################################################################################
FROM ubuntu:20.04

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
