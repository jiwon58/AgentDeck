use crate::{
    diagnostics,
    model::{error, Result},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

#[derive(Clone, Copy)]
pub enum Table {
    Roots,
    Runtimes,
    Routers,
    Sessions,
    Settings,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Self::Roots => "agent_roots",
            Self::Runtimes => "runtime_installations",
            Self::Routers => "router_profiles",
            Self::Sessions => "session_definitions",
            Self::Settings => "application_settings",
        }
    }
}

pub struct Store(Mutex<Connection>);

fn sqlite_error(err: rusqlite::Error) -> String {
    diagnostics::record(
        "WARN",
        "storage.sqlite_failed",
        None,
        err.sqlite_error().map(|code| i64::from(code.extended_code)),
        None,
    );
    error(err)
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path).map_err(sqlite_error)?;
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(sqlite_error)?;
        match version {
            0 => {
                let transaction = db.transaction().map_err(sqlite_error)?;
                transaction
                    .execute_batch(include_str!("../migrations/001_initial.sql"))
                    .map_err(sqlite_error)?;
                transaction.commit().map_err(sqlite_error)?;
            }
            1 => {}
            _ => return Err("The database belongs to a newer Agent Deck version".into()),
        }
        Ok(Self(Mutex::new(db)))
    }

    fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0.lock().map_err(error)
    }

    pub fn list<T: DeserializeOwned>(&self, table: Table) -> Result<Vec<T>> {
        let db = self.db()?;
        let mut query = db
            .prepare(&format!(
                "SELECT document FROM {} ORDER BY rowid DESC",
                table.name()
            ))
            .map_err(sqlite_error)?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sqlite_error)?;
        rows.map(|row| serde_json::from_str(&row.map_err(sqlite_error)?).map_err(error))
            .collect()
    }

    pub fn get<T: DeserializeOwned>(&self, table: Table, id: &str) -> Result<Option<T>> {
        self.db()?
            .query_row(
                &format!("SELECT document FROM {} WHERE id = ?1", table.name()),
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_error)?
            .map(|value| serde_json::from_str(&value).map_err(error))
            .transpose()
    }

    pub fn put(&self, table: Table, id: &str, value: &impl Serialize) -> Result<()> {
        let json = serde_json::to_string(value).map_err(error)?;
        self.db()?.execute(&format!("INSERT INTO {} (id, document) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET document = excluded.document", table.name()), params![id, json]).map_err(sqlite_error)?;
        Ok(())
    }

    pub fn delete(&self, table: Table, id: &str) -> Result<()> {
        self.db()?
            .execute(&format!("DELETE FROM {} WHERE id = ?1", table.name()), [id])
            .map_err(sqlite_error)?;
        Ok(())
    }

    pub fn import_settings(
        &self,
        preferences: &crate::model::Preferences,
        routers: &[crate::model::Router],
    ) -> Result<()> {
        let mut db = self.db()?;
        let transaction = db.transaction().map_err(sqlite_error)?;
        transaction.execute("INSERT INTO application_settings (id, document) VALUES ('terminal', ?1) ON CONFLICT(id) DO UPDATE SET document = excluded.document",
            [serde_json::to_string(preferences).map_err(error)?]).map_err(sqlite_error)?;
        for profile in routers {
            transaction
                .execute(
                    "INSERT INTO router_profiles (id, document) VALUES (?1, ?2)",
                    params![profile.id, serde_json::to_string(profile).map_err(error)?],
                )
                .map_err(sqlite_error)?;
        }
        transaction.commit().map_err(sqlite_error)
    }
}
