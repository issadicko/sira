mod run;
mod sandbox;
mod vars;

pub use run::{
    assert, run, AssertionOutcome, AssertionSpec, Callbacks, Input, LogLine, NextRequest, Output, PathParam, Phase,
    ResponseSize, ScriptRequest, ScriptResponse, TestResult,
};
pub use sandbox::{Clock, Limits, Sandbox, ScriptError};
pub use vars::{Dirty, Vars, ENV_NAME};

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn run(source: &str) -> Result<(), ScriptError> {
        Sandbox::new(Limits { compute: Duration::from_millis(300), ..Limits::default() }).unwrap().run(source)
    }

    #[test]
    fn enf_sec_02_an_endless_loop_is_stopped_after_the_compute_budget() {
        let err = run("(async () => { while (true) {} })()").unwrap_err();
        assert!(err.message.contains("interrompu"), "{err:?}");
    }

    #[test]
    fn enf_sec_02_memory_is_capped() {
        let sandbox = Sandbox::new(Limits { memory: 8 << 20, ..Limits::default() }).unwrap();
        let err =
            sandbox.run("(async () => { const a = []; while (true) a.push('x'.repeat(1 << 20)); })()").unwrap_err();
        assert!(err.message.contains("out of memory"), "{err:?}");
    }

    #[test]
    fn enf_sec_02_the_sandbox_reaches_neither_disk_nor_network_nor_processes() {
        let source = "(async () => {
            for (const name of ['require', 'process', 'fetch', 'XMLHttpRequest', 'std', 'os', 'Deno', 'Bun'])
                if (typeof globalThis[name] !== 'undefined') throw new Error(name);
        })()";
        run(source).unwrap();
    }

    #[test]
    fn a_thrown_error_reports_its_message() {
        let err = run("(async () => { throw new Error('boom'); })()").unwrap_err();
        assert_eq!(err.message, "boom");
    }
}
