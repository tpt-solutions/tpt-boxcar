//! Integration tests for the Postgres and MySQL wire-protocol drivers against
//! real database instances. These are `#[ignore]`d by default since they need
//! a live Postgres/MySQL to talk to; run explicitly with:
//!
//! `cargo test -p tpt-tether-proxy --test wire_driver_integration -- --ignored`
//!
//! Connection info is read from env vars with sensible local-dev defaults:
//! - `PG_TEST_HOST` / `PG_TEST_PORT` / `PG_TEST_DB` / `PG_TEST_USER` / `PG_TEST_PASS`
//! - `MYSQL_TEST_HOST` / `MYSQL_TEST_PORT` / `MYSQL_TEST_DB` / `MYSQL_TEST_USER` / `MYSQL_TEST_PASS`

use serde_json::json;
use tpt_tether_proxy::drivers::{box_driver, DriverKind};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::test]
#[ignore]
async fn postgres_parameterized_execute_and_query_roundtrip() {
    let host = env_or("PG_TEST_HOST", "localhost");
    let port: u16 = env_or("PG_TEST_PORT", "5432").parse().unwrap();
    let db = env_or("PG_TEST_DB", "tether_test");
    let user = env_or("PG_TEST_USER", "postgres");
    let pass = env_or("PG_TEST_PASS", "");

    let mut driver = box_driver(DriverKind::Postgres);
    driver
        .connect(&host, port, &db, &user, &pass)
        .await
        .expect("connect to postgres");

    driver
        .execute("DELETE FROM widgets", &[])
        .await
        .expect("clear table");

    let inserted = driver
        .execute(
            "INSERT INTO widgets (name, qty, price) VALUES ($1, $2, $3)",
            &[json!("bolt"), json!(10), json!(1.5)],
        )
        .await
        .expect("parameterized insert");
    assert_eq!(
        inserted, 1,
        "expected 1 affected row from INSERT, got {inserted}"
    );

    driver
        .execute(
            "INSERT INTO widgets (name, qty, price) VALUES ($1, $2, $3)",
            &[json!("nut"), json!(20), json!(0.5)],
        )
        .await
        .expect("second parameterized insert");

    let updated = driver
        .execute(
            "UPDATE widgets SET qty = $1 WHERE name = $2",
            &[json!(99), json!("bolt")],
        )
        .await
        .expect("parameterized update");
    assert_eq!(
        updated, 1,
        "expected 1 affected row from UPDATE, got {updated}"
    );

    let result = driver
        .query(
            "SELECT name, qty, price FROM widgets WHERE name = $1",
            &[json!("bolt")],
        )
        .await
        .expect("parameterized select");
    assert_eq!(result.columns, vec!["name", "qty", "price"]);
    assert_eq!(result.values.len(), 1);
    let row = result.values[0].as_array().expect("row is array");
    assert_eq!(row[0], json!("bolt"));
    assert_eq!(row[1], json!(99));
    assert_eq!(row[2], json!(1.5));

    let deleted = driver
        .execute("DELETE FROM widgets WHERE name = $1", &[json!("nut")])
        .await
        .expect("parameterized delete");
    assert_eq!(
        deleted, 1,
        "expected 1 affected row from DELETE, got {deleted}"
    );
}

#[tokio::test]
#[ignore]
async fn mysql_parameterized_execute_and_query_roundtrip() {
    let host = env_or("MYSQL_TEST_HOST", "127.0.0.1");
    let port: u16 = env_or("MYSQL_TEST_PORT", "3307").parse().unwrap();
    let db = env_or("MYSQL_TEST_DB", "tether_test");
    let user = env_or("MYSQL_TEST_USER", "tester");
    let pass = env_or("MYSQL_TEST_PASS", "testpass");

    let mut driver = box_driver(DriverKind::Mysql);
    driver
        .connect(&host, port, &db, &user, &pass)
        .await
        .expect("connect to mysql");

    driver
        .execute("DELETE FROM widgets", &[])
        .await
        .expect("clear table");

    let inserted = driver
        .execute(
            "INSERT INTO widgets (name, qty, price) VALUES (?, ?, ?)",
            &[json!("bolt"), json!(10), json!(1.5)],
        )
        .await
        .expect("parameterized insert");
    assert_eq!(
        inserted, 1,
        "expected 1 affected row from INSERT, got {inserted}"
    );

    driver
        .execute(
            "INSERT INTO widgets (name, qty, price) VALUES (?, ?, ?)",
            &[json!("nut"), json!(20), json!(0.5)],
        )
        .await
        .expect("second parameterized insert");

    let updated = driver
        .execute(
            "UPDATE widgets SET qty = ? WHERE name = ?",
            &[json!(99), json!("bolt")],
        )
        .await
        .expect("parameterized update");
    assert_eq!(
        updated, 1,
        "expected 1 affected row from UPDATE, got {updated}"
    );

    let result = driver
        .query(
            "SELECT name, qty, price FROM widgets WHERE name = ?",
            &[json!("bolt")],
        )
        .await
        .expect("parameterized select");
    assert_eq!(result.columns, vec!["name", "qty", "price"]);
    assert_eq!(result.values.len(), 1);
    let row = result.values[0].as_array().expect("row is array");
    assert_eq!(row[0], json!("bolt"));
    assert_eq!(row[1], json!(99));
    assert_eq!(row[2], json!(1.5));

    let deleted = driver
        .execute("DELETE FROM widgets WHERE name = ?", &[json!("nut")])
        .await
        .expect("parameterized delete");
    assert_eq!(
        deleted, 1,
        "expected 1 affected row from DELETE, got {deleted}"
    );
}
