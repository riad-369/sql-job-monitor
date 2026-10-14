use slint::ComponentHandle;
use sql_job_monitor::{platform::TRAFFIC_LIGHT_INSET, MainWindow};

#[test]
fn login_form_asks_for_a_sql_login_beside_the_traffic_lights() {
    let ui = MainWindow::new().expect("create the window");
    ui.set_unified_title_bar(true);
    ui.set_title_inset(TRAFFIC_LIGHT_INSET);
    ui.set_form_server("sql.example.com".into());
    ui.set_form_username("reader".into());
    ui.set_error_message("Login failed. Verify the SQL Server login and password.".into());

    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(300), move || {
        let ui = weak.upgrade().expect("window still open");
        assert!(!ui.get_connected());
        assert_eq!(ui.get_title_inset(), TRAFFIC_LIGHT_INSET);
        if let Ok(path) = std::env::var("LOGIN_SNAPSHOT") {
            let shot = ui.window().take_snapshot().expect("snapshot");
            let image = image_bytes(shot.width(), shot.height(), shot.as_bytes());
            std::fs::write(path, image).expect("write snapshot");
        }
        slint::quit_event_loop().expect("quit");
    });

    ui.run().expect("run the window");
}

/// Uncompressed PPM keeps this test free of an image-encoding dependency.
fn image_bytes(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in rgba.chunks(4) {
        out.extend_from_slice(&pixel[..3]);
    }
    out
}
