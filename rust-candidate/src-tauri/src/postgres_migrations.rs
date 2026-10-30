//! Mechanical Rust ports of the first ordered CS Demo Manager v1 migrations.
//! Original source: src/node/database/migrations/v1/create-timestamps-table.ts.
//! The upstream MIT license is preserved in the pinned source tree.

/// Apply the original timestamps-table migration in the caller's transaction.
/// This individual step does not record schema version 1.
pub fn create_timestamps_table(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    transaction.batch_execute(
        "CREATE TABLE IF NOT EXISTS \"timestamps\" (\"name\" varchar NOT NULL UNIQUE PRIMARY KEY, \"date\" timestamptz NOT NULL DEFAULT now())",
    )
}

/// Apply the original v1 demos-table migration in the caller's transaction.
/// This does not mark schema version 1 complete.
pub fn create_demos_table(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    transaction.batch_execute(
        "CREATE TABLE IF NOT EXISTS \"demos\" (\"checksum\" varchar NOT NULL PRIMARY KEY, \"name\" varchar NOT NULL, \"game\" varchar NOT NULL, \"source\" varchar NOT NULL, \"type\" varchar NOT NULL, \"date\" timestamptz NOT NULL, \"map_name\" varchar NOT NULL, \"tick_count\" integer NOT NULL, \"tickrate\" double precision NOT NULL, \"framerate\" double precision NOT NULL, \"duration\" double precision NOT NULL, \"server_name\" varchar NOT NULL, \"client_name\" varchar NOT NULL, \"network_protocol\" integer NOT NULL DEFAULT 0, \"build_number\" integer NOT NULL DEFAULT 0, \"share_code\" varchar DEFAULT '')",
    )
}

/// Apply the original v1 demo-paths-table migration in the caller's transaction.
pub fn create_demo_paths_table(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    transaction.batch_execute(
        "CREATE TABLE IF NOT EXISTS \"demo_paths\" (\"checksum\" varchar NOT NULL, \"file_path\" varchar NOT NULL, CONSTRAINT \"demo_paths_pk\" PRIMARY KEY (\"checksum\", \"file_path\"))",
    )
}

/// Apply the available ordered v1 prefix within one caller-owned transaction.
/// This is incomplete and must never mark schema version 1 as complete.
pub fn apply_v1_available_steps(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    create_timestamps_table(transaction)?;
    create_demos_table(transaction)?;
    create_demo_paths_table(transaction)?;
    Ok(())
}

/// Apply all original v1 operations in source order in the caller's transaction.
/// The original driver writes its current-version marker only after the full
/// version chain succeeds; this function intentionally does not write one.
fn ensure_migrations_table(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    transaction.batch_execute("create table if not exists \"migrations\" (\"schema_version\" integer not null unique, \"run_at\" timestamptz not null)")
}
fn apply_v1_modules(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    apply_v1_available_steps(transaction)?;
    transaction.batch_execute(include_str!("postgres_v1_remaining.sql"))
}
pub fn apply_v1(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    ensure_migrations_table(transaction)?;
    apply_v1_modules(transaction)
}

/// Apply the original version-2 round-tags migration in the caller's transaction.
/// Version recording remains the responsibility of the complete migration driver.
pub fn apply_v2(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    transaction.batch_execute(
        "CREATE TABLE IF NOT EXISTS \"round_tags\" (\"checksum\" varchar NOT NULL, \"round_number\" integer NOT NULL, \"tag_id\" int8 NOT NULL, CONSTRAINT \"round_tags_checksum_round_number_tag_id_unique\" UNIQUE (\"checksum\", \"round_number\", \"tag_id\"))",
    )
}

include!("postgres_v3_default_maps.rs");

/// Failure from the original v3 table-introspection or PostgreSQL operations.
#[derive(Debug)]
pub enum V3MigrationError { Database(postgres::Error), MapsTableNotFound }
impl std::fmt::Display for V3MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Database(error) => write!(f, "{error}"), Self::MapsTableNotFound => write!(f, "Table maps not found") }
    }
}
impl std::error::Error for V3MigrationError {}
impl From<postgres::Error> for V3MigrationError { fn from(error:postgres::Error)->Self {Self::Database(error)} }

/// Apply the original v3 conditional map-threshold migration in the caller's transaction.
pub fn apply_v3(transaction: &mut postgres::Transaction<'_>) -> Result<(), V3MigrationError> {
    // Run the original Kysely catalog scan, including its metadata expressions
    // and schema ordering, before selecting the first table named maps.
    let rows=inspect_original_tables(transaction)?;
    let mut first_schema: Option<String> = None;
    for row in &rows {
        let table: String=row.try_get(3)?;
        if table != "maps" {continue;}
        let schema: String=row.try_get(5)?;
        match &first_schema {
            None => first_schema=Some(schema.clone()),
            Some(first) if *first != schema => break,
            _ => {}
        }
        let column:String=row.try_get(0)?;
        if column == "threshold_z" {return Ok(());}
    }
    if first_schema.is_none() {return Err(V3MigrationError::MapsTableNotFound);}
    transaction.batch_execute("ALTER TABLE \"maps\" ADD COLUMN \"threshold_z\" integer NOT NULL DEFAULT 0")?;
    for (name, threshold) in V3_DEFAULT_MAP_THRESHOLDS {
        transaction.execute("UPDATE \"maps\" SET \"threshold_z\"=$1 WHERE \"name\"=$2", &[threshold, name])?;
    }
    Ok(())
}

fn inspect_original_tables(transaction: &mut postgres::Transaction<'_>) -> Result<Vec<postgres::Row>, postgres::Error> {
    transaction.query(r###"select "a"."attname" as "column", "a"."attnotnull" as "not_null", "a"."atthasdef" as "has_default", "c"."relname" as "table", "c"."relkind" as "table_type", "ns"."nspname" as "schema", "typ"."typname" as "type", "dtns"."nspname" as "type_schema", col_description(a.attrelid, a.attnum) as "column_description", pg_get_serial_sequence(quote_ident(ns.nspname) || '.' || quote_ident(c.relname), a.attname) as "auto_incrementing" from "pg_catalog"."pg_attribute" as "a" inner join "pg_catalog"."pg_class" as "c" on "a"."attrelid" = "c"."oid" inner join "pg_catalog"."pg_namespace" as "ns" on "c"."relnamespace" = "ns"."oid" inner join "pg_catalog"."pg_type" as "typ" on "a"."atttypid" = "typ"."oid" inner join "pg_catalog"."pg_namespace" as "dtns" on "typ"."typnamespace" = "dtns"."oid" where "c"."relkind" in ('r', 'v', 'p', 'f') and "ns"."nspname" !~ '^pg_' and "ns"."nspname" != 'information_schema' and "ns"."nspname" != 'crdb_internal' and has_schema_privilege(ns.nspname, 'USAGE') and "a"."attnum" >= 0 and "a"."attisdropped" != TRUE and "c"."relname" != 'kysely_migration' and "c"."relname" != 'kysely_migration_lock' order by "ns"."nspname", "c"."relname", "a"."attnum""###, &[])
}

macro_rules! original_sql_migration {
    ($function:ident, $source:literal) => {
        pub fn $function(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
            transaction.batch_execute(include_str!($source))
        }
    };
}
original_sql_migration!(apply_v4, "postgres_v4.sql");
original_sql_migration!(apply_v5, "postgres_v5.sql");
original_sql_migration!(apply_v6, "postgres_v6.sql");
original_sql_migration!(apply_v7, "postgres_v7.sql");
original_sql_migration!(apply_v8, "postgres_v8.sql");
original_sql_migration!(apply_v9, "postgres_v9.sql");
original_sql_migration!(apply_v10, "postgres_v10.sql");
original_sql_migration!(apply_v11, "postgres_v11.sql");
// V12 drops columns and may only be invoked by the guarded driver below.
fn apply_v12(transaction: &mut postgres::Transaction<'_>) -> Result<(), postgres::Error> {
    transaction.batch_execute(include_str!("postgres_v12.sql"))
}
original_sql_migration!(apply_v13, "postgres_v13.sql");
original_sql_migration!(apply_v14, "postgres_v14.sql");
original_sql_migration!(apply_v15, "postgres_v15.sql");

#[derive(Debug)]
pub enum MigrationDriverError {
    Database(postgres::Error),
    V3(V3MigrationError),
    NewerVersion(i32),
    DestructiveResetRequiresAuthorization,
    MigrationAlreadyInProgress,
    V12MatchDataWouldBeLost,
    V12UnsafeRelation,
    V12UnsafeCollation,
}
impl std::fmt::Display for MigrationDriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(f, "{error}"),
            Self::V3(error) => write!(f, "{error}"),
            Self::NewerVersion(version) => write!(f, "Database schema version mismatch. Database schema version is {version}, app schema version is 15."),
            Self::DestructiveResetRequiresAuthorization => write!(f, "Version-zero database has existing relations; the original destructive public-schema reset requires explicit authorization."),
            Self::MigrationAlreadyInProgress => write!(f, "A cooperating database migration is already in progress."),
            Self::V12MatchDataWouldBeLost => write!(f, "The v12 migration would discard match data that is missing or differs in demos."),
            Self::V12UnsafeRelation => write!(f, "The v12 migration requires non-inherited ordinary tables with row-level security disabled."),
            Self::V12UnsafeCollation => write!(f, "The v12 migration refuses nondeterministic text collations."),
        }
    }
}
impl MigrationDriverError {
    /// Original CS Demo Manager domain error code, when one exists.
    pub fn original_error_code(&self) -> Option<i32> {
        match self { Self::NewerVersion(_) => Some(701), _ => None }
    }
}
impl std::error::Error for MigrationDriverError {}
impl From<postgres::Error> for MigrationDriverError { fn from(error:postgres::Error)->Self {Self::Database(error)} }
impl From<V3MigrationError> for MigrationDriverError { fn from(error:V3MigrationError)->Self {Self::V3(error)} }

/// Run the original migration selection and marker transaction up to version 15.
/// Unlike the original automatic reset, a version-zero database with existing
/// accessible relations is rejected before any write. This callable driver is
/// not wired to the application connection path while safety is unverified.
pub fn migrate_to_current(client: &mut postgres::Client) -> Result<(), MigrationDriverError> {
    // Fresh READ COMMITTED snapshots after relation locks avoid comparing stale data.
    let mut transaction=client.build_transaction().isolation_level(postgres::IsolationLevel::ReadCommitted).start()?;
    // Transaction-scoped, nonblocking lock serializes cooperating Rust drivers.
    // Other clients must use the same protocol; the original Node driver does not.
    let locked:bool=transaction.query_one("SELECT pg_try_advisory_xact_lock($1,$2)", &[&0x4353444d_i32,&0x4d494752_i32])?.try_get(0)?;
    if !locked {transaction.rollback()?;return Err(MigrationDriverError::MigrationAlreadyInProgress);}
    transaction.batch_execute("SAVEPOINT csdm_version_probe")?;
    let current:i32 = match transaction.query_opt("SELECT schema_version FROM migrations ORDER BY schema_version DESC LIMIT 1", &[]) {
        Ok(Some(row)) => {let version=row.try_get::<_,Option<i32>>(0)?.unwrap_or(0);transaction.batch_execute("RELEASE SAVEPOINT csdm_version_probe")?;version},
        Ok(None) => {transaction.batch_execute("RELEASE SAVEPOINT csdm_version_probe")?;0},
        Err(error) if error.code() == Some(&postgres::error::SqlState::UNDEFINED_TABLE) => {transaction.batch_execute("ROLLBACK TO SAVEPOINT csdm_version_probe; RELEASE SAVEPOINT csdm_version_probe")?;0},
        Err(error) => return Err(error.into()),
    };
    if current > 15 {transaction.rollback()?;return Err(MigrationDriverError::NewerVersion(current));}
    if current == 0 && !inspect_original_tables(&mut transaction)?.is_empty() {
        transaction.rollback()?;
        return Err(MigrationDriverError::DestructiveResetRequiresAuthorization);
    }
    ensure_migrations_table(&mut transaction)?;
    if current < 1 {apply_v1_modules(&mut transaction)?;}
    if current < 12 {
        // Lock in fixed order before any intermediate migration can mutate data.
        // The later v12 ALTER needs ACCESS EXCLUSIVE on matches; acquire it now
        // to avoid lock upgrade, and prevent concurrent demo writers until commit.
        transaction.batch_execute("SET LOCAL lock_timeout = '10s'; LOCK TABLE \"matches\" IN ACCESS EXCLUSIVE MODE; LOCK TABLE \"demos\" IN SHARE ROW EXCLUSIVE MODE")?;
        // The locks above and these lookups resolve the same unqualified names
        // used by v12. Reject views/foreign relations and any RLS-enabled table
        // rather than changing security settings or trusting a filtered scan.
        for name in ["matches", "demos"] {
            let relation=transaction.query_opt("SELECT c.relkind::text,c.relrowsecurity,EXISTS (SELECT 1 FROM pg_catalog.pg_inherits i WHERE i.inhparent=c.oid OR i.inhrelid=c.oid) FROM pg_catalog.pg_class c WHERE c.oid=to_regclass($1)", &[&name])?;
            let row=relation.ok_or(MigrationDriverError::V12UnsafeRelation)?;
            let kind:String=row.try_get(0)?;let rls:bool=row.try_get(1)?;let inherited:bool=row.try_get(2)?;
            if kind != "r" || rls || inherited {return Err(MigrationDriverError::V12UnsafeRelation);}
        }
        // A nondeterministic collation may equate byte-distinct values or checksums.
        // Fail closed rather than silently discarding match-only data.
        let unsafe_collation:bool=transaction.query_one("SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_collation c ON c.oid=a.attcollation WHERE a.attrelid IN (to_regclass($1),to_regclass($2)) AND a.attnum>0 AND NOT a.attisdropped AND NOT c.collisdeterministic)", &[&"matches",&"demos"])?.try_get(0)?;
        if unsafe_collation {transaction.rollback()?;return Err(MigrationDriverError::V12UnsafeCollation);}
        let unsafe_data:bool=transaction.query_one("SELECT EXISTS (SELECT 1 FROM \"matches\" m LEFT JOIN \"demos\" d ON d.\"checksum\"=m.\"checksum\" WHERE d.\"checksum\" IS NULL OR m.\"name\" IS DISTINCT FROM d.\"name\" OR m.\"game\" IS DISTINCT FROM d.\"game\" OR m.\"source\" IS DISTINCT FROM d.\"source\" OR m.\"type\" IS DISTINCT FROM d.\"type\" OR m.\"date\" IS DISTINCT FROM d.\"date\" OR m.\"map_name\" IS DISTINCT FROM d.\"map_name\" OR m.\"tick_count\" IS DISTINCT FROM d.\"tick_count\" OR m.\"tickrate\" IS DISTINCT FROM d.\"tickrate\" OR m.\"framerate\" IS DISTINCT FROM d.\"framerate\" OR m.\"duration\" IS DISTINCT FROM d.\"duration\" OR m.\"server_name\" IS DISTINCT FROM d.\"server_name\" OR m.\"client_name\" IS DISTINCT FROM d.\"client_name\" OR m.\"network_protocol\" IS DISTINCT FROM d.\"network_protocol\" OR m.\"build_number\" IS DISTINCT FROM d.\"build_number\" OR m.\"share_code\" IS DISTINCT FROM d.\"share_code\")", &[])?.try_get(0)?;
        if unsafe_data {transaction.rollback()?;return Err(MigrationDriverError::V12MatchDataWouldBeLost);}
    }
    if current < 2 {apply_v2(&mut transaction)?;}
    if current < 3 {apply_v3(&mut transaction)?;}
    if current < 4 {apply_v4(&mut transaction)?;}
    if current < 5 {apply_v5(&mut transaction)?;}
    if current < 6 {apply_v6(&mut transaction)?;}
    if current < 7 {apply_v7(&mut transaction)?;}
    if current < 8 {apply_v8(&mut transaction)?;}
    if current < 9 {apply_v9(&mut transaction)?;}
    if current < 10 {apply_v10(&mut transaction)?;}
    if current < 11 {apply_v11(&mut transaction)?;}
    if current < 12 {apply_v12(&mut transaction)?;}
    if current < 13 {apply_v13(&mut transaction)?;}
    if current < 14 {apply_v14(&mut transaction)?;}
    if current < 15 {apply_v15(&mut transaction)?;}
    if current < 15 {transaction.execute("INSERT INTO migrations (schema_version,run_at) VALUES ($1,now())", &[&15_i32])?;}
    transaction.commit()?;
    Ok(())
}
