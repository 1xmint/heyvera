//! Independent SQLite ownership boundary for HeyVera Socials.
//!
//! This crate deliberately has no dependency on Cortex. It owns the Socials
//! schema, verifies its checksummed baseline, inspects a legacy mixed database
//! through a read-only handle, and exports only an explicit table allowlist.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::types::Value;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const BASELINE_ID: &str = "0001_baseline";
pub const BASELINE_SQL: &str = include_str!("../migrations/0001_baseline.sql");

/// The complete Socials domain-table contract. The migration ledger and FTS
/// shadow tables are derived infrastructure and therefore excluded.
pub const SOCIAL_TABLES: [&str; 43] = [
    "accounts",
    "billing_history",
    "code_redemptions",
    "promo_codes",
    "pulse_audit_log",
    "pulse_drafts",
    "pulse_goals",
    "pulse_schedules",
    "referral_codes",
    "social_blocks",
    "social_bookmarks",
    "social_communities",
    "social_community_invites",
    "social_community_memberships",
    "social_conversation_activity_clock",
    "social_conversation_participants",
    "social_conversations",
    "social_direct_message_starts",
    "social_follow_requests",
    "social_follows",
    "social_likes",
    "social_linked_agents",
    "social_live_sessions",
    "social_longform",
    "social_media_objects",
    "social_media_shelves",
    "social_message_request_clock",
    "social_message_requests",
    "social_messages",
    "social_mutes",
    "social_notifications",
    "social_page_follows",
    "social_pages",
    "social_post_media",
    "social_posts",
    "social_profile_prefs",
    "social_profiles",
    "social_reports",
    "social_reposts",
    "social_ws_tickets",
    "social_x402_receipts",
    "subscriptions",
    "webhook_events",
];

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("destination database already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("database has tables but no HeyVera migration ledger")]
    LegacyOrPartialSchema,
    #[error("migration {id} has checksum {actual}, expected {expected}")]
    ChecksumMismatch {
        id: String,
        expected: String,
        actual: String,
    },
    #[error("migration ledger does not contain {BASELINE_ID}")]
    MissingBaseline,
    #[error("Socials schema is missing tables: {0:?}")]
    MissingTables(Vec<String>),
    #[error("Socials schema contains forbidden tables: {0:?}")]
    UnexpectedTables(Vec<String>),
    #[error("source table {table} columns do not match the Socials baseline")]
    ColumnMismatch { table: String },
    #[error("SQLite integrity check failed: {0:?}")]
    Integrity(Vec<String>),
    #[error("SQLite foreign-key check found {0} violation(s)")]
    ForeignKeys(usize),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct SocialDatabase {
    connection: Mutex<Connection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryKeyRange {
    None,
    Integer { minimum: i64, maximum: i64 },
    Opaque { populated_rows: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInspection {
    pub name: String,
    pub row_count: i64,
    pub primary_key_columns: Vec<String>,
    pub primary_key_range: PrimaryKeyRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInspection {
    pub sqlite_version: String,
    pub journal_mode: String,
    pub wal_present: bool,
    pub wal_bytes: Option<u64>,
    pub legacy_schema_version: Option<i64>,
    pub foreign_key_violations: usize,
    pub tables: Vec<TableInspection>,
    /// Mixed tables are reported only by name and count. Row contents and
    /// identifiers never enter the report.
    pub mixed_table_counts: Vec<(String, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    pub copied_rows: Vec<(String, i64)>,
    pub source: SourceInspection,
}

impl SocialDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        configure(&connection)?;
        migrate(&mut connection)?;
        validate_connection(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn baseline_checksum() -> String {
        checksum(BASELINE_SQL)
    }

    pub fn row_count(&self, table: &str) -> Result<i64> {
        if !SOCIAL_TABLES.contains(&table) {
            return Err(Error::UnexpectedTables(vec![table.to_string()]));
        }
        count_rows(
            &self.connection.lock().expect("Socials database lock"),
            table,
        )
    }

    pub fn rebuild_search_index(&self) -> Result<()> {
        rebuild_search_index(&mut self.connection.lock().expect("Socials database lock"))
    }

    pub fn verify(&self) -> Result<()> {
        validate_connection(&self.connection.lock().expect("Socials database lock"))
    }
}

/// Inspect a source without ever obtaining a write-capable handle.
pub fn inspect_source(path: impl AsRef<Path>) -> Result<SourceInspection> {
    let path = path.as_ref();
    let connection = open_read_only(path)?;
    let sqlite_version = connection.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    let journal_mode = connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    let legacy_schema_version = if table_exists(&connection, "schema_version")? {
        Some(connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?)
    } else {
        None
    };
    let foreign_key_violations = foreign_key_violation_count(&connection)?;
    let mut tables = Vec::with_capacity(SOCIAL_TABLES.len());
    for table in SOCIAL_TABLES {
        if table_exists(&connection, table)? {
            tables.push(inspect_table(&connection, table)?);
        }
    }
    let mut mixed_table_counts = Vec::new();
    for table in ["audit_log", "usage_events"] {
        if table_exists(&connection, table)? {
            mixed_table_counts.push((table.to_string(), count_rows(&connection, table)?));
        }
    }
    let wal_metadata = std::fs::metadata(with_suffix(path, "-wal")).ok();
    Ok(SourceInspection {
        sqlite_version,
        journal_mode,
        wal_present: wal_metadata.is_some(),
        wal_bytes: wal_metadata.map(|metadata| metadata.len()),
        legacy_schema_version,
        foreign_key_violations,
        tables,
        mixed_table_counts,
    })
}

/// Export all 43 Socials tables into a newly-created v1 database.
pub fn export_socials(
    source_path: impl AsRef<Path>,
    destination_path: impl AsRef<Path>,
) -> Result<ExportReport> {
    let source_path = source_path.as_ref();
    let destination_path = destination_path.as_ref();
    if destination_path.exists() {
        return Err(Error::DestinationExists(destination_path.to_path_buf()));
    }
    let source_report = inspect_source(source_path)?;
    let found = source_report
        .tables
        .iter()
        .map(|table| table.name.as_str())
        .collect::<BTreeSet<_>>();
    let missing = SOCIAL_TABLES
        .iter()
        .filter(|table| !found.contains(**table))
        .map(|table| (*table).to_string())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Error::MissingTables(missing));
    }

    let source = open_read_only(source_path)?;
    let mut destination = Connection::open(destination_path)?;
    configure(&destination)?;
    migrate(&mut destination)?;
    destination.pragma_update(None, "foreign_keys", "OFF")?;

    // Runtime triggers make historical row order significant. Preserve their
    // exact SQL, remove them for the copy, and restore them before validation.
    let triggers = schema_object_sql(&destination, "trigger")?;
    for (name, _) in &triggers {
        destination.execute_batch(&format!("DROP TRIGGER {}", quoted(name)))?;
    }
    let copied_rows = {
        let transaction = destination.transaction()?;
        transaction.execute("DELETE FROM social_conversation_activity_clock", [])?;
        transaction.execute("DELETE FROM social_message_request_clock", [])?;
        let mut counts = Vec::with_capacity(SOCIAL_TABLES.len());
        for table in SOCIAL_TABLES {
            counts.push((table.to_string(), copy_table(&source, &transaction, table)?));
        }
        transaction.commit()?;
        counts
    };
    for (_, sql) in triggers {
        destination.execute_batch(&sql)?;
    }
    destination.pragma_update(None, "foreign_keys", "ON")?;
    rebuild_search_index(&mut destination)?;
    validate_connection(&destination)?;
    Ok(ExportReport {
        copied_rows,
        source: source_report,
    })
}

fn open_read_only(path: &Path) -> Result<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn configure(connection: &Connection) -> Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<()> {
    let tables = user_tables(connection)?;
    if !tables.contains("schema_migrations") {
        if !tables.is_empty() {
            return Err(Error::LegacyOrPartialSchema);
        }
        let transaction = connection.transaction()?;
        transaction.execute_batch(BASELINE_SQL)?;
        transaction.execute(
            "INSERT INTO schema_migrations(id, checksum, applied_at) VALUES (?1, ?2, unixepoch())",
            params![BASELINE_ID, checksum(BASELINE_SQL)],
        )?;
        transaction.commit()?;
        return Ok(());
    }
    let actual = connection
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE id = ?1",
            [BASELINE_ID],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(actual) = actual else {
        return Err(Error::MissingBaseline);
    };
    let expected = checksum(BASELINE_SQL);
    if actual != expected {
        return Err(Error::ChecksumMismatch {
            id: BASELINE_ID.to_string(),
            expected,
            actual,
        });
    }
    Ok(())
}

fn validate_connection(connection: &Connection) -> Result<()> {
    validate_table_allowlist(connection)?;
    let integrity = connection
        .prepare("PRAGMA integrity_check")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if integrity.as_slice() != ["ok"] {
        return Err(Error::Integrity(integrity));
    }
    let violations = foreign_key_violation_count(connection)?;
    if violations != 0 {
        return Err(Error::ForeignKeys(violations));
    }
    Ok(())
}

fn validate_table_allowlist(connection: &Connection) -> Result<()> {
    let actual = user_tables(connection)?;
    let expected = SOCIAL_TABLES
        .iter()
        .map(|table| (*table).to_string())
        .collect::<BTreeSet<_>>();
    let missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Error::MissingTables(missing));
    }
    let unexpected = actual
        .difference(&expected)
        .filter(|table| {
            table.as_str() != "schema_migrations"
                && table.as_str() != "social_posts_fts"
                && !table.starts_with("social_posts_fts_")
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unexpected.is_empty() {
        return Err(Error::UnexpectedTables(unexpected));
    }
    Ok(())
}

fn user_tables(connection: &Connection) -> Result<BTreeSet<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    Ok(tables)
}

fn schema_object_sql(connection: &Connection, object_type: &str) -> Result<Vec<(String, String)>> {
    let mut statement = connection.prepare(
        "SELECT name, sql FROM sqlite_schema WHERE type = ?1 AND sql IS NOT NULL ORDER BY rowid",
    )?;
    let objects = statement
        .query_map([object_type], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(objects)
}

fn copy_table(source: &Connection, destination: &Transaction<'_>, table: &str) -> Result<i64> {
    let source_columns = columns(source, table)?;
    let destination_columns = columns(destination, table)?;
    if source_columns != destination_columns {
        return Err(Error::ColumnMismatch {
            table: table.to_string(),
        });
    }
    let column_list = source_columns
        .iter()
        .map(|column| quoted(column))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=source_columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut select = source.prepare(&format!("SELECT {column_list} FROM {}", quoted(table)))?;
    let mut insert = destination.prepare(&format!(
        "INSERT INTO {} ({column_list}) VALUES ({placeholders})",
        quoted(table)
    ))?;
    let mut rows = select.query([])?;
    let mut copied = 0;
    while let Some(row) = rows.next()? {
        let values = (0..source_columns.len())
            .map(|index| row.get::<_, Value>(index))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        insert.execute(rusqlite::params_from_iter(values.iter()))?;
        copied += 1;
    }
    Ok(copied)
}

fn columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({})", quoted(table)))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(columns)
}

fn inspect_table(connection: &Connection, table: &str) -> Result<TableInspection> {
    let row_count = count_rows(connection, table)?;
    let mut statement = connection.prepare(&format!("PRAGMA table_info({})", quoted(table)))?;
    let info = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut primary_keys = info
        .iter()
        .filter(|(_, _, position)| *position > 0)
        .collect::<Vec<_>>();
    primary_keys.sort_by_key(|(_, _, position)| *position);
    let primary_key_columns = primary_keys
        .iter()
        .map(|(name, _, _)| name.clone())
        .collect::<Vec<_>>();
    let primary_key_range = match primary_keys.as_slice() {
        [(column, declared_type, _)] if declared_type.eq_ignore_ascii_case("INTEGER") => {
            let sql = format!(
                "SELECT MIN({0}), MAX({0}) FROM {1}",
                quoted(column),
                quoted(table)
            );
            match connection.query_row(&sql, [], |row| {
                Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?))
            })? {
                (Some(minimum), Some(maximum)) => PrimaryKeyRange::Integer { minimum, maximum },
                _ => PrimaryKeyRange::None,
            }
        }
        [] => PrimaryKeyRange::None,
        _ => PrimaryKeyRange::Opaque {
            populated_rows: row_count,
        },
    };
    Ok(TableInspection {
        name: table.to_string(),
        row_count,
        primary_key_columns,
        primary_key_range,
    })
}

fn rebuild_search_index(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM social_posts_fts", [])?;
    transaction.execute(
        "INSERT INTO social_posts_fts(post_id, body) SELECT id, body FROM social_posts WHERE deleted_at IS NULL", [])?;
    transaction.commit()?;
    Ok(())
}

fn count_rows(connection: &Connection, table: &str) -> Result<i64> {
    Ok(connection.query_row(
        &format!("SELECT COUNT(*) FROM {}", quoted(table)),
        [],
        |row| row.get(0),
    )?)
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn foreign_key_violation_count(connection: &Connection) -> Result<usize> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let mut rows = statement.query([])?;
    let mut count = 0;
    while rows.next()?.is_some() {
        count += 1;
    }
    Ok(count)
}

fn checksum(sql: &str) -> String {
    // Git may materialize the SQL with CRLF on Windows. Migration identity is
    // content identity, not checkout line-ending identity.
    let canonical = sql.replace("\r\n", "\n");
    Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
