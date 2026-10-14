fn main() -> anyhow::Result<()> {
    sql_job_monitor::app::Application::new()?.run()
}
