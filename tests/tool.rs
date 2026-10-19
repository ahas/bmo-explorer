//! The call protocol's tests (the tool host's docs/mcp-host.md, 「A call」), one module for each module of
//! `bmo_explorer::tool`, read through its public names as a tool reads them.

mod tool {
  mod answer;
  mod ceiling;
  mod input;
  mod kept;
  mod manifest;
  mod protocol;
  mod served;
  mod session;
}
