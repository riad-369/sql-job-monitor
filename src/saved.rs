use serde::{Deserialize, Serialize};
use std::{fs, path::Path, path::PathBuf};
use uuid::Uuid;
use zeroize::Zeroizing;

const KEYRING_SERVICE: &str = "SQL Job Monitor";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: String,
    pub server: String,
    pub port: Option<u16>,
    pub username: String,
    pub trust_server_certificate: bool,
}

impl SavedConnection {
    pub fn label(&self) -> String {
        match self.port {
            Some(port) => format!("{}:{port}", self.server),
            None => self.server.clone(),
        }
    }

    pub fn detail(&self) -> String {
        let auth = format!("SQL login {}", self.username);
        if self.trust_server_certificate {
            format!("{auth} · certificate override")
        } else {
            auth
        }
    }
}

pub fn connections_path() -> PathBuf {
    crate::paths::app_data_dir().join("connections.json")
}

pub fn load() -> Vec<SavedConnection> {
    load_from(&connections_path())
}

pub fn load_from(path: &Path) -> Vec<SavedConnection> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut connections: Vec<SavedConnection> = serde_json::from_str(&text).unwrap_or_default();
    // Integrated-auth entries copied from the Windows build have no SQL login to sign in with.
    connections.retain(|connection| !connection.username.trim().is_empty());
    connections
}

pub fn store(connections: &[SavedConnection]) -> Result<(), String> {
    store_at(&connections_path(), connections)
}

pub fn store_at(path: &Path, connections: &[SavedConnection]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(connections).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

/// Inserts or updates a connection matched by server, port, and login.
/// The matching entry stays at the top so the most recent login is first.
pub fn upsert(connections: &mut Vec<SavedConnection>, mut connection: SavedConnection) -> String {
    if connection.id.is_empty() {
        connection.id = connections
            .iter()
            .find(|saved| same_target(saved, &connection))
            .map(|saved| saved.id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
    }
    connections.retain(|saved| saved.id != connection.id && !same_target(saved, &connection));
    let id = connection.id.clone();
    connections.insert(0, connection);
    id
}

pub fn store_password(id: &str, password: &str) -> Result<(), String> {
    keyring::Entry::new(KEYRING_SERVICE, id)
        .map_err(|error| error.to_string())?
        .set_password(password)
        .map_err(|error| error.to_string())
}

pub fn load_password(id: &str) -> Result<Zeroizing<String>, String> {
    keyring::Entry::new(KEYRING_SERVICE, id)
        .map_err(|error| error.to_string())?
        .get_password()
        .map(Zeroizing::new)
        .map_err(|_| "The saved password could not be read. Enter it again and connect.".to_owned())
}

pub fn delete_password(id: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, id) {
        let _ = entry.delete_credential();
    }
}

fn same_target(left: &SavedConnection, right: &SavedConnection) -> bool {
    left.server.eq_ignore_ascii_case(&right.server)
        && left.port == right.port
        && left.username.eq_ignore_ascii_case(&right.username)
        && left.trust_server_certificate == right.trust_server_certificate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(server: &str, username: &str) -> SavedConnection {
        SavedConnection {
            id: String::new(),
            server: server.to_owned(),
            port: None,
            username: username.to_owned(),
            trust_server_certificate: false,
        }
    }

    #[test]
    fn upsert_deduplicates_and_moves_recent_login_first() {
        let mut connections = Vec::new();
        upsert(&mut connections, sample("alpha", "reader"));
        upsert(&mut connections, sample("beta", "sa"));
        upsert(&mut connections, sample("ALPHA", "Reader"));

        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0].server, "ALPHA");
        assert_eq!(connections[0].detail(), "SQL login Reader");
        assert_eq!(connections[1].label(), "beta");
    }

    #[test]
    fn round_trips_connections_without_passwords() {
        let path = std::env::temp_dir().join(format!("sql-job-monitor-{}.json", Uuid::new_v4()));
        let connection = sample("CAL-DWH", "reader");
        store_at(&path, std::slice::from_ref(&connection)).unwrap();
        let loaded = load_from(&path);
        let _ = fs::remove_file(&path);

        let text = serde_json::to_string(&loaded).unwrap();
        assert_eq!(loaded[0].server, connection.server);
        assert!(!text.to_lowercase().contains("password"));
    }

    #[test]
    fn reads_connections_saved_with_the_windows_auth_flag() {
        let text = r#"[{"id":"1","server":"dwh","port":1433,"windows_auth":false,
            "username":"reader","trust_server_certificate":true},
            {"id":"2","server":"dwh","port":null,"windows_auth":true,
            "username":"","trust_server_certificate":false}]"#;
        let path = std::env::temp_dir().join(format!("sql-job-monitor-{}.json", Uuid::new_v4()));
        fs::write(&path, text).unwrap();
        let loaded = load_from(&path);
        let _ = fs::remove_file(&path);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].label(), "dwh:1433");
        assert_eq!(
            loaded[0].detail(),
            "SQL login reader · certificate override"
        );
    }
}
