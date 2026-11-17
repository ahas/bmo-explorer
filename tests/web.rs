//! The web layer's tests (docs/web-access.md), one module per module of `bmo_explorer::web`, and the stand-ins
//! they read the web through in `testing`.

mod address;
mod browser;
mod browser_challenge;
mod browser_copy;
mod browser_display;
mod browser_firefox;
mod browser_places;
mod browser_profile;
mod browser_proxy;
mod client;
mod identity;
mod jar;
mod pacing;
mod pdf;
mod picture;
mod picture_raster;
mod roster;
mod search;
mod session;

pub(crate) mod testing;
