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
    cp target/release/findex_cloud /usr/bin/findex_cloud && \
    cd static/ && npm install

####################################################################################################
## Final image
####################################################################################################
FROM ubuntu:20.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && \
    apt-get install --no-install-recommends -qq -y \
    libssl-dev && \
    rm -fr /var/lib/apt/lists/*


COPY --from=builder /usr/bin/findex_cloud* /usr/bin/

ENV DATABASE_URL=sqlite://database.sqlite

RUN touch database.sqlite

CMD ["findex_cloud"]
