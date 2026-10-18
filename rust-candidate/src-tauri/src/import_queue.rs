//! Durable, single-worker import requests for the original CSDM shell.
//! The source demo remains in place; interrupted jobs retry on next startup.

use crate::import::{ImportReceipt, import_native_with_control};
use demostrike::demo_import::ImportControl;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tauri::State;

const CREATE_QUEUE_TABLE: &str = "CREATE TABLE IF NOT EXISTS import_jobs (
    id INTEGER PRIMARY KEY,
    demo_path TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('queued','running','completed','failed','cancelled')),
    recovered INTEGER NOT NULL DEFAULT 0,
    receipt TEXT,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportJob {
    id: i64,
    demo_path: String,
    state: String,
    recovered: bool,
    receipt: Option<ImportReceipt>,
    error: Option<String>,
    created_at: String,
}

pub struct ImportQueue {
    database: PathBuf,
    library: PathBuf,
    active: AtomicBool,
    current: Mutex<Option<(i64, ImportControl)>>,
}

impl ImportQueue {
    pub fn new(directory: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let queue = Self {
            database: directory.join("demostrike-import-queue.sqlite"),
            library: directory.join("demostrike-library.sqlite"),
            active: AtomicBool::new(false),
            current: Mutex::new(None),
        };
        let mut connection = queue.open()?;
        Self::migrate_cancelled_state(&mut connection)?;
        // A killed process cannot have committed a successful result. The
        // native import transaction handles an interrupted database write.
        connection
            .execute(
                "UPDATE import_jobs SET state = 'queued', recovered = 1 WHERE state = 'running'",
                [],
            )
            .map_err(|error| error.to_string())?;
        Ok(queue)
    }

    fn open(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.database).map_err(|error| error.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(CREATE_QUEUE_TABLE)
            .map_err(|error| error.to_string())?;
        Ok(connection)
    }

    fn migrate_cancelled_state(connection: &mut Connection) -> Result<(), String> {
        let definition: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='import_jobs'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if definition.contains("'cancelled'") {
            return Ok(());
        }
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute_batch("ALTER TABLE import_jobs RENAME TO import_jobs_before_cancel;")
            .map_err(|error| error.to_string())?;
        transaction
            .execute_batch(CREATE_QUEUE_TABLE)
            .map_err(|error| error.to_string())?;
        transaction
            .execute_batch(
                "INSERT INTO import_jobs(id,demo_path,state,recovered,receipt,error,created_at)
                 SELECT id,demo_path,state,recovered,receipt,error,created_at
                 FROM import_jobs_before_cancel;
                 DROP TABLE import_jobs_before_cancel;",
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    }

    fn read_job(connection: &Connection, id: i64) -> Result<ImportJob, String> {
        connection.query_row(
            "SELECT id, demo_path, state, recovered, receipt, error, created_at FROM import_jobs WHERE id = ?1",
            [id], |row| {
                let receipt: Option<String> = row.get(4)?;
                Ok(ImportJob {
                    id: row.get(0)?, demo_path: row.get(1)?, state: row.get(2)?,
                    recovered: row.get(3)?,
                    receipt: receipt.and_then(|text| serde_json::from_str(&text).ok()),
                    error: row.get(5)?, created_at: row.get(6)?,
                })
            },
        ).map_err(|error| error.to_string())
    }

    pub fn enqueue(self: &Arc<Self>, demo_path: String) -> Result<ImportJob, String> {
        let demo_path = demo_path.trim().to_owned();
        if demo_path.is_empty() {
            return Err("Choose a CS2 .dem file first.".into());
        }
        let connection = self.open()?;
        let existing: Option<i64> = connection.query_row(
            "SELECT id FROM import_jobs WHERE demo_path = ?1 AND state IN ('queued','running') ORDER BY id LIMIT 1",
            [&demo_path], |row| row.get(0),
        ).optional().map_err(|error| error.to_string())?;
        let id = if let Some(id) = existing {
            id
        } else {
            connection
                .execute(
                    "INSERT INTO import_jobs(demo_path,state) VALUES (?1,'queued')",
                    [&demo_path],
                )
                .map_err(|error| error.to_string())?;
            connection.last_insert_rowid()
        };
        let job = Self::read_job(&connection, id)?;
        self.start();
        Ok(job)
    }

    pub fn list(self: &Arc<Self>) -> Result<Vec<ImportJob>, String> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare("SELECT id FROM import_jobs ORDER BY id DESC LIMIT 30")
            .map_err(|error| error.to_string())?;
        let ids = statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let jobs = ids
            .into_iter()
            .map(|id| Self::read_job(&connection, id))
            .collect::<Result<Vec<_>, _>>()?;
        self.start();
        Ok(jobs)
    }

    pub fn cancel(&self, id: i64) -> Result<ImportJob, String> {
        // Claim and cancellation share this lock: a job cannot become
        // "running" between the state read and control registration.
        let current = self.current.lock().map_err(|error| error.to_string())?;
        let connection = self.open()?;
        let job = Self::read_job(&connection, id)?;
        match job.state.as_str() {
            "queued" => {
                connection
                    .execute(
                        "UPDATE import_jobs SET state='cancelled', error=NULL
                         WHERE id=?1 AND state='queued'",
                        [id],
                    )
                    .map_err(|error| error.to_string())?;
            }
            "running" => {
                let Some((active_id, control)) = current.as_ref() else {
                    return Err("Import is changing state; try cancelling again.".into());
                };
                if *active_id != id {
                    return Err("Import is changing state; try cancelling again.".into());
                }
                if !control.request_cancel() {
                    return Err("Import is finalizing and can no longer be cancelled.".into());
                }
                connection
                    .execute(
                        "UPDATE import_jobs SET state='cancelled', error=NULL
                         WHERE id=?1 AND state='running'",
                        [id],
                    )
                    .map_err(|error| error.to_string())?;
            }
            "cancelled" => return Ok(job),
            _ => return Err("Only queued or running imports can be cancelled.".into()),
        }
        Self::read_job(&connection, id)
    }

    fn claim(&self) -> Result<Option<(i64, String, bool)>, String> {
        let mut connection = self.open()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let next: Option<(i64, String, bool)> = transaction
            .query_row(
                "SELECT id, demo_path, recovered FROM import_jobs WHERE state = 'queued' ORDER BY id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((id, _, _)) = next {
            transaction
                .execute(
                    "UPDATE import_jobs SET state = 'running' WHERE id = ?1",
                    [id],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(next)
    }

    fn finish(&self, id: i64, result: Result<ImportReceipt, String>) -> Result<(), String> {
        let connection = self.open()?;
        let succeeded = result.is_ok();
        let changed = match result {
            Ok(receipt) => {
                let json = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
                connection
                    .execute(
                        "UPDATE import_jobs SET state='completed', receipt=?2, error=NULL
                    WHERE id=?1 AND state='running'",
                        params![id, json],
                    )
                    .map_err(|error| error.to_string())?
            }
            Err(error) => connection
                .execute(
                    "UPDATE import_jobs SET state='failed', receipt=NULL, error=?2
                         WHERE id=?1 AND state='running'",
                    params![id, error],
                )
                .map_err(|error| error.to_string())?,
        };
        if changed == 0 {
            let state = Self::read_job(&connection, id)?.state;
            if state != "cancelled" || succeeded {
                return Err(format!(
                    "Import job {id} changed state before completion: {state}"
                ));
            }
        }
        Ok(())
    }

    fn recovered_receipt(&self, source: &str) -> Option<ImportReceipt> {
        let library = self.library.to_string_lossy();
        let snapshot = crate::library::list_matches(&library).ok()?;
        let match_row = snapshot
            .matches
            .into_iter()
            .find(|item| item.demo_file_path.as_deref() == Some(source))?;
        Some(ImportReceipt {
            path: library.into_owned(),
            match_id: match_row.id,
            map_name: match_row.map_name,
            player_count: usize::try_from(match_row.player_count).ok()?,
            round_count: usize::try_from(match_row.round_count).ok()?,
        })
    }

    pub fn start(self: &Arc<Self>) {
        if self.active.swap(true, Ordering::AcqRel) {
            return;
        }
        let queue = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                let claimed = match queue.current.lock() {
                    Ok(mut current) => match queue.claim() {
                        Ok(Some((id, path, recovered))) => {
                            let control = ImportControl::new();
                            *current = Some((id, control.clone()));
                            Ok(Some((id, path, recovered, control)))
                        }
                        Ok(None) => Ok(None),
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error.to_string()),
                };
                match claimed {
                    Ok(Some((id, path, recovered, control))) => {
                        let result =
                            import_native_with_control(Path::new(&path), &queue.library, &control);
                        let result = match result {
                            Err(error) if recovered && error.contains("already imported") => {
                                queue.recovered_receipt(&path).ok_or(error)
                            }
                            other => other,
                        };
                        if let Err(error) = queue.finish(id, result) {
                            eprintln!("Could not persist import job {id}: {error}");
                        }
                        match queue.current.lock() {
                            Ok(mut current) => {
                                if current
                                    .as_ref()
                                    .is_some_and(|(active_id, _)| *active_id == id)
                                {
                                    *current = None;
                                }
                            }
                            Err(error) => eprintln!("Could not clear import control: {error}"),
                        }
                    }
                    Ok(None) => {
                        queue.active.store(false, Ordering::Release);
                        // Close the enqueue/empty-queue race without two workers.
                        let has_pending = queue
                            .open()
                            .and_then(|connection| {
                                connection.query_row(
                            "SELECT EXISTS(SELECT 1 FROM import_jobs WHERE state='queued')", [],
                            |row| row.get::<_, bool>(0),
                        ).map_err(|error| error.to_string())
                            })
                            .unwrap_or(false);
                        if has_pending && !queue.active.swap(true, Ordering::AcqRel) {
                            continue;
                        }
                        break;
                    }
                    Err(error) => {
                        eprintln!("Could not read import queue: {error}");
                        queue.active.store(false, Ordering::Release);
                        break;
                    }
                }
            }
        });
    }
}

#[tauri::command]
pub fn csdm_enqueue_demo(
    queue: State<'_, Arc<ImportQueue>>,
    demo_path: String,
) -> Result<ImportJob, String> {
    queue.enqueue(demo_path)
}

#[tauri::command]
pub fn csdm_list_import_jobs(queue: State<'_, Arc<ImportQueue>>) -> Result<Vec<ImportJob>, String> {
    queue.list()
}

#[tauri::command]
pub fn csdm_cancel_import(
    queue: State<'_, Arc<ImportQueue>>,
    job_id: i64,
) -> Result<ImportJob, String> {
    queue.cancel(job_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_queue_job_is_recovered_without_losing_history() {
        let temporary = tempfile::tempdir().unwrap();
        let first = ImportQueue::new(temporary.path()).unwrap();
        let connection = first.open().unwrap();
        connection
            .execute(
                "INSERT INTO import_jobs(demo_path,state) VALUES ('a.dem','running')",
                [],
            )
            .unwrap();
        connection.execute("INSERT INTO import_jobs(demo_path,state,error) VALUES ('b.dem','failed','bad demo')", []).unwrap();
        drop(connection);
        let recovered = ImportQueue::new(temporary.path()).unwrap();
        let connection = recovered.open().unwrap();
        assert_eq!(
            ImportQueue::read_job(&connection, 1).unwrap().state,
            "queued"
        );
        let recovered_flag: bool = connection
            .query_row("SELECT recovered FROM import_jobs WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(recovered_flag);
        assert!(ImportQueue::read_job(&connection, 1).unwrap().recovered);
        assert!(!ImportQueue::read_job(&connection, 2).unwrap().recovered);
        assert_eq!(
            ImportQueue::read_job(&connection, 2)
                .unwrap()
                .error
                .as_deref(),
            Some("bad demo")
        );
    }

    #[test]
    fn legacy_queue_schema_migrates_without_losing_jobs() {
        let temporary = tempfile::tempdir().unwrap();
        let database = temporary.path().join("demostrike-import-queue.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE import_jobs (
                id INTEGER PRIMARY KEY,
                demo_path TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN ('queued','running','completed','failed')),
                recovered INTEGER NOT NULL DEFAULT 0,
                receipt TEXT,
                error TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            );
            INSERT INTO import_jobs(demo_path,state,error)
            VALUES ('bad.dem','failed','invalid source');
            INSERT INTO import_jobs(demo_path,state)
            VALUES ('recover.dem','running');",
            )
            .unwrap();
        drop(connection);
        let queue = ImportQueue::new(temporary.path()).unwrap();
        let connection = queue.open().unwrap();
        assert_eq!(
            ImportQueue::read_job(&connection, 1)
                .unwrap()
                .error
                .as_deref(),
            Some("invalid source")
        );
        assert_eq!(
            ImportQueue::read_job(&connection, 2).unwrap().state,
            "queued"
        );
        assert!(ImportQueue::read_job(&connection, 2).unwrap().recovered);
        drop(connection);
        assert_eq!(queue.cancel(2).unwrap().state, "cancelled");
        let reopened = ImportQueue::new(temporary.path()).unwrap();
        assert_eq!(
            ImportQueue::read_job(&reopened.open().unwrap(), 2)
                .unwrap()
                .state,
            "cancelled"
        );
    }

    #[test]
    fn queued_cancellation_is_terminal_and_cannot_be_claimed() {
        let temporary = tempfile::tempdir().unwrap();
        let queue = ImportQueue::new(temporary.path()).unwrap();
        let connection = queue.open().unwrap();
        connection
            .execute(
                "INSERT INTO import_jobs(demo_path,state) VALUES ('queued.dem','queued')",
                [],
            )
            .unwrap();
        drop(connection);
        assert_eq!(queue.cancel(1).unwrap().state, "cancelled");
        assert_eq!(queue.cancel(1).unwrap().state, "cancelled");
        assert!(queue.claim().unwrap().is_none());
        assert!(
            ImportQueue::read_job(&queue.open().unwrap(), 1)
                .unwrap()
                .receipt
                .is_none()
        );
    }

    #[test]
    fn running_cancellation_keeps_job_cancelled_and_skips_import() {
        let temporary = tempfile::tempdir().unwrap();
        let queue = ImportQueue::new(temporary.path()).unwrap();
        let demo = temporary.path().join("running.dem");
        std::fs::write(&demo, b"PBDEMS2\0cancelled fixture").unwrap();
        let connection = queue.open().unwrap();
        connection
            .execute(
                "INSERT INTO import_jobs(demo_path,state) VALUES (?1,'running')",
                [demo.to_string_lossy().as_ref()],
            )
            .unwrap();
        drop(connection);
        let control = ImportControl::new();
        *queue.current.lock().unwrap() = Some((1, control.clone()));
        assert_eq!(queue.cancel(1).unwrap().state, "cancelled");
        assert!(control.is_cancelled());
        let result = import_native_with_control(&demo, &queue.library, &control);
        assert!(result.unwrap_err().contains("cancelled"));
        queue
            .finish(1, Err("demo import cancelled".into()))
            .unwrap();
        let reopened = ImportQueue::new(temporary.path()).unwrap();
        assert_eq!(
            ImportQueue::read_job(&reopened.open().unwrap(), 1)
                .unwrap()
                .state,
            "cancelled"
        );
        assert!(
            !queue.library.exists(),
            "cancelled source must not create a match library"
        );
    }
}
