use std::collections::HashSet;
use std::fs;

use heed::types::*;
use heed::EnvOpenOptions;

use cosmian_findex::{parameters::UID_LENGTH, EncryptedTable, Uid, UpsertData};

use crate::{
    core::{Index, IndexesDatabase, Table},
    errors::Error,
};

pub(crate) struct Database {
    env: heed::Env,
    db: heed::Database<ByteSlice, ByteSlice>,
}

impl Database {
    pub(crate) fn create() -> Self {
        let indexes_url = "data/indexes.lmdb";

        fs::create_dir_all(indexes_url).expect("Cannot create LMDB directory");

        let env = EnvOpenOptions::new()
            .map_size(4 * 1024 * 1024 * 1024)
            .open(indexes_url)
            .expect("Cannot open database");

        // we will open the default unamed database
        let db = env.create_database(None).expect("Cannot create database");

        Database { env, db }
    }
}

impl IndexesDatabase for Database {
    fn set_size(&self, index: &mut Index) -> Result<(), Error> {
        let txn = self.env.read_txn()?;

        index.size = Some(
            self.db
                .get(&txn, &size_key(index))?
                .map(|bytes| usize::from_be_bytes(bytes.try_into().unwrap()) as i64)
                .unwrap_or(0),
        );

        Ok(())
    }

    fn fetch(
        &self,
        index: &Index,
        table: Table,
        uids: HashSet<Uid<UID_LENGTH>>,
    ) -> Result<EncryptedTable<UID_LENGTH>, Error> {
        let mut uids_and_values = EncryptedTable::<UID_LENGTH>::with_capacity(uids.len());

        let txn = self.env.read_txn()?;
        for uid in uids {
            if let Some(value) = self.db.get(&txn, &key(index, table, &uid))? {
                uids_and_values.insert(uid, value.to_vec());
            }
        }

        Ok(uids_and_values)
    }

    fn upsert_entries(
        &self,
        index: &Index,
        data: UpsertData<UID_LENGTH>,
    ) -> Result<EncryptedTable<UID_LENGTH>, Error> {
        let mut rejected = EncryptedTable::<UID_LENGTH>::with_capacity(1);

        let mut txn = self.env.write_txn()?;
        for (uid, (old_value, new_value)) in data {
            let key = key(index, Table::Entries, &uid);

            let existing_value = self.db.get(&txn, &key)?;

            if existing_value == old_value.as_deref() {
                if existing_value.is_none() {
                    let size = self
                        .db
                        .get(&txn, &size_key(index))?
                        .map(|bytes| usize::from_be_bytes(bytes.try_into().unwrap()) as i64)
                        .unwrap_or(0);

                    self.db.put(
                        &mut txn,
                        &size_key(index),
                        &(size + new_value.len() as i64).to_be_bytes(),
                    )?;
                }

                self.db.put(&mut txn, &key, &new_value)?;
            } else {
                rejected.insert(uid.clone(), existing_value.unwrap().to_vec());
            }
        }
        txn.commit()?;

        Ok(rejected)
    }

    fn insert_chains(&self, index: &Index, data: EncryptedTable<UID_LENGTH>) -> Result<(), Error> {
        let mut txn = self.env.write_txn()?;
        let mut size = self
            .db
            .get(&txn, &size_key(index))?
            .map(|bytes| usize::from_be_bytes(bytes.try_into().unwrap()) as i64)
            .unwrap_or(0);
        for (uid, value) in data {
            size += value.len() as i64;
            self.db
                .put(&mut txn, &key(index, Table::Chains, &uid), &value)?;
        }

        self.db
            .put(&mut txn, &size_key(index), &size.to_be_bytes())?;
        txn.commit()?;

        Ok(())
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub(crate) enum Prefix {
    Entries,
    Chains,
    Size,
}

fn table_to_prefix(table: Table) -> Prefix {
    match table {
        Table::Entries => Prefix::Entries,
        Table::Chains => Prefix::Chains,
    }
}

fn key(index: &Index, table: Table, uid: &Uid<UID_LENGTH>) -> Vec<u8> {
    [
        &index.id.to_be_bytes(),
        &[table_to_prefix(table) as u8][..],
        uid.as_ref(),
    ]
    .concat()
}

fn size_key(index: &Index) -> Vec<u8> {
    [&index.id.to_be_bytes(), &[Prefix::Size as u8][..]].concat()
}
