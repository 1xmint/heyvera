use cortex_api::db::Database;
use heyvera_db::{export_socials, SocialDatabase, SOCIAL_TABLES};

#[test]
fn current_mixed_v68_exports_into_the_independent_socials_baseline() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("mixed-v68.db");
    let destination = temp.path().join("socials-v1.db");

    let mixed = Database::open(&source);
    assert_eq!(mixed.schema_version(), 68);
    drop(mixed);

    let report = export_socials(&source, &destination).unwrap();
    assert_eq!(report.source.legacy_schema_version, Some(68));
    assert_eq!(report.source.tables.len(), SOCIAL_TABLES.len());
    assert_eq!(report.copied_rows.len(), SOCIAL_TABLES.len());
    assert!(report.copied_rows.iter().all(|(table, rows)| {
        let expected = i64::from(
            table == "social_conversation_activity_clock"
                || table == "social_message_request_clock",
        );
        *rows == expected
    }));

    SocialDatabase::open(&destination)
        .unwrap()
        .verify()
        .unwrap();
}
