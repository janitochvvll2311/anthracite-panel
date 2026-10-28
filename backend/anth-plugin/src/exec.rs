//! The capability seam.
//!
//! A plugin never runs anything itself: it names a capability and the host
//! decides what that means. [`Executor`] is that boundary, and the only reason
//! a plugin's logic can be tested on the host is that a test implements it over
//! canned answers.

use alloc::string::String;

use serde_json::Value;

/// Runs one capability, as the host lends it to a guest.
pub trait Executor {
    /// Runs `capability` with `args`, returning the host's JSON answer.
    ///
    /// `Err` is the host refusing or failing, with a detail meant for a human.
    /// A plugin should turn it into a [`crate::Response`] rather than swallow it:
    /// a refused capability served as `200` is an operator who cannot debug it.
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String>;

    /// Writes one line to the daemon log, at `info`.
    fn log(&mut self, message: &str) {
        self.log_at(crate::abi::LogLevel::Info, message);
    }

    /// Writes one line at a level of the plugin's choosing.
    ///
    /// The level is **relative to the daemon's**: a `debug` line is shown when
    /// the operator asked for `debug`, not before. A plugin that logged at an
    /// absolute level could raise the daemon's verbosity by itself, which is the
    /// wrong direction for a guest to be trusted with.
    fn log_at(&mut self, level: crate::abi::LogLevel, message: &str);

    /// Runs a capability that takes no arguments.
    ///
    /// Sends `{}` rather than `null`: the host decodes arguments into a map, so
    /// `null` is refused and `{}` is the honest empty case.
    fn exec_bare(&mut self, capability: &str) -> Result<Value, String> {
        self.exec(capability, &serde_json::json!({}))
    }
}
