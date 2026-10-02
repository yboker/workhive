//! In-memory `ToolLaunchInstrumentationPort` fake for use-case tests.
//!
//! Captures the most recent wrap requests so tests can assert that the
//! launch path actually went through the supervisor wrapper, and lets
//! tests force a wrap failure to verify the use-case fallback contract.

use std::cell::RefCell;

use workhive_core::ports::{
    ToolLaunchInstrumentationError, ToolLaunchInstrumentationPort, WrapToolCommandRequest,
};

/// Captures every request the use case sends to the instrumentation
/// port, returning a deterministic supervisor-style command string.
#[derive(Default)]
pub struct FakeToolLaunchInstrumentation {
    /// When set, `wrap_tool_command` returns this error instead of
    /// producing a wrapped command.
    force_error: RefCell<Option<ToolLaunchInstrumentationError>>,
    /// Recorded requests in invocation order.
    captured: RefCell<Vec<WrapToolCommandRequest>>,
}

impl FakeToolLaunchInstrumentation {
    pub fn new() -> Self {
        Self::default()
    }

    /// Force the next `wrap_tool_command` calls to fail with the given
    /// error so the use-case fallback path can be exercised.
    pub fn set_force_error(&self, err: ToolLaunchInstrumentationError) {
        *self.force_error.borrow_mut() = Some(err);
    }

    /// Drain captured requests for assertion.
    pub fn captured(&self) -> Vec<WrapToolCommandRequest> {
        self.captured.borrow().clone()
    }
}

impl ToolLaunchInstrumentationPort for FakeToolLaunchInstrumentation {
    fn wrap_tool_command(
        &self,
        request: WrapToolCommandRequest,
    ) -> Result<String, ToolLaunchInstrumentationError> {
        if let Some(err) = self.force_error.borrow_mut().take() {
            return Err(err);
        }
        let unit_args = request
            .unit_id
            .as_ref()
            .map(|unit_id| vec!["--unit".to_owned(), unit_id.as_str().to_owned()])
            .unwrap_or_default();
        let argv: Vec<String> = std::iter::once("/usr/local/bin/workhive".to_owned())
            .chain(["internal".to_owned(), "pane-supervisor".to_owned()])
            .chain(unit_args)
            .chain(
                request
                    .tool_name
                    .as_deref()
                    .map(|t| vec!["--tool".to_owned(), t.to_owned()])
                    .unwrap_or_default(),
            )
            .chain(std::iter::once("--".to_owned()))
            .chain(std::iter::once(request.program.clone()))
            .chain(request.args.iter().cloned())
            .collect();
        self.captured.borrow_mut().push(request);
        Ok(argv.join(" "))
    }
}
