# SQL Job Monitor

A native desktop app for watching and controlling SQL Server Agent jobs. Connect once, then monitor activity, inspect history, and start or stop jobs without opening SQL Server Management Studio.

One Cargo project builds for Windows and macOS. The target operating system selects the window, sign-in, and settings path. The TLS backend is a Cargo feature, and it is rustls unless you opt into the legacy build.

**[Download the Windows installer](https://github.com/keith-k2/sql-job-monitor/releases/latest)**

## What it does

- Connect to a server, including named instances (`HOST\INSTANCE`) and the `HOST,PORT` form.
- See every job's status, enabled state, current step, last run, duration, next run, owner, and category.
- Filter by status, search, sort, and resize columns. Running jobs stay pinned at the top.
- Open recent history and the steps inside each run.
- Switch to a zoomable timeline that shows how long each job and step took.
- Start a job, start it at a chosen step, stop it, or enable and disable it.
- Refresh manually or on a timer. The last successful connection signs in again the next time the app starts.
- Remember the window layout, appearance, and history pane. The pin keeps the window above other windows.

SQL Server Agent is not available in SQL Server Express. Azure Elastic Jobs are outside this app's scope.

| | Windows | macOS |
|---|---|---|
| Sign-in | Windows account or SQL login | SQL login only |
| Saved passwords | Windows Credential Manager | Login Keychain |
| Settings | `%APPDATA%\SQL Job Monitor` | `~/Library/Application Support/SQL Job Monitor` |
| Window | Frameless window, Win32 caption buttons | Native traffic lights in a unified title bar; double-click zooms |
| Shortcuts | F5, Ctrl+F, Ctrl+scroll | ⌘R or F5, ⌘F, ⌘+scroll; ⌘Q quits |
| Look | Navy dark theme | macOS system grays and Cupertino controls |
| Install | WiX MSI | `.app` bundle |

## Windows

Windows 10 or later.

### Install

1. Open the [latest release](https://github.com/keith-k2/sql-job-monitor/releases/latest).
2. Download `sql-job-monitor-0.1.0-x86_64.msi`.
3. Run the installer. Windows asks for administrator approval because the app is installed for all users on the computer.
4. Launch **SQL Job Monitor** from the Start menu.

To remove it, use **Apps** in Windows Settings.

### Build

Install the current stable Rust toolchain from [rustup](https://rustup.rs/), then:

```powershell
cargo run --release
```

That is the default `rustls-tls` feature: TLS 1.2 and 1.3. On Windows the same build also enables integrated authentication. The executable is `target\release\sql-job-monitor.exe`, with the application icon embedded.

Checks:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Installer

The MSI is built with [cargo-wix](https://github.com/volks73/cargo-wix) and WiX Toolset 3.14:

```powershell
cargo install cargo-wix
cargo wix --bin-path C:\path\to\wix314
```

`target\wix\sql-job-monitor-0.1.0-x86_64.msi` is the installer. Its upgrade identifiers live in `Cargo.toml` and `wix\main.wxs`.

## macOS

macOS 11 Big Sur or later, on Apple silicon or Intel.

Install the current stable Rust toolchain from [rustup](https://rustup.rs/) and the Xcode Command Line Tools (`xcode-select --install`). Then, from this folder:

```bash
bash scripts/build-macos-app.sh
```

This writes `dist/SQL Job Monitor.app`. Quit the running app, then replace the installed copy and open the new one:

```bash
osascript -e 'quit app "SQL Job Monitor"'
rm -rf "/Applications/SQL Job Monitor.app"
cp -R "dist/SQL Job Monitor.app" "/Applications/SQL Job Monitor.app"
open -a "SQL Job Monitor"
```

Other options:

```bash
bash scripts/build-macos-app.sh --universal   # one binary for Apple silicon and Intel
bash scripts/build-macos-app.sh --dmg         # also write dist/sql-job-monitor-0.1.0-macos.dmg
bash scripts/build-macos-app.sh --legacy-tls  # OpenSSL, TLS 1.0 allowed: unpatched SQL Server 2012 only
```

The bundle is ad-hoc signed, so it runs on the Mac that built it. To give it to other people, set `CODESIGN_IDENTITY` to a Developer ID Application certificate before running the script, then notarize the result with `xcrun notarytool`. An unsigned copy downloaded from elsewhere has to be opened with right-click > Open the first time.

To run without bundling:

```bash
cargo run --release
```

The first connection to a server on your local network may show a macOS "find devices on local networks" prompt. Allow it, or later enable SQL Job Monitor under System Settings > Privacy & Security > Local Network.

## Connect

The login needs permission to read SQL Server Agent metadata in `msdb`.

- `SQLAgentReaderRole` can monitor the jobs that account is allowed to see.
- `SQLAgentOperatorRole` can also start and stop eligible local jobs.
- `sysadmin` is not required.

SQL Server must accept TCP/IP connections. On macOS it must also accept SQL Server authentication (mixed mode). A named instance also needs SQL Server Browser and UDP 1434, unless you enter its fixed port.

On Windows, **Use my current Windows account** signs in with integrated authentication. macOS has no Windows account, so that checkbox is absent and a saved Windows-auth entry is skipped.

## TLS

Connections require encryption (`Encrypt=true` in Tiberius: the whole session is encrypted, not only the login). The default feature is `rustls-tls`:

- The negotiated versions are TLS 1.2 and TLS 1.3.
- Certificate validation uses the operating system trust store, through `rustls-native-certs`.
- **Trust server certificate** still skips certificate and hostname checks for that one connection. Use it only when you already trust the server.

`rustls-tls` and `legacy-tls` cannot be combined. The build stops if both are enabled.

Every TLS error ends with `TLS: …` naming the backend compiled into the running app, for example `TLS: rustls (TLS 1.2 and 1.3).`

If the server only speaks TLS 1.0 or 1.1, rustls refuses the handshake. SQL Server 2016 and later, and older instances with the TLS 1.2 update, are fine. Enabling TLS 1.2 on the server is the right fix.

### Legacy build for SQL Server 2012 without the TLS 1.2 update

An unpatched SQL Server 2012 offers only TLS 1.0. That build is opt-in:

```bash
# macOS
bash scripts/build-macos-app.sh --legacy-tls

# Windows, or a local cargo build on either platform
cargo build --release --no-default-features --features legacy-tls
```

What it changes:

- **Backend.** Tiberius's `vendored-openssl` feature uses `opentls`, which calls OpenSSL rather than Secure Transport or Schannel. OpenSSL 3 is compiled from source (`openssl-src`) and linked statically.
- **Security level 0.** At OpenSSL 3's default level 1, TLS 1.0 and 1.1 are refused, along with SHA-1 handshake signatures and RSA keys under 2048 bits. `vendor/opentls` sets `DEFAULT:@SECLEVEL=0` and `SSL_OP_LEGACY_SERVER_CONNECT`. TLS 1.2 and 1.3 stay enabled and are chosen whenever the server offers them.
- **Certificates.** OpenSSL does not read the Keychain or the Windows certificate store. A self-signed certificate needs **Trust server certificate**, or a CA in a PEM file passed as `SSL_CERT_FILE`.

Use this build only on a network you trust, and only until the server is patched. SQL Server 2012 TLS 1.2 support ([KB3135244](https://support.microsoft.com/help/3135244)) is in SP4 (11.0.7001.0) and selected earlier cumulative updates. SP4 is the simplest fix.

Confirm which stack a build uses:

```bash
cargo tree --no-default-features --features legacy-tls -i opentls
cargo tree -i rustls
```

`security-framework` can still appear on macOS through `keyring` and `rustls-native-certs`. Neither of those carries the SQL session in the default build.

## Develop

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy --all-targets --no-default-features --features legacy-tls -- -D warnings
cargo test --no-default-features --features legacy-tls
```

`tests/toolbar_layout.rs` and `tests/login_layout.rs` open a real window. Set `TOOLBAR_SNAPSHOT=/tmp/toolbar.png` or `LOGIN_SNAPSHOT=/tmp/login.ppm` to save what they rendered.

Set `SQL_JOB_MONITOR_DATA_DIR` to keep settings and saved connections in another folder. When it is unset, Windows still uses `%APPDATA%\SQL Job Monitor`.

Live SQL tests stay ignored unless a server is provided:

```bash
export SQL_JOB_MONITOR_TEST_SERVER='server.example.com\INSTANCE'
export SQL_JOB_MONITOR_TEST_USER='reader'
export SQL_JOB_MONITOR_TEST_PASSWORD='…'
cargo test --test sql_integration -- --ignored
```

On Windows, omit the user and password to sign in with the current Windows account. Set `SQL_JOB_MONITOR_TEST_PORT` for a fixed port, or `SQL_JOB_MONITOR_TEST_TRUST_CERT=1` only for a development server.

`.github/workflows/windows.yml` runs the default build on Windows. `.github/workflows/macos.yml` runs the same checks, the legacy TLS build, and uploads a universal `.dmg`.

### Layout

- `src/sql` — connection and every SQL Server Agent query and command.
- `src/domain` — job, history, and timeline models, filtering, and formatting.
- `src/app.rs` — wires the UI to the SQL layer, polling, and saved state.
- `src/platform.rs` — Win32 caption and work area, or the macOS unified title bar, plus shortcuts and opening links.
- `src/paths.rs`, `src/settings.rs`, `src/saved.rs` — `%APPDATA%` or Application Support, and Credential Manager or Keychain passwords.
- `ui/` — the Slint interface. Colors and the window frame follow the target OS.
- `wix/` — the Windows MSI.
- `packaging/macos/Info.plist`, `scripts/build-macos-app.sh` — the app bundle.

## Security

- A connection is saved only after a successful login when **Remember this connection** is checked.
- Server settings are stored in `%APPDATA%\SQL Job Monitor\connections.json` on Windows and `~/Library/Application Support/SQL Job Monitor/connections.json` on macOS. SQL passwords go to Windows Credential Manager or the login Keychain, not into that file.
- Passwords and connection strings are not written to logs.
- Default connections use TLS 1.2 or 1.3 through rustls. Trusting the server certificate is an explicit, per-connection choice and does not weaken the protocol version. The optional `legacy-tls` feature is the only build that can offer TLS 1.0 or 1.1.
- Queries time out, and a newer refresh replaces an older result that arrives late.

## License

[MIT](LICENSE). The icons in `assets/lucide` are from [Lucide](https://lucide.dev/) and use the ISC license included in that folder.
