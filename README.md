# Findex Cloud

This is a server to store encrypted indexes. See [the Cosmian documentation](https://docs.cosmian.com/cloudproof_encryption/encrypted_search/).

## Definitions

Instance
: The server running the Findex Cloud binary

Index
: An index contains the association from keywords to locations. An **instance** can contain multiple indexes. For example, on a single **instance** you can have a "Dev" index and a "Prod" index, or you can store a "User" index and a "Companies" index. Each index have a different Findex key and label.

Metadata database
: The metadata database stores the list of **indexes** with their names, authentification keys. Two implementations exists for the metadata database: [SQLite](https://www.sqlite.org/index.html) and [DynamoDB](https://aws.amazon.com/fr/dynamodb/).

Indexes database
: The indexes database stores the Findex entries and chains for all **indexes**. The Findex keys are prefixed with the **index** ID to be found. Three implementations exists for the indexes database: [DynamoDB](https://aws.amazon.com/fr/dynamodb/), [RocksDB](https://rocksdb.org/) and [LMMD](https://en.wikipedia.org/wiki/Lightning_Memory-Mapped_Database).

## Implementations

### SQLite (metadata)

See the [./src/sqlite.rs](./src/sqlite.rs) file.

### DynamoDB (metadata and indexes)

See comment inside ̏the [./src/dynamodb.rs](./src/dynamodb.rs) file.

### RocksDB (indexes)

See the [./src/rocksdb.rs](./src/rocksdb.rs) file.

### LMMD (indexes)

See the [./src/heed.rs](./src/heed.rs) file. `heed` is the name of the Rust implementation of LMMD.

## Setup

```bash
# Run it outside the folder since the SQLx CLI doesn’t build with the toolchain used by Findex Cloud
cargo install sqlx-cli 

# Inside the findex_cloud folder
sqlx database setup
cargo run
```

Visit [http://localhost:8080](http://localhost:8080)

## Configuration

You can run Findex Cloud with different implementations. Rust features gate the different implementation (to allow building a "minimal" Findex Cloud with just the used implementation) but the binary inside the Docker is built with all the features for all the implementations. The default enabled features are SQLite for metadata and RocksDB for indexes (if you don’t want to use these you can disable default features with `--no-default-features`).

After building Findex Cloud with the correct wanted features, you can choose the implementation for the metadata database and the indexes database at runtime with environment variables:
- METADATA_DATABASE_TYPE
- INDEXES_DATABASE_TYPE

Some implementations require additional config values in environment databases. For exemple, to run with DynamoDB:

```bash
AWS_ACCESS_KEY_ID=xxx AWS_SECRET_ACCESS_KEY=xxx AWS_REGION=eu-west-3 INDEXES_DATABASE_TYPE=dynamodb METADATA_DATABASE_TYPE=dynamodb cargo run --no-default-features --features dynamodb
```

## `log_requests` feature

This feature is only useful in development mode. It allows to log all requests done to Findex Cloud and store the requested values and the responses. We use these dump to attack the architecture and try to find the requested keywords as an insider. These informations don’t leak the requested keywords nor the stored indexes.