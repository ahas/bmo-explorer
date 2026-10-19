//! A virtual X server of a browse run's own (docs/browse-runs.md, 「Browser strategy」), so a Chrome that is
//! not headless never opens a window on a person's desktop session, never reaches that session's D-Bus and
//! never asks its keyring for anything. The server picks a free display number itself and accepts only the
//! random cookie written beside it, and it dies with the process that started it.

use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use rand::Rng;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

// How long a server asked to stop is given before it is killed.
const STOP_WAIT: Duration = Duration::from_secs(3);

/// A display started: its name, the authority file holding its cookie, the runtime directory a program on it
/// is handed, and the server's process.
pub struct Display {
  pub display: String,
  pub xauthority: PathBuf,
  pub runtime_dir: PathBuf,
  server: Child,
}

impl Display {
  /// The server's process number, while it runs.
  pub fn pid(&self) -> Option<u32> {
    self.server.id()
  }

  /// The server stopped: asked first, killed where it has not gone in its while.
  pub async fn stop(mut self) {
    if let Some(pid) = self.server.id() {
      // SAFETY: the process is this display's own server, started by it and not yet waited for.
      unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
      }
    }

    if tokio::time::timeout(STOP_WAIT, self.server.wait()).await.is_err() {
      let _ = self.server.kill().await;
    }
  }
}

// An authority file holding one MIT-MAGIC-COOKIE-1 entry for every address and display, which is what lets the
// cookie be written before the server has said which display number it took.
fn authority_entry(cookie: &[u8]) -> Vec<u8> {
  let field = |bytes: &[u8]| {
    let mut out = (bytes.len() as u16).to_be_bytes().to_vec();

    out.extend_from_slice(bytes);
    out
  };
  let mut entry = 0xffff_u16.to_be_bytes().to_vec();

  entry.extend(field(b""));
  entry.extend(field(b""));
  entry.extend(field(b"MIT-MAGIC-COOKIE-1"));
  entry.extend(field(cookie));
  entry
}

// The program a variable names, or the machine's own.
pub fn program(variable: &str, default: &str) -> PathBuf {
  std::env::var_os(variable)
    .filter(|named| !named.is_empty())
    .map_or_else(|| PathBuf::from(default), PathBuf::from)
}

/// The server started under `dir`, which the caller owns and removes; one that has not named its display by
/// the deadline is stopped and said to be unavailable.
pub async fn start(dir: &Path, deadline: Instant) -> Result<Display, String> {
  let xvfb = program("BMO_XVFB_PATH", "/usr/bin/Xvfb");
  let setpriv = program("BMO_SETPRIV_PATH", "/usr/bin/setpriv");

  if !xvfb.exists() || !setpriv.exists() {
    return Err(format!("A private display is unavailable ({}, {})", xvfb.display(), setpriv.display()));
  }

  let home = dir.join("display");
  let runtime_dir = home.join("runtime");
  let xauthority = home.join("Xauthority");
  let cookie: [u8; 16] = rand::rng().random();
  let written = std::fs::DirBuilder::new()
    .recursive(true)
    .mode(0o700)
    .create(&runtime_dir)
    .and_then(|()| {
      std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&xauthority)
    })
    .and_then(|mut file| std::io::Write::write_all(&mut file, &authority_entry(&cookie)));

  written.map_err(|why| format!("A private display is unavailable: {why}"))?;

  let mut server = Command::new(&setpriv)
    .args(["--pdeathsig", "SIGKILL"])
    .arg(&xvfb)
    .args(["-displayfd", "1", "-auth"])
    .arg(&xauthority)
    .args(["-nolisten", "tcp", "-screen", "0", "1920x1080x24"])
    .env_clear()
    .env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_owned()))
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .map_err(|why| format!("A private display is unavailable: {why}"))?;
  let mut said = String::new();
  let mut number = None;

  if let Some(mut out) = server.stdout.take() {
    let mut chunk = [0u8; 64];

    while number.is_none() {
      let left = deadline.saturating_duration_since(Instant::now());

      if left.is_zero() {
        break;
      }

      match tokio::time::timeout(left, out.read(&mut chunk)).await {
        Ok(Ok(read)) if read > 0 => said.push_str(&String::from_utf8_lossy(&chunk[..read])),
        _ => break,
      }

      number = said
        .split_once('\n')
        .map(|(first, _)| first)
        .filter(|first| !first.is_empty() && first.bytes().all(|byte| byte.is_ascii_digit()))
        .map(str::to_owned);
    }
  }

  let display = Display {
    display: String::new(),
    xauthority,
    runtime_dir,
    server,
  };

  let Some(number) = number else {
    display.stop().await;

    return Err("The private display did not start before the web deadline".to_owned());
  };

  Ok(Display {
    display: format!(":{number}"),
    ..display
  })
}
