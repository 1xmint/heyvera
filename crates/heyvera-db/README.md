# heyvera-db

Independent SQLite schema and migration tooling for HeyVera Socials. This
crate is additive until the application-state cutover: the running
`heyvera-server` still uses the legacy mixed database module in PR 3.

The v1 schema owns exactly 43 domain tables. It uses the checksummed
`schema_migrations` ledger, rebuilds `social_posts_fts` from `social_posts`,
and rejects unexpected domain tables.

Inspect a mixed source without opening it for writes:

```text
cargo run -p heyvera-db --bin heyvera-db -- inspect path/to/source.db
```

Export to a path that does not yet exist:

```text
cargo run -p heyvera-db --bin heyvera-db -- export path/to/source.db path/to/socials.db
```

The report emits schema versions, row counts, non-sensitive primary-key range
metadata, WAL mode, and integrity state. It never prints customer rows or
opaque identifiers. The exporter reads only the checked-in Socials allowlist;
it does not copy the mixed `audit_log` or `usage_events` tables.
