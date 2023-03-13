## Builder
####################################################################################################
FROM gitlab.cosmian.com:5000/core/ci-rust-20-04:20220929153432 AS builder

WORKDIR /backend

COPY ./ .

RUN sqlx database reset -y && \
    cargo build --release && \
    cp target/release/findex_cloud /usr/bin/findex_cloud

####################################################################################################
## Final image
####################################################################################################
FROM ubuntu:20.04

ENV DEBIAN_FRONTEND=noninteractive

COPY --from=builder /usr/bin/findex_cloud* /usr/bin/

ENV DATABASE_URL=sqlite://database.sqlite

CMD ["findex_cloud"]
