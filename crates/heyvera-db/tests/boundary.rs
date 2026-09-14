use heyvera_db::{
    export_socials, inspect_source, Error, PrimaryKeyRange, SocialDatabase, BASELINE_ID,
    SOCIAL_TABLES,
};
use rusqlite::Connection;

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        )
        .unwrap()
}

fn count_rows(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn fresh_schema_is_checksummed_complete_and_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("socials.db");
    let database = SocialDatabase::open(&path).unwrap();
    database.verify().unwrap();
    assert_eq!(database.row_count("accounts").unwrap(), 0);
    drop(database);

    let connection = Connection::open(&path).unwrap();
    let checksum: String = connection
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE id = ?1",
            [BASELINE_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(checksum, SocialDatabase::baseline_checksum());
    drop(connection);
    SocialDatabase::open(&path).unwrap().verify().unwrap();
}

#[test]
fn altered_migration_checksum_stops_startup() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("socials.db");
    drop(SocialDatabase::open(&path).unwrap());
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE schema_migrations SET checksum = 'tampered' WHERE id = ?1",
            [BASELINE_ID],
        )
        .unwrap();
    assert!(matches!(
        SocialDatabase::open(&path),
        Err(Error::ChecksumMismatch { .. })
    ));
}

#[test]
fn schema_allowlist_rejects_a_cortex_table() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("socials.db");
    drop(SocialDatabase::open(&path).unwrap());
    Connection::open(&path)
        .unwrap()
        .execute("CREATE TABLE runs(id TEXT PRIMARY KEY)", [])
        .unwrap();
    assert!(matches!(
        SocialDatabase::open(&path),
        Err(Error::UnexpectedTables(tables)) if tables == ["runs"]
    ));
}

#[test]
fn schema_allowlist_rejects_every_cortex_domain_table() {
    const CORTEX_TABLES: [&str; 61] = [
        "artifacts",
        "context_flow_artifacts",
        "conversations",
        "cortex_approval_requests",
        "cortex_authority_memberships",
        "cortex_authority_resources",
        "cortex_authority_scopes",
        "cortex_groups",
        "cortex_task_chats",
        "cortex_tasks",
        "cost_sessions",
        "cost_warnings",
        "credential_assignments",
        "credit_balances",
        "credit_transactions",
        "decisions",
        "deployment_adapters",
        "execution_jobs",
        "github_imports",
        "group_task_state",
        "idempotency_keys",
        "integration_connections",
        "integration_events",
        "integration_mappings",
        "integration_oauth_states",
        "manual_overrides",
        "messages",
        "operations_events",
        "outcomes",
        "price_list_models",
        "price_list_task_classes",
        "price_lists",
        "project_workspaces",
        "provider_capabilities",
        "provider_request_reservations",
        "provider_spend",
        "provider_spend_authorizations",
        "resource_leases",
        "runs",
        "score_evidence",
        "sequence_allocations",
        "step_attempts",
        "step_dependencies",
        "step_quotes",
        "step_verification_state",
        "step_work_contracts",
        "steps",
        "supplier_capacities",
        "user_api_keys",
        "user_budgets",
        "user_containers",
        "user_credentials",
        "user_profiles",
        "verification_checks",
        "verification_jobs",
        "verification_runs",
        "verification_specs",
        "verifier_reports",
        "worker_keys",
        "worker_sessions",
        "workers",
    ];

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("socials.db");
    drop(SocialDatabase::open(&path).unwrap());
    let connection = Connection::open(&path).unwrap();
    for table in CORTEX_TABLES {
        connection
            .execute(&format!("CREATE TABLE \"{table}\"(id TEXT)"), [])
            .unwrap();
    }
    drop(connection);

    assert!(matches!(
        SocialDatabase::open(&path),
        Err(Error::UnexpectedTables(tables)) if tables.len() == CORTEX_TABLES.len()
    ));
}

#[test]
fn synthetic_mixed_v68_exports_only_socials_and_rebuilds_fts() {
    let temp = tempfile::tempdir().unwrap();
    let source_path = temp.path().join("mixed-v68.db");
    let destination_path = temp.path().join("socials-v1.db");
    drop(SocialDatabase::open(&source_path).unwrap());

    let source = Connection::open(&source_path).unwrap();
    source
        .execute_batch(
            "CREATE TABLE schema_version(version INTEGER NOT NULL);
             INSERT INTO schema_version VALUES (68);
             CREATE TABLE runs(id TEXT PRIMARY KEY, goal TEXT NOT NULL);
             INSERT INTO runs VALUES ('cortex-canary', 'must not cross');
             CREATE TABLE usage_events(id TEXT PRIMARY KEY, provider TEXT NOT NULL);
             INSERT INTO usage_events VALUES ('pulse-usage', 'pulse');
             INSERT INTO accounts(clerk_user_id, email, display_name)
                 VALUES ('user-1', 'private@example.invalid', 'Private');
             INSERT INTO subscriptions(
                 clerk_user_id, stripe_customer_id, stripe_subscription_id,
                 plan_type, status
             ) VALUES ('user-1', 'customer-1', 'subscription-1', 'monthly', 'active');
             INSERT INTO billing_history(
                 id, clerk_user_id, stripe_event_id, amount_cents, description, status
             ) VALUES ('bill-1', 'user-1', 'event-1', 1200, 'Existing plan', 'paid');
             INSERT INTO social_profiles(id, clerk_user_id, handle, display_name)
                 VALUES ('profile-1', 'user-1', 'one', 'One'),
                        ('profile-2', 'user-2', 'two', 'Two');
             INSERT INTO social_posts(id, profile_id, body, audience_profile_id)
                 VALUES ('post-1', 'profile-1', 'searchable boundary', 'profile-1');
             INSERT INTO social_conversations(
                 id, creator_profile_id, creation_key, activity_sequence
             ) VALUES ('conversation-1', 'profile-1', 'creation-key', 1);
             INSERT INTO social_conversation_participants(
                 conversation_id, profile_id, joined_message_sequence,
                 last_read_message_sequence
             ) VALUES ('conversation-1', 'profile-1', 0, 0),
                      ('conversation-1', 'profile-2', 0, 0);
             INSERT INTO social_messages(
                 id, conversation_id, sender_profile_id, content, sequence,
                 client_message_id
             ) VALUES (
                 'message-1', 'conversation-1', 'profile-1', 'hello', 1,
                 'client-message-1'
             );
             UPDATE social_conversation_activity_clock SET next_sequence = 2;
             INSERT INTO social_message_requests(
                 id, sender_profile_id, recipient_profile_id, client_request_id,
                 content, content_fingerprint, activity_sequence
             ) VALUES (
                 'request-1', 'profile-1', 'profile-2', 'client-request-1',
                 'request hello',
                 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                 1
             );
             UPDATE social_message_request_clock SET next_sequence = 2;",
        )
        .unwrap();
    drop(source);

    let inspection = inspect_source(&source_path).unwrap();
    assert_eq!(inspection.legacy_schema_version, Some(68));
    assert_eq!(inspection.tables.len(), SOCIAL_TABLES.len());
    assert_eq!(inspection.mixed_table_counts, [("usage_events".into(), 1)]);
    assert!(matches!(
        inspection
            .tables
            .iter()
            .find(|table| table.name == "accounts")
            .unwrap()
            .primary_key_range,
        PrimaryKeyRange::Opaque { populated_rows: 1 }
    ));

    let report = export_socials(&source_path, &destination_path).unwrap();
    assert_eq!(report.copied_rows.len(), SOCIAL_TABLES.len());
    let destination = Connection::open(&destination_path).unwrap();
    assert!(!table_exists(&destination, "runs"));
    assert!(!table_exists(&destination, "schema_version"));
    assert!(!table_exists(&destination, "usage_events"));
    assert_eq!(count_rows(&destination, "subscriptions"), 1);
    assert_eq!(count_rows(&destination, "billing_history"), 1);
    assert_eq!(count_rows(&destination, "social_messages"), 1);
    assert_eq!(count_rows(&destination, "social_message_requests"), 1);
    assert_eq!(count_rows(&destination, "social_posts_fts"), 1);
    assert!(destination
        .execute(
            "INSERT INTO social_messages(
                id, conversation_id, sender_profile_id, content, sequence, client_message_id
             ) VALUES ('message-2', 'conversation-1', 'profile-2', 'duplicate', 1,
                       'client-message-2')",
            [],
        )
        .is_err());
    assert!(destination
        .execute(
            "INSERT INTO social_message_requests(
                id, sender_profile_id, recipient_profile_id, client_request_id,
                content, content_fingerprint, state, activity_sequence
             ) VALUES (
                'request-2', 'profile-2', 'profile-1', 'client-request-2', 'bad state',
                'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                'unknown', 2
             )",
            [],
        )
        .is_err());
    let integrity: String = destination
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let mut foreign_key_check = destination.prepare("PRAGMA foreign_key_check").unwrap();
    assert!(foreign_key_check
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
}

#[test]
fn export_requires_every_owned_source_table_and_a_new_destination() {
    let temp = tempfile::tempdir().unwrap();
    let incomplete = temp.path().join("incomplete.db");
    Connection::open(&incomplete)
        .unwrap()
        .execute("CREATE TABLE accounts(clerk_user_id TEXT PRIMARY KEY)", [])
        .unwrap();
    let destination = temp.path().join("destination.db");
    assert!(matches!(
        export_socials(&incomplete, &destination),
        Err(Error::MissingTables(_))
    ));

    drop(SocialDatabase::open(&destination).unwrap());
    assert!(matches!(
        export_socials(&incomplete, &destination),
        Err(Error::DestinationExists(path)) if path == destination
    ));
}
