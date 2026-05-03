use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, named_params};
use thiserror::Error;

use crate::state::ProjectRecord;

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
            create table if not exists projects (
                id text primary key,
                path text not null unique,
                name text,
                icon text,
                created_at integer not null default (unixepoch()),
                updated_at integer not null default (unixepoch())
            );
            create table if not exists project_metadata (
                project_id text not null references projects(id) on delete cascade,
                key text not null,
                value text not null,
                primary key(project_id, key)
            );
            create table if not exists workspace_bindings (
                workspace_id text primary key,
                project_id text not null references projects(id) on delete cascade,
                updated_at integer not null default (unixepoch())
            );
            insert or ignore into schema_migrations(version) values (1);
            ",
        )?;
        Ok(())
    }

    pub fn load_projects(&self) -> Result<BTreeMap<String, ProjectRecord>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("select id, path, name, icon from projects order by id")?;
        let rows = stmt.query_map([], |row| {
            Ok(ProjectRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                icon: row.get(3)?,
                metadata: BTreeMap::new(),
            })
        })?;

        let mut projects = BTreeMap::new();
        for row in rows {
            let project = row?;
            projects.insert(project.id.clone(), project);
        }

        let mut stmt = self.conn.prepare(
            "select project_id, key, value from project_metadata order by project_id, key",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (project_id, key, value) = row?;
            if let Some(project) = projects.get_mut(&project_id) {
                project.metadata.insert(key, value);
            }
        }

        Ok(projects)
    }

    pub fn load_workspace_bindings(&self) -> Result<BTreeMap<String, String>, StorageError> {
        self.load_string_map(
            "select workspace_id, project_id from workspace_bindings order by workspace_id",
        )
    }

    pub fn upsert_project(
        &mut self,
        id: &str,
        name: Option<&str>,
        icon: Option<&str>,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "
            insert into projects(id, path, name, icon) values (:id, :id, :name, :icon)
            on conflict(id) do update set
                name = coalesce(excluded.name, projects.name),
                icon = coalesce(excluded.icon, projects.icon),
                updated_at = unixepoch()
            ",
            named_params! {
                ":id": id,
                ":name": name,
                ":icon": icon,
            },
        )?;
        Ok(())
    }

    pub fn bind_workspace(
        &mut self,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<Option<String>, StorageError> {
        let previous = self
            .conn
            .query_row(
                "select project_id from workspace_bindings where workspace_id = :workspace_id",
                named_params! { ":workspace_id": workspace_id },
                |row| row.get(0),
            )
            .optional()?;
        self.conn.execute(
            "
            insert into workspace_bindings(workspace_id, project_id) values (:workspace_id, :project_id)
            on conflict(workspace_id) do update set
                project_id = excluded.project_id,
                updated_at = unixepoch()
            ",
            named_params! {
                ":workspace_id": workspace_id,
                ":project_id": project_id,
            },
        )?;
        Ok(previous)
    }

    pub fn unbind_workspace(&mut self, workspace_id: &str) -> Result<Option<String>, StorageError> {
        let previous = self
            .conn
            .query_row(
                "select project_id from workspace_bindings where workspace_id = :workspace_id",
                named_params! { ":workspace_id": workspace_id },
                |row| row.get(0),
            )
            .optional()?;
        self.conn.execute(
            "delete from workspace_bindings where workspace_id = :workspace_id",
            named_params! { ":workspace_id": workspace_id },
        )?;
        Ok(previous)
    }

    pub fn set_metadata(
        &mut self,
        project_id: &str,
        key: &str,
        value: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "
            insert into project_metadata(project_id, key, value) values (:project_id, :key, :value)
            on conflict(project_id, key) do update set value = excluded.value
            ",
            named_params! {
                ":project_id": project_id,
                ":key": key,
                ":value": value,
            },
        )?;
        Ok(())
    }

    pub fn remove_metadata(&mut self, project_id: &str, key: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "delete from project_metadata where project_id = :project_id and key = :key",
            named_params! {
                ":project_id": project_id,
                ":key": key,
            },
        )?;
        Ok(())
    }

    fn load_string_map(&self, sql: &str) -> Result<BTreeMap<String, String>, StorageError> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>().map_err(StorageError::from)
    }
}
