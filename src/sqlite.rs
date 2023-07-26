use async_trait::async_trait;
use sqlx::{migrate::MigrateDatabase, sqlite::SqlitePoolOptions, Sqlite, SqlitePool};

use crate::{
    core::{Index, MetadataDatabase, NewIndex},
    errors::Error,
};

pub(crate) struct Database(SqlitePool);

impl Database {
    pub(crate) async fn create() -> Self {
        let db_url = "sqlite://data/database.sqlite";

        if !Sqlite::database_exists(db_url)
            .await
            .unwrap_or_else(|e| panic!("Cannot check database existance at {db_url} ({e})"))
        {
            Sqlite::create_database(db_url)
                .await
                .unwrap_or_else(|e| panic!("Cannot create database {db_url} ({e})"));
        }

        let pool = SqlitePoolOptions::new()
            .connect(db_url)
            .await
            .unwrap_or_else(|e| panic!("Cannot connect to database at {db_url} ({e})"));

        sqlx::migrate!()
            .run(&pool)
            .await
            .unwrap_or_else(|e| panic!("Cannot run migration on database at {db_url} ({e})"));

        Database(pool)
    }
}

#[async_trait]
impl MetadataDatabase for Database {
    async fn get_indexes(&self) -> Result<Vec<Index>, Error> {
        let mut db = self.0.acquire().await?;

        Ok(sqlx::query_as!(
            Index,
            r#"
            SELECT
                *,
                null as "size: _"
            FROM indexes
            ORDER BY created_at DESC"#,
        )
        .fetch_all(&mut db)
        .await?)
    }

    async fn get_index(&self, id: &str) -> Result<Option<Index>, Error> {
        let mut db = self.0.acquire().await?;

        let index = sqlx::query_as!(
            Index,
            r#"
                SELECT
                    *,
                    null as "size: _"
                FROM indexes
                WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&mut db)
        .await?;

        Ok(index)
    }

    async fn delete_index(&self, id: &str) -> Result<(), Error> {
        let mut db = self.0.acquire().await?;

        sqlx::query_as!(Index, r#"DELETE FROM indexes WHERE id = $1"#, id,)
            .execute(&mut db)
            .await?;

        Ok(())
    }

    async fn create_index(&self, new_index: NewIndex) -> Result<Index, Error> {
        let mut db = self.0.acquire().await?;

        let Id { id } = sqlx::query_as!(
            Id,
            r#"INSERT INTO indexes (
                id,
    
                name,
    
                fetch_entries_key,
                fetch_chains_key,
                upsert_entries_key,
                insert_chains_key
            ) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id"#,
            new_index.id,
            new_index.name,
            new_index.fetch_entries_key,
            new_index.fetch_chains_key,
            new_index.upsert_entries_key,
            new_index.insert_chains_key,
        )
        .fetch_one(&mut db)
        .await?;

        Ok(sqlx::query_as!(
            Index,
            r#"SELECT *, null as "size: _" FROM indexes WHERE id = $1"#,
            id
        )
        .fetch_one(&mut db)
        .await?)
    }
}

struct Id {
    // The column is mark as `NOT NULL` but SQLx seems to not understand it.
    id: Option<String>,
}
