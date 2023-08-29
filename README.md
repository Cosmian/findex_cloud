# Findex Cloud

This is a server to store encrypted indexes. See [the Cosmian documentation](https://docs.cosmian.com/cloudproof_encryption/encrypted_search/).

## Setup

```bash
# Run it outside the folder since the SQLx CLI doesn’t build with the toolchain used by Findex Cloud
cargo install sqlx-cli 

# Inside the findex_cloud folder
sqlx database setup
cargo run
```

Visit [http://localhost:8080](http://localhost:8080)

You can run the server with other databases, eg: with DynamoDB:

```bash
AWS_ACCESS_KEY_ID= AWS_SECRET_ACCESS_KEY= AWS_REGION=eu-west-3 INDEXES_DATABASE_TYPE=dynamodb METADATA_DATABASE_TYPE=dynamodb cargo run --no-default-features --features dynamodb
```

## Definitions

Instance
: The server running the Findex Cloud binary

Index
: An index contains the association from keywords to locations. An **instance** can contain multiple indexes. For example, on a single **instance** you can have a "Dev" index and a "Prod" index, or you can store a "User" index and a "Companies" index. Each index have a different Findex key and label.

Metadata database
: The metadata database stores the list of **indexes** with their names, authentification keys. Two drivers exists for the metadata database: SQLite and DynamoDB.

Indexes database
: The indexes database stores the Findex entries and chains for all **indexes**. The Findex keys are prefixed with the **index** ID to be found.
