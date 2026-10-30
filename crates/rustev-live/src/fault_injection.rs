//! Test-only faults scoped to the calling thread. Drive the complete run on
//! a current-thread executor while this scope is alive. Other tests are isolated.
//!
//! The first fault unwinds the run, leaving completed OS writes intact, including
//! unsynced complete journal lines and temporary files. This models process kill,
//! not power loss. Unwinding bypasses normal cleanup; destructor settlements
//! return an error before touching disk. Drop the run, runtime and journal before dropping
//! the scope; only the journal lock is cleaned up, as permitted by the harness.
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Default)]
struct State {
    target: Option<usize>,
    trace: Vec<String>,
    killed: bool,
}
thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

#[doc(hidden)]
#[derive(Debug)]
pub struct Crash;

#[doc(hidden)]
pub struct Scope(PhantomData<Rc<()>>);

impl Scope {
    #[doc(hidden)]
    pub fn new(target: Option<usize>) -> Self {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            assert!(s.is_none(), "nested fault scope");
            *s = Some(State {
                target,
                ..State::default()
            });
        });
        Self(PhantomData)
    }

    #[doc(hidden)]
    pub fn trace(&self) -> Vec<String> {
        STATE.with(|s| s.borrow().as_ref().unwrap().trace.clone())
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        STATE.with(|s| *s.borrow_mut() = None);
    }
}

pub(crate) fn point(kind: &str, name: &str, phase: &str) -> std::io::Result<()> {
    let crash = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(s) = s.as_mut() else {
            return Ok(false);
        };
        if s.killed {
            return Err(std::io::Error::other("simulated process has ended"));
        }
        s.trace.push(format!("{kind}:{name}:{phase}"));
        s.killed = s.target == Some(s.trace.len());
        Ok(s.killed)
    })?;
    if crash {
        // Bypass the panic printer: hundreds of intentional crashes are expected.
        std::panic::resume_unwind(Box::new(Crash));
    }
    Ok(())
}
