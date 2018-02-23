use std::mem;
use std::result;
pub use failure::{Error, ResultExt};

use emacs_module::*;
use super::{Env, Value};
use super::IntoLisp;

/// We assume that the C code in Emacs really treats it as an enum and doesn't return an undeclared
/// value, but we still need to safeguard against possible compatibility issue (Emacs may add more
/// statuses in the future). FIX: Use an enum, and check for compatibility on load. Possible or not?
pub type FuncallExit = emacs_funcall_exit;

const RETURN: FuncallExit = emacs_funcall_exit_emacs_funcall_exit_return;
const SIGNAL: FuncallExit = emacs_funcall_exit_emacs_funcall_exit_signal;
const THROW: FuncallExit = emacs_funcall_exit_emacs_funcall_exit_throw;

#[derive(Debug)]
pub struct TempValue {
    raw: emacs_value,
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Non-local signal: symbol={:?} data={:?}", symbol, data)]
    Signal { symbol: TempValue, data: TempValue },

    #[fail(display = "Non-local throw: tag={:?} value={:?}", tag, value)]
    Throw { tag: TempValue, value: TempValue },

    #[fail(display = "Wrong user-pointer type, expected: {}", expected)]
    UserPtrHasWrongType { expected: &'static str },

    #[fail(display = "Invalid user-pointer, expected: {}", expected)]
    UnknownUserPtr { expected: &'static str },

    #[fail(display = "Invalid symbol name")]
    InvalidSymbol,

    #[fail(display = "Invalid string")]
    InvalidString,

    #[fail(display = "Invalid function name")]
    InvalidFunction,
}

pub type Result<T> = result::Result<T, Error>;

impl TempValue {
    unsafe fn new(raw: emacs_value) -> Self {
        Self { raw }
    }

    /// # Safety
    ///
    /// This must only be temporarily used to inspect a non-local signal/throw from Lisp.
    pub unsafe fn value<'e>(&self, env: &'e Env) -> Value<'e> {
        Value::new(self.raw, env)
    }
}

/// Technically these are unsound, but they are necessary to use the `Fail` trait. We ensure safety
/// by marking TempValue methods as unsafe.
unsafe impl Send for TempValue {}
unsafe impl Sync for TempValue {}

impl Env {
    /// Handles possible non-local exit after calling Lisp code.
    pub(crate) fn handle_exit<T>(&self, result: T) -> Result<T> {
        let mut symbol = unsafe { mem::uninitialized() };
        let mut data = unsafe { mem::uninitialized() };
        let status = self.non_local_exit_get(&mut symbol, &mut data);
        match (status, symbol, data) {
            (RETURN, ..) => Ok(result),
            (SIGNAL, symbol, data) => {
                self.non_local_exit_clear();
                Err(ErrorKind::Signal {
                    symbol: unsafe { TempValue::new(symbol) },
                    data: unsafe { TempValue::new(data) },
                }.into())
            },
            (THROW, tag, value) => {
                self.non_local_exit_clear();
                Err(ErrorKind::Throw {
                    tag: unsafe { TempValue::new(tag) },
                    value: unsafe { TempValue::new(value) },
                }.into())
            },
            _ => panic!("Unexpected non local exit status {}", status),
        }
    }

    /// Converts a Rust's `Result` to either a normal value, or a non-local exit in Lisp.
    pub(crate) unsafe fn maybe_exit(&self, result: Result<Value>) -> emacs_value {
        match result {
            Ok(v) => v.raw,
            Err(error) => {
                match error.downcast_ref::<ErrorKind>() {
                    Some(&ErrorKind::Signal { ref symbol, ref data }) =>
                        self.signal(symbol.raw, data.raw),
                    Some(&ErrorKind::Throw { ref tag, ref value }) =>
                        self.throw(tag.raw, value.raw),
                    // TODO: Internal
                    _ => self.signal_str("error", &format!("Error: {}", error))
                        .expect("Fail to signal error to Emacs"),
                }
            }
        }
    }

    // TODO: Prepare static values for the symbols.
    pub(crate) fn signal_str(&self, symbol: &str, message: &str) -> Result<emacs_value> {
        let message = message.into_lisp(&self)?;
        let data = self.list(&[message])?;
        let symbol = self.intern(symbol)?;
        unsafe {
            Ok(self.signal(symbol.raw, data.raw))
        }
    }

    fn non_local_exit_get(&self, symbol: &mut emacs_value, data: &mut emacs_value) -> FuncallExit {
        raw_call_no_exit!(self, non_local_exit_get, symbol as *mut emacs_value, data as *mut emacs_value)
    }

    fn non_local_exit_clear(&self) {
        raw_call_no_exit!(self, non_local_exit_clear)
    }

    /// # Safety
    ///
    /// The given raw values must still live.
    #[allow(unused_unsafe)]
    unsafe fn throw(&self, tag: emacs_value, value: emacs_value) -> emacs_value {
        raw_call_no_exit!(self, non_local_exit_throw, tag, value);
        tag
    }

    /// # Safety
    ///
    /// The given raw values must still live.
    #[allow(unused_unsafe)]
    unsafe fn signal(&self, symbol: emacs_value, data: emacs_value) -> emacs_value {
        raw_call_no_exit!(self, non_local_exit_signal, symbol, data);
        symbol
    }
}
