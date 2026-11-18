//! The repository's one test harness (bmo-chat's docs/conventions.md, 「Rust builds」): the checks that span the
//! repository and each area's tests a module per file, an area's own modules in the folder of its name, and the
//! web layer's stand-ins in `web::testing`. Every module shares the process.

mod client;
mod forms;
mod import;
mod mcp;
mod openapi;
mod repository;
mod site;
mod store;
mod tool;
mod tools;
mod web;
mod web_helper;
