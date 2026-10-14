# SQL Job Monitor for macOS

A native macOS desktop app for watching and controlling SQL Server Agent jobs. Connect once, then monitor activity, inspect history, and start or stop jobs without a Windows VM or SQL Server Management Studio.

This is a macOS rebuild of [keith-k2/sql-job-monitor](https://github.com/keith-k2/sql-job-monitor) (Windows), based on commit `03ad29e09ac67661ca1fc20c1d3c78195d496dd1`. The SQL queries, job commands, data model, timeline, filtering, and almost all of the Slint UI come from that project unchanged. The Windows-only parts (Win32 window calls, integrated Windows authentication, `%APPDATA%`, and the MSI installer) are replaced with macOS equivalents.

macOS 11 Big Sur or later, on Apple silicon or Intel.

## What it does

- Connect with a SQL Server login, including named instances (`HOST\INSTANCE`) and the `HOST,PORT` form.
- See every job's status, enabled state, current step, last run, duration, next run, owner, and category.
- Filter by status, search, sort, and resize columns. Running jobs stay pinned at the top.
- Open recent history and the steps inside each run.
- Switch to a zoomable timeline that shows how long each job and step took.
- Start a job, start it at a chosen step, stop it, or enable and disable it.
- Refresh manually or on a timer. The last successful connection signs in again the next time the app starts.
- Remember the window layout, appearance, and history pane. The pin button floats the window above other windows.

SQL Server Agent is not available in SQL Server Express. Azure Elastic Jobs are outside this app's scope.

### What's different from the Windows app

| | Windows app | This macOS app |
|---|---|---|
| Sign-in | Windows account or SQL login | SQL login only |
| Saved passwords | Windows Credential Manager | Login Keychain |
| Settings | `%APPDATA%\SQL Job Monitor` | `~/Library/Application Support/SQL Job Monitor` |
| Window | Custom frameless title bar | Native traffic lights in a unified title bar; double-click zooms |
| Shortcuts | F5, Ctrl+F, Ctrl+scroll | ⌘R or F5, ⌘F, ⌘+scroll; ⌘Q quits |
| Look | Navy dark theme | macOS system grays, accent blue, and Cupertino-style controls |
| TLS | Secure Transport (`native-tls`) | rustls, TLS 1.2 and 1.3. macOS Secure Transport closes SQL Server's handshake |

## Build and install

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

## Connect

The login needs permission to read SQL Server Agent metadata in `msdb`.

- `SQLAgentReaderRole` can monitor the jobs that account is allowed to see.
- `SQLAgentOperatorRole` can also start and stop eligible local jobs.
- `sysadmin` is not required.

SQL Server must accept TCP/IP connections and SQL Server authentication (mixed mode). A named instance also needs SQL Server Browser and UDP 1434, unless you enter its fixed port.

The first connection to a server on your local network may show a macOS "find devices on local networks" prompt. Allow it, or later enable SQL Job Monitor under System Settings > Privacy & Security > Local Network.

## TLS

Connections require encryption (`Encrypt=true` in Tiberius: the whole session is encrypted, not only the login). The handshake is the TDS prelogin wrapped in TLS, which Apple's Secure Transport implementation rejects by closing the socket. The resulting error is `connection closed via error`. That happens before any certificate is inspected, so **Trust server certificate** cannot fix it. Tiberius documents this and recommends rustls on Apple platforms ([tiberius README](https://github.com/prisma/tiberius), [tiberius#65](https://github.com/prisma/tiberius/issues/65)).

This app therefore uses rustls by default:

- The negotiated versions are TLS 1.2 and TLS 1.3.
- Certificate validation uses the certificates in the macOS trust store, through `rustls-native-certs`. An internal CA added in Keychain Access is trusted. A public CA already in the system roots is trusted too.
- **Trust server certificate** still skips certificate and hostname checks for that one connection. Use it only when you already trust the server, for example a development instance with SQL Server's self-signed certificate.

Every TLS error ends with `TLS: …` naming the backend compiled into the running app, for example `TLS: rustls (TLS 1.2 and 1.3).` If an error has no such suffix, or still reads `connection closed via error`, an old copy of the app is running: that text comes only from Secure Transport, which no current build uses. Quit it and reinstall as shown above.

If the server only speaks TLS 1.0 or 1.1, rustls refuses the handshake (`peer is incompatible: ServerDoesNotSupportTls12Or13`) and the app says the server only offers TLS 1.0 or 1.1. SQL Server 2016 and later, and older instances with the TLS 1.2 update, are fine. Enabling TLS 1.2 on the server is the right fix; see [Does the server have TLS 1.2?](#does-the-server-have-tls-12).

### Legacy build for SQL Server 2012 without the TLS 1.2 update

An unpatched SQL Server 2012 offers only TLS 1.0, usually with a 1024-bit SHA-1 self-signed certificate. For that case there is a separate, opt-in build:

```bash
bash scripts/build-macos-app.sh --legacy-tls
```

What it changes:

- **Backend.** Tiberius's `vendored-openssl` feature uses `opentls`, which always calls OpenSSL, on macOS too. It does not go through `native-tls` or Secure Transport. OpenSSL 3 is compiled from source (`openssl-src`) and linked statically, so the build needs Perl and `make` from the Xcode Command Line Tools and takes a few minutes longer.
- **Security level 0.** At OpenSSL 3's default level 1, TLS 1.0 and 1.1 are refused, along with MD5/SHA-1 handshake signatures and RSA keys under 2048 bits, which is everything such a server offers. Tiberius gives no way to configure its connector, so `vendor/opentls` (opentls 0.2.1, applied through `[patch.crates-io]`) sets the cipher string to `DEFAULT:@SECLEVEL=0`. It also sets `SSL_OP_LEGACY_SERVER_CONNECT`, which lets OpenSSL 3 connect to a Windows server without RFC 5746 secure renegotiation; renegotiation itself stays off. Those settings live in `vendor/opentls/src/sync_io/connector/builder.rs`.
- **Still preferred: newer versions.** TLS 1.2 and 1.3 remain enabled and are chosen whenever the server offers them.
- **Certificates.** OpenSSL does not read the Keychain; it uses `/etc/ssl/cert.pem`. A server's self-signed certificate needs **Trust server certificate**, or the CA in a PEM file passed as `SSL_CERT_FILE`.

The risk: TLS 1.0 with CBC ciphers and SHA-1 has known attacks (BEAST, POODLE-style downgrades, SHA-1 collisions), and level 0 accepts weak keys. Use this build only on a network you trust, and only until the server is patched. Each OpenSSL security fix needs a rebuild.

`tests/legacy_tls.rs` verifies this build against a local `openssl s_server -tls1` with a 1024-bit SHA-1 certificate. The patched connector completes a TLS 1.0 handshake, and stock OpenSSL 3 is refused by the same server:

```bash
cargo test --no-default-features --features legacy-tls --test legacy_tls
```

On macOS this needs Homebrew's `openssl@3` first on `PATH`; with the system LibreSSL `openssl`, the handshake tests skip themselves. To confirm which TLS stack a build uses:

```bash
cargo tree --target aarch64-apple-darwin --no-default-features --features legacy-tls -i opentls
cargo tree --target aarch64-apple-darwin --no-default-features --features legacy-tls -i native-tls   # "did not match any packages"
strings -a "/Applications/SQL Job Monitor.app/Contents/MacOS/sql-job-monitor" | grep -m1 '^OpenSSL 3\.'
```

`security-framework` still appears in both trees through `keyring`, for Keychain passwords. In the default build, `rustls-native-certs` uses it to read trust roots. Neither use carries TLS traffic. Run `cargo tree` with `--no-default-features`: the default `rustls-tls` feature cannot be combined with `legacy-tls`, and the build stops with an error saying so.

Install the legacy build the same way, replacing the normal one; keep only one `SQL Job Monitor.app` in `/Applications`. To go back, rebuild without `--legacy-tls`.

If the legacy build also fails with a closed connection, the server has probably disabled TLS 1.0 in Schannel without enabling TLS 1.2, or the host and port do not reach SQL Server itself.

### Does the server have TLS 1.2?

Run this on the server, for example in SSMS:

```sql
SELECT SERVERPROPERTY('ProductVersion')     AS version,
       SERVERPROPERTY('ProductLevel')       AS service_pack,
       SERVERPROPERTY('ProductUpdateLevel') AS cumulative_update,
       @@VERSION                            AS full_version;
```

SQL Server 2012 is `11.0.x`. TLS 1.2 support ([KB3135244](https://support.microsoft.com/help/3135244)) arrived in these builds:

| Branch | First build with TLS 1.2 |
|---|---|
| SP4 (any build) | 11.0.7001.0 |
| SP3 CU | 11.0.6518.0 |
| SP3 GDR | 11.0.6216.27 |
| SP2 CU | 11.0.5644.2 |
| SP2 GDR | 11.0.5352.0 |
| SP1 and RTM | none; install SP4 |

The simplest fix is to install SQL Server 2012 SP4, which includes it, plus the latest SP4 GDR. SQL Server 2012 is out of support, so SP4 is the end of the line.

A new enough build is not sufficient on its own. Windows must also allow TLS 1.2 for Schannel servers:

- Windows Server 2012 and later: enabled by default.
- Windows Server 2008 R2: disabled by default for servers. Under `HKLM\SYSTEM\CurrentControlSet\Control\SecurityProviders\SCHANNEL\Protocols\TLS 1.2\Server`, set `Enabled`=1 and `DisabledByDefault`=0 (DWORD), then restart the SQL Server service.
- Windows Server 2008: needs [KB4019276](https://support.microsoft.com/help/4019276) first, then the same keys.

After the change, the default rustls build connects. To confirm the session is encrypted, run `SELECT encrypt_option FROM sys.dm_exec_connections WHERE session_id = @@SPID;` (expect `TRUE`). SQL Server 2012 does not report the negotiated TLS version. The test that matters is that the default build now connects.

## Develop

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy --all-targets --no-default-features --features legacy-tls -- -D warnings
cargo test --no-default-features --features legacy-tls
```

`tests/toolbar_layout.rs` and `tests/login_layout.rs` open a real window. Set `TOOLBAR_SNAPSHOT=/tmp/toolbar.png` or `LOGIN_SNAPSHOT=/tmp/login.ppm` to save what they rendered.

Set `SQL_JOB_MONITOR_DATA_DIR` to keep settings and saved connections in another folder, for example a scratch profile while testing.

Live SQL tests stay ignored unless a server is provided:

```bash
export SQL_JOB_MONITOR_TEST_SERVER='server.example.com\INSTANCE'
export SQL_JOB_MONITOR_TEST_USER='reader'
export SQL_JOB_MONITOR_TEST_PASSWORD='…'
cargo test --test sql_integration -- --ignored
```

Set `SQL_JOB_MONITOR_TEST_PORT` for a fixed port, or `SQL_JOB_MONITOR_TEST_TRUST_CERT=1` only for a development server.

`.github/workflows/macos.yml` runs the same checks on a macOS runner and uploads a universal `.dmg`.

### Layout

- `src/sql` — connection and every SQL Server Agent query and command.
- `src/domain` — job, history, and timeline models, filtering, and formatting.
- `src/app.rs` — wires the UI to the SQL layer, polling, and saved state.
- `src/platform.rs` — the macOS-specific pieces: unified title bar, window placement, ⌘ shortcuts, and opening links.
- `src/paths.rs`, `src/settings.rs`, `src/saved.rs` — Application Support files and Keychain passwords.
- `ui/` — the Slint interface and theme.
- `packaging/macos/Info.plist`, `scripts/build-macos-app.sh` — the app bundle.

## Security

- A connection is saved only after a successful login when **Remember this connection** is checked.
- Server settings are stored in `~/Library/Application Support/SQL Job Monitor/connections.json`. SQL passwords go to your login Keychain under the service name "SQL Job Monitor", never into that file. macOS may ask to allow access the first time a saved password is read.
- Passwords and connection strings are not written to logs.
- Connections use TLS 1.2 or 1.3 through rustls, checked against the macOS trust store. Trusting the server certificate is an explicit, per-connection choice and does not weaken the protocol version. The optional `--legacy-tls` build is the only one that can offer TLS 1.0 or 1.1. It uses a statically linked OpenSSL 3 at security level 0; see [Legacy build](#legacy-build-for-sql-server-2012-without-the-tls-12-update).
- Queries time out, and a newer refresh replaces an older result that arrives late.

## License

[MIT](LICENSE). The icons in `assets/lucide` are from [Lucide](https://lucide.dev/) and use the ISC license included in that folder.
