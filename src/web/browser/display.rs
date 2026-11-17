//! A virtual X server the web layer starts for a browser of its own (docs/web-access.md, 「How the browser is
//! started」), so a non-headless browser never opens a window on a person's desktop session, never reaches that session's
//! D-Bus and never asks its keyring for anything. The server picks a free display number itself and accepts
//! only the random cookie written beside it, and it dies with the process that started it.

use std::collections::BTreeMap;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::web::error::{Failure, WebError, WebResult};

/// A display standing for one browser.
pub struct PrivateDisplay {
  pub display: String,
  pub xauthority: PathBuf,
  pub runtime_dir: PathBuf,
  child: Child,
}

/// The program that keeps a child bound to its parent's life: it dies with the process that started it.
pub fn setpriv() -> String {
  std::env::var("BMO_SETPRIV_PATH").unwrap_or_else(|_| "/usr/bin/setpriv".into())
}

/// The environment a browser on a private display runs with: the caller's own, less every variable of a
/// person's session, plus that display, a runtime directory of its own and a session bus switched off
/// rather than looked for.
pub fn detached_environment(
  base: impl IntoIterator<Item = (String, String)>,
  display: &str,
  xauthority: &Path,
  runtime_dir: &Path,
) -> BTreeMap<String, String> {
  let session = &super::detached::DETACHED.session_variables;
  let mut environment: BTreeMap<String, String> =
    base.into_iter().filter(|(name, _)| !session.iter().any(|one| one == name)).collect();

  environment.insert("DISPLAY".into(), display.into());
  environment.insert("XAUTHORITY".into(), xauthority.to_string_lossy().into_owned());
  environment.insert("XDG_RUNTIME_DIR".into(), runtime_dir.to_string_lossy().into_owned());
  environment.insert("DBUS_SESSION_BUS_ADDRESS".into(), "disabled:".into());
  environment
}

// An authority file holding one MIT-MAGIC-COOKIE-1 entry for every address and display, which lets the
// cookie be written before the server has said which display number it took.
pub fn authority_entry(cookie: &[u8]) -> Vec<u8> {
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

impl PrivateDisplay {
  /// The server started under `dir`, which the caller owns and removes, waited for until it names its display.
  /// A server that ends its output without naming one is stopped and reported unavailable.
  pub async fn start(dir: &Path) -> WebResult<Self> {
    let xvfb = std::env::var("BMO_XVFB_PATH").unwrap_or_else(|_| "/usr/bin/Xvfb".into());
    let setpriv = setpriv();

    if !Path::new(&xvfb).exists() || !Path::new(&setpriv).exists() {
      return Err(WebError::new(
        format!("A private display is unavailable ({xvfb}, {setpriv})"),
        Failure::BrowserUnavailable,
      ));
    }

    let home = dir.join("display");
    let runtime_dir = home.join("runtime");
    let unavailable = |why: std::io::Error| {
      WebError::new(format!("A private display is unavailable: {why}"), Failure::BrowserUnavailable)
    };

    std::fs::DirBuilder::new()
      .recursive(true)
      .mode(0o700)
      .create(&runtime_dir)
      .map_err(unavailable)?;

    let xauthority = home.join("Xauthority");
    let cookie: [u8; 16] = rand::random();

    {
      use std::io::Write;

      let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&xauthority)
        .map_err(unavailable)?;

      file.write_all(&authority_entry(&cookie)).map_err(unavailable)?;
    }

    let mut child = Command::new(&setpriv)
      .args(["--pdeathsig", "SIGKILL", &xvfb, "-displayfd", "1", "-auth"])
      .arg(&xauthority)
      .args(["-nolisten", "tcp", "-screen", "0", "1920x1080x24"])
      .env_clear()
      .env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()))
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .kill_on_drop(true)
      .spawn()
      .map_err(unavailable)?;
    let mut stdout = child.stdout.take().expect("the display's output was asked for");
    let mut said = String::new();
    let mut number: Option<String> = None;

    while number.is_none() {
      let mut chunk = [0_u8; 64];
      let read = match stdout.read(&mut chunk).await {
        Ok(read) if read > 0 => read,
        _ => break,
      };

      said.push_str(&String::from_utf8_lossy(&chunk[..read]));

      if let Some((line, _)) = said.split_once('\n')
        && !line.is_empty()
        && line.chars().all(|c| c.is_ascii_digit())
      {
        number = Some(line.to_owned());
      }
    }

    let mut display = Self {
      display: String::new(),
      xauthority,
      runtime_dir,
      child,
    };

    match number {
      Some(number) => {
        display.display = format!(":{number}");
        Ok(display)
      }
      None => {
        display.stop().await;
        Err(WebError::new(
          "The private display ended without naming its display",
          Failure::BrowserUnavailable,
        ))
      }
    }
  }

  pub fn pid(&self) -> Option<u32> {
    self.child.id()
  }

  /// The server asked to end, and waited for until it has: Xvfb ends on `SIGTERM`.
  pub async fn stop(&mut self) {
    if let Some(pid) = self.child.id() {
      // SAFETY: a signal sent to the server this display started and still holds.
      unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
      }
    }

    let _ = self.child.wait().await;
  }
}
