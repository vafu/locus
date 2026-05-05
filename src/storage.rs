use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, named_params};
use thiserror::Error;

use crate::state::Link;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("failed to create data directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("missing XDG data directory")]
    MissingDataDir,
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

#[derive(Debug)]
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    pub fn open_default() -> Result<Self, StorageError> {
        let base = dirs::data_dir().ok_or(StorageError::MissingDataDir)?;
        Self::open(base.join("locus").join("locus.db"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| StorageError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("pragma foreign_keys = ON;")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "
            create table if not exists schema_migrations (
                version integer primary key,
                applied_at integer not null default (unixepoch())
            );
            create table if not exists links (
                source text not null,
                relation text not null,
                target text not null,
                created_at integer not null default (unixepoch()),
                updated_at integer not null default (unixepoch()),
                primary key(source, relation, target)
            );
            create index if not exists links_source_relation_idx on links(source, relation);
            create index if not exists links_target_relation_idx on links(target, relation);
            create table if not exists properties (
                subject text not null,
                key text not null,
                value text not null,
                created_at integer not null default (unixepoch()),
                updated_at integer not null default (unixepoch()),
                primary key(subject, key)
            );
            create index if not exists properties_subject_idx on properties(subject);
            insert or ignore into schema_migrations(version) values (2);
            ",
        )?;
        Ok(())
    }

    pub fn load_links(&self) -> Result<BTreeSet<Link>, StorageError> {
        let mut stmt = self.conn.prepare(
            "select source, relation, target from links order by source, relation, target",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Link {
                source: row.get(0)?,
                relation: row.get(1)?,
                target: row.get(2)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    pub fn load_properties(&self) -> Result<BTreeMap<(String, String), String>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("select subject, key, value from properties order by subject, key")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }

    pub fn add_link(&mut self, link: &Link) -> Result<(), StorageError> {
        self.conn.execute(
            "
            insert into links(source, relation, target) values (:source, :relation, :target)
            on conflict(source, relation, target) do update set updated_at = unixepoch()
            ",
            named_params! {
                ":source": link.source,
                ":relation": link.relation,
                ":target": link.target,
            },
        )?;
        Ok(())
    }

    pub fn remove_link(&mut self, link: &Link) -> Result<(), StorageError> {
        self.conn.execute(
            "
            delete from links
            where source = :source and relation = :relation and target = :target
            ",
            named_params! {
                ":source": link.source,
                ":relation": link.relation,
                ":target": link.target,
            },
        )?;
        Ok(())
    }

    pub fn remove_links(&mut self, source: &str, relation: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "delete from links where source = :source and relation = :relation",
            named_params! {
                ":source": source,
                ":relation": relation,
            },
        )?;
        Ok(())
    }

    pub fn set_link(&mut self, link: &Link) -> Result<(), StorageError> {
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "delete from links where source = :source and relation = :relation",
            named_params! {
                ":source": link.source,
                ":relation": link.relation,
            },
        )?;
        transaction.execute(
            "
            insert into links(source, relation, target) values (:source, :relation, :target)
            on conflict(source, relation, target) do update set updated_at = unixepoch()
            ",
            named_params! {
                ":source": link.source,
                ":relation": link.relation,
                ":target": link.target,
            },
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_property(
        &mut self,
        subject: &str,
        key: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "
            insert into properties(subject, key, value) values (:subject, :key, :value)
            on conflict(subject, key) do update set
                value = excluded.value,
                updated_at = unixepoch()
            ",
            named_params! {
                ":subject": subject,
                ":key": key,
                ":value": value,
            },
        )?;
        Ok(())
    }

    pub fn remove_property(&mut self, subject: &str, key: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "delete from properties where subject = :subject and key = :key",
            named_params! {
                ":subject": subject,
                ":key": key,
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_foreign_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(tmp.path().join("locus.db")).unwrap();
        let enabled: bool = store
            .conn
            .query_row("pragma foreign_keys", [], |row| row.get(0))
            .unwrap();

        assert!(enabled);
    }
}
