use sql_job_monitor::sql::{self, Authentication, ConnectionOptions};
use zeroize::Zeroizing;

/// Run explicitly with:
/// `cargo test --test sql_integration -- --ignored`
/// Windows can use the current Windows account when `SQL_JOB_MONITOR_TEST_USER` is unset.
/// macOS requires that user and `SQL_JOB_MONITOR_TEST_PASSWORD`.
#[tokio::test]
#[ignore = "requires SQL_JOB_MONITOR_TEST_SERVER and a live SQL Server Agent"]
async fn connects_and_reads_job_snapshot() {
    let server =
        std::env::var("SQL_JOB_MONITOR_TEST_SERVER").expect("SQL_JOB_MONITOR_TEST_SERVER is set");
    let username = std::env::var("SQL_JOB_MONITOR_TEST_USER").ok();
    let password = std::env::var("SQL_JOB_MONITOR_TEST_PASSWORD").unwrap_or_default();
    let authentication = match username {
        Some(username) => Authentication::SqlServer {
            username,
            password: Zeroizing::new(password),
        },
        None if cfg!(target_os = "windows") => Authentication::Integrated,
        None => panic!("set SQL_JOB_MONITOR_TEST_USER to a SQL login"),
    };
    let options = ConnectionOptions {
        server,
        port: std::env::var("SQL_JOB_MONITOR_TEST_PORT")
            .ok()
            .and_then(|value| value.parse().ok()),
        database: "msdb".to_owned(),
        authentication,
        trust_server_certificate: std::env::var_os("SQL_JOB_MONITOR_TEST_TRUST_CERT").is_some(),
    };

    let client = sql::connect(&options).await.expect("connection succeeds");
    let jobs = sql::fetch_jobs(&client)
        .await
        .expect("job snapshot succeeds");

    if let Some(job) = jobs.first() {
        sql::fetch_history(&client, &job.id)
            .await
            .expect("history succeeds");
    }
}
