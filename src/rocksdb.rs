use std::{collections::HashSet, iter::zip};

use cosmian_findex::{parameters::UID_LENGTH, EncryptedTable, Uid, UpsertData};
use rocksdb::{MergeOperands, Options, TransactionDB, TransactionDBOptions};

use crate::{
    core::{Index, IndexesDatabase, Table},
    errors::Error,
};

pub(crate) struct Database(TransactionDB);

impl Database {
    pub(crate) fn create() -> Self {
        let indexes_url = "data/indexes_rocksdb";

        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_merge_operator_associative("add", merge_add);
        opts.set_max_open_files(10);
        let mut txn_db_opts = TransactionDBOptions::default();
        txn_db_opts.set_txn_lock_timeout(10);

        let transaction_db: TransactionDB =
            TransactionDB::open(&opts, &txn_db_opts, indexes_url).unwrap();

        Database(transaction_db)
    }
}

impl IndexesDatabase for Database {
    fn set_size(&self, index: &mut Index) -> Result<(), Error> {
        index.size = Some(
            self.0
                .get(size_key(index))?
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

        let values = self
            .0
            .multi_get(uids.iter().map(|uid| key(index, table, uid)));

        for (uid, value) in zip(uids.into_iter(), values.into_iter()) {
            let value = value.unwrap();
            if let Some(value) = value {
                uids_and_values.insert(uid, value);
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

        for (uid, (old_value, new_value)) in data {
            let key = key(index, Table::Entries, &uid);

            let transaction = self.0.transaction();

            let existing_value = match transaction.get_for_update(&key, true) {
                Ok(existing_value) => existing_value,
                Err(err) if err.as_ref() == "Operation timed out: Timeout waiting to lock key" => {
                    transaction.rollback()?;

                    let mut retry = 3;
                    let value = loop {
                        if let Some(value) = self.0.get(&key)? {
                            break value;
                        }

                        retry -= 1;
                        if retry <= 0 {
                            return Err(Error::Rocksdb(err));
                        }
                    };

                    rejected.insert(uid.clone(), value);
                    continue;
                }
                err => err?,
            };

            if existing_value == old_value {
                if existing_value.is_none() {
                    transaction.merge(size_key(index), new_value.len().to_be_bytes())?;
                }

                transaction.put(&key, new_value)?;
                transaction.commit()?;
            } else {
                transaction.rollback()?;
                rejected.insert(uid.clone(), existing_value.unwrap());
            }
        }

        Ok(rejected)
    }

    fn insert_chains(&self, index: &Index, data: EncryptedTable<UID_LENGTH>) -> Result<(), Error> {
        let mut size = 0;
        for (uid, value) in data {
            size += value.len();
            self.0.put(key(index, Table::Chains, &uid), value)?;
        }

        self.0.merge(size_key(index), size.to_be_bytes())?;

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

fn merge_add(
    _key: &[u8],
    existing_value: Option<&[u8]>,
    operands: &MergeOperands,
) -> Option<Vec<u8>> {
    let mut result = 0;

    if let Some(existing_value) = existing_value {
        result += match existing_value.try_into().map(usize::from_be_bytes) {
            Ok(value) => value,
            Err(_) => return None,
        };
    }

    for operand in operands {
        result += match operand.try_into().map(usize::from_be_bytes) {
            Ok(value) => value,
            Err(_) => return None,
        };
    }

    Some(result.to_be_bytes().to_vec())
}