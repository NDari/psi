//! Limits on every Lua state (psi-plan.md §3.3): a memory cap, and a stop
//! for code that runs too long without yielding to psi. They catch bugs;
//! extensions are trusted (D-25).
//!
//! The stop is an error raised from an instruction hook. A loop could catch
//! it with pcall and go on, so pcall and xpcall re-raise it, and coroutines
//! get the same hook (each Lua thread has its own). Time spent awaiting psi
//! (a confirm dialog, a tool call) doesn't count.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{HookTriggers, Lua, Thread, VmState};

/// The text of the stop error; the wrappers look for it.
pub const KILLED: &str = "psi: the extension ran too long without yielding and was stopped";

/// How often the hook looks at the clock, in Lua instructions.
const EVERY: u32 = 10_000;

#[derive(Debug, Clone)]
pub struct Limits {
    pub memory: usize,
    /// The longest a call may run without awaiting psi.
    pub busy: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory: 256 << 20,
            busy: Duration::from_secs(5),
        }
    }
}

/// Since when Lua has been running without awaiting psi.
#[derive(Debug, Clone, Default)]
pub struct Busy(Rc<Cell<Option<Instant>>>);

impl Busy {
    pub fn start(&self) {
        self.0.set(Some(Instant::now()));
    }
    pub fn stop(&self) {
        self.0.set(None);
    }
    fn over(&self, max: Duration) -> bool {
        self.0.get().is_some_and(|t| t.elapsed() > max)
    }
}

fn hook(busy: Busy, max: Duration) -> impl Fn(&Lua, &mlua::debug::Debug) -> mlua::Result<VmState> {
    move |_, _| {
        if busy.over(max) {
            Err(mlua::Error::runtime(KILLED))
        } else {
            Ok(VmState::Continue)
        }
    }
}

/// Applies the limits to `lua`; returns the clock the host keeps.
pub fn install(lua: &Lua, limits: &Limits) -> mlua::Result<Busy> {
    lua.set_memory_limit(limits.memory)?;
    let busy = Busy::default();
    let max = limits.busy;
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(EVERY),
        hook(busy.clone(), max),
    )?;
    let b = busy.clone();
    let hook_thread = lua.create_function(move |_, co: Thread| {
        co.set_hook(
            HookTriggers::new().every_nth_instruction(EVERY),
            hook(b.clone(), max),
        )?;
        Ok(co)
    })?;
    lua.load(
        r#"
        local hook, KILLED = ...
        local create, resume = coroutine.create, coroutine.resume
        coroutine.create = function(f) return hook(create(f)) end
        coroutine.wrap = function(f)
          local co = coroutine.create(f)
          return function(...)
            local r = table.pack(resume(co, ...))
            if not r[1] then error(r[2], 0) end
            return table.unpack(r, 2, r.n)
          end
        end
        local raw_pcall, raw_xpcall = pcall, xpcall
        local function killed(e) return tostring(e):find(KILLED, 1, true) ~= nil end
        local function check(ok, ...)
          if not ok and killed((...)) then error((...), 0) end
          return ok, ...
        end
        pcall = function(f, ...) return check(raw_pcall(f, ...)) end
        xpcall = function(f, h, ...)
          return check(raw_xpcall(f, function(e)
            if killed(e) then return e end
            return h(e)
          end, ...))
        end
        "#,
    )
    .set_name("=psi limits")
    .call::<()>((hook_thread, KILLED))?;
    Ok(busy)
}

/// A Lua thread for one call from psi, with the hook set.
pub fn thread(lua: &Lua, f: mlua::Function, busy: &Busy, max: Duration) -> mlua::Result<Thread> {
    let t = lua.create_thread(f)?;
    t.set_hook(
        HookTriggers::new().every_nth_instruction(EVERY),
        hook(busy.clone(), max),
    )?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(code: &str) -> (String, Duration) {
        let lua = Lua::new();
        let limits = Limits {
            memory: 32 << 20,
            busy: Duration::from_millis(100),
        };
        let busy = install(&lua, &limits).unwrap();
        let f: mlua::Function = lua.load(code).into_function().unwrap();
        let t = thread(&lua, f, &busy, limits.busy).unwrap();
        busy.start();
        let start = Instant::now();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let r = rt.block_on(t.into_async::<()>(()).unwrap());
        busy.stop();
        let msg = r.err().map(|e| e.to_string()).unwrap_or_default();
        (msg, start.elapsed())
    }

    #[test]
    fn runaway_loops_are_stopped_in_every_shape() {
        for code in [
            "while true do end",
            "while true do pcall(function() while true do end end) end",
            "while true do xpcall(function() while true do end end, function(e) return e end) end",
            "local co = coroutine.wrap(function() while true do end end) while true do pcall(co) end",
            "while true do coroutine.resume(coroutine.create(function() while true do end end)) end",
        ] {
            let (msg, took) = run(code);
            assert!(msg.contains(KILLED), "{code}: {msg}");
            assert!(took < Duration::from_secs(2), "{code}: {took:?}");
        }
    }

    /// The 5.3-compat math functions are there (they need libm's cosh and
    /// sinh, which tie a glibc build to the build machine's glibc).
    #[test]
    fn math_cosh_and_sinh_still_work() {
        let (msg, _) = run(
            "assert(math.cosh(0) == 1) assert(math.abs(math.sinh(1) - 1.1752011936438014) < 1e-15) assert(math.sinh(1e-10) == 1e-10)",
        );
        assert_eq!(msg, "");
    }

    #[test]
    fn memory_is_capped_and_normal_errors_still_catch() {
        let (msg, _) =
            run("local t = {} for i = 1, 1e8 do t[i] = string.rep('x', 1 << 20) .. i end");
        assert!(msg.contains("not enough memory"), "{msg}");
        let (msg, _) = run("local ok, e = pcall(error, 'boom') assert(not ok and e == 'boom')");
        assert_eq!(msg, "");
    }
}
