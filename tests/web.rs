//! The web layer's tests (docs/web-access.md), one module per module of `bmo_explorer::web`, and the stand-ins
//! they read the web through in `testing`.

mod address;
mod chrome;
mod chrome_display;
mod chrome_places;
mod chrome_profile;
mod chrome_proxy;
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
