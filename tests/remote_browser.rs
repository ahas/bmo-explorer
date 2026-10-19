//! The remote browser's tests (docs/remote-browser.md): the steps read, the secrets kept out of everything the
//! run writes, the drive of a node through its exec, and the launch and the runner against stand-ins for the
//! services they call.

#[path = "browse/stand_in.rs"]
mod stand_in;

mod remote_browser {
  mod drive;
  mod verb;
}
