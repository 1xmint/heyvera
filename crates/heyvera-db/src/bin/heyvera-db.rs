use std::path::PathBuf;

use heyvera_db::{export_socials, inspect_source, PrimaryKeyRange};

fn main() {
    if let Err(error) = run() {
        eprintln!("heyvera-db: {error}");
        std::process::exit(1);
    }
}

fn run() -> heyvera_db::Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    match arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .as_deref()
    {
        Some("inspect") => {
            let source = required_path(arguments.next(), "inspect <mixed-source.db>");
            reject_extra(arguments.next());
            print_inspection(&inspect_source(source)?);
        }
        Some("export") => {
            let source = required_path(arguments.next(), "export <mixed-source.db> <socials.db>");
            let destination =
                required_path(arguments.next(), "export <mixed-source.db> <socials.db>");
            reject_extra(arguments.next());
            let report = export_socials(source, destination)?;
            print_inspection(&report.source);
            println!("exported Socials rows:");
            for (table, rows) in report.copied_rows {
                println!("  {table}: {rows}");
            }
        }
        _ => usage(),
    }
    Ok(())
}

fn required_path(value: Option<std::ffi::OsString>, usage_text: &str) -> PathBuf {
    value.map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: heyvera-db {usage_text}");
        std::process::exit(2);
    })
}

fn reject_extra(value: Option<std::ffi::OsString>) {
    if value.is_some() {
        usage();
    }
}

fn usage() -> ! {
    eprintln!(
        "usage:\n  heyvera-db inspect <mixed-source.db>\n  \
         heyvera-db export <mixed-source.db> <new-socials.db>"
    );
    std::process::exit(2)
}

fn print_inspection(report: &heyvera_db::SourceInspection) {
    println!("SQLite version: {}", report.sqlite_version);
    println!("journal mode: {}", report.journal_mode);
    println!(
        "WAL: {}{}",
        if report.wal_present {
            "present"
        } else {
            "absent"
        },
        report
            .wal_bytes
            .map_or_else(String::new, |bytes| format!(" ({bytes} bytes)"))
    );
    println!(
        "legacy schema version: {}",
        report
            .legacy_schema_version
            .map_or_else(|| "none".to_string(), |version| version.to_string())
    );
    println!("foreign-key violations: {}", report.foreign_key_violations);
    println!("Socials tables:");
    for table in &report.tables {
        let primary_key = if table.primary_key_columns.is_empty() {
            "no declared primary key".to_string()
        } else {
            format!("primary key {}", table.primary_key_columns.join("+"))
        };
        let range = match &table.primary_key_range {
            PrimaryKeyRange::None => "no range".to_string(),
            PrimaryKeyRange::Integer { minimum, maximum } => {
                format!("integer range {minimum}..{maximum}")
            }
            PrimaryKeyRange::Opaque { populated_rows } => {
                format!("opaque range withheld ({populated_rows} populated rows)")
            }
        };
        println!(
            "  {}: {} rows; {primary_key}; {range}",
            table.name, table.row_count
        );
    }
    for (table, rows) in &report.mixed_table_counts {
        println!("mixed table {table}: {rows} rows (contents not inspected)");
    }
}
