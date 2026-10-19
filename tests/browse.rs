//! The browse run's tests (docs/browse-runs.md): what its runner says and asks, the display of its own, and the
//! launch and the runner as bmo-agency and a shell run them, against stand-ins for the services they call.

mod browse {
  mod display;
  mod runner;
  mod stand_in;
  mod verb;
}
