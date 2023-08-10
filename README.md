# Findex Cloud

This is a server to store encrypted indexes. See [the Cosmian documentation](https://docs.cosmian.com/cloudproof_encryption/encrypted_search/).

## Setup

```bash
sqlx database setup
cargo run
```

Visit [http://localhost:8080](http://localhost:8080)

You run the server with other databases, eg: with DynamoDB:

```bash
AWS_ACCESS_KEY_ID= AWS_SECRET_ACCESS_KEY= AWS_REGION=eu-west-3 INDEXES_DATABASE_TYPE=dynamodb METADATA_DATABASE_TYPE=dynamodb cargo run --no-default-features --features dynamodb
```
