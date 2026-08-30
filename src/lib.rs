//! An append-only Markdown parser designed for token-streamed LLM output.
//!
//! The parser returns changes to a small, renderer-independent AST. Ordinary
//! blocks use suffix replacement; open fenced code blocks use a byte splice so
//! a multi-megabyte code block is never copied into every delta.

mod binary;
mod inline;
mod parser;

pub use binary::{encode_delta, encode_delta_into};
pub use parser::{Block, Delta, Inline, Op, Parser};

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{Delta, Parser};
    use std::{cell::RefCell, mem, slice, str};

    pub struct Handle {
        parser: Parser,
        delta: Delta,
        output: Vec<u8>,
        input: Vec<u8>,
    }

    thread_local! {
        static ALLOCS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn md_create() -> *mut Handle {
        Box::into_raw(Box::new(Handle {
            parser: Parser::new(),
            delta: Delta::default(),
            output: Vec::new(),
            input: Vec::new(),
        }))
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_destroy(handle: *mut Handle) {
        if !handle.is_null() {
            drop(unsafe { Box::from_raw(handle) });
        }
    }

    /// Allocates input memory. The returned pointer must be passed to md_free.
    #[unsafe(no_mangle)]
    pub extern "C" fn md_alloc(len: usize) -> *mut u8 {
        let mut buf = vec![0; len];
        let ptr = buf.as_mut_ptr();
        ALLOCS.with(|a| a.borrow_mut().push(buf));
        ptr
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn md_free(ptr: *mut u8) {
        ALLOCS.with(|a| {
            let mut allocations = a.borrow_mut();
            if let Some(i) = allocations.iter().position(|v| v.as_ptr() == ptr) {
                allocations.swap_remove(i);
            }
        });
    }

    fn append_utf8(h: &mut Handle, bytes: &[u8]) -> u32 {
        let Ok(text) = str::from_utf8(bytes) else {
            return 0;
        };
        h.parser.append_into(text, &mut h.delta);
        crate::encode_delta_into(&h.delta, &mut h.output);
        1
    }

    /// Returns reusable input memory owned by this parser handle. The buffer is
    /// valid until another call that can grow the handle input buffer.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_input_reserve(handle: *mut Handle, len: usize) -> *mut u8 {
        let Some(h) = (unsafe { handle.as_mut() }) else {
            return std::ptr::null_mut();
        };
        h.input.resize(len, 0);
        if len == 0 {
            std::ptr::null_mut()
        } else {
            h.input.as_mut_ptr()
        }
    }

    /// Appends bytes previously written into `md_input_reserve` memory. This is
    /// the zero-allocation hot path used by the JavaScript streaming wrapper.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_append_input(handle: *mut Handle, len: usize) -> u32 {
        let Some(h) = (unsafe { handle.as_mut() }) else {
            return 0;
        };
        if len > h.input.len() {
            return 0;
        }
        let bytes = &h.input[..len];
        // Parser append may mutate other handle fields, so validate UTF-8 first
        // and then borrow the parser/output fields independently.
        let Ok(text) = str::from_utf8(bytes) else {
            return 0;
        };
        h.parser.append_into(text, &mut h.delta);
        crate::encode_delta_into(&h.delta, &mut h.output);
        1
    }

    /// Appends UTF-8 and stores an MDA1 delta in the handle's output buffer.
    /// Returns 1 on success and 0 for invalid arguments/UTF-8. Kept for ABI
    /// compatibility; new JavaScript callers should use the reusable input path.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_append(handle: *mut Handle, ptr: *const u8, len: usize) -> u32 {
        let Some(h) = (unsafe { handle.as_mut() }) else {
            return 0;
        };
        if ptr.is_null() && len != 0 {
            return 0;
        }
        let bytes = if len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(ptr, len) }
        };
        append_utf8(h, bytes)
    }

    /// Clears the document and stores its truncate delta in the output buffer.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_reset(handle: *mut Handle) -> u32 {
        let Some(h) = (unsafe { handle.as_mut() }) else {
            return 0;
        };
        crate::encode_delta_into(&h.parser.reset(), &mut h.output);
        1
    }

    /// Finalizes the last streamed block and stores its delta.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_finish(handle: *mut Handle) -> u32 {
        let Some(h) = (unsafe { handle.as_mut() }) else {
            return 0;
        };
        crate::encode_delta_into(&h.parser.finish(), &mut h.output);
        1
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_delta_ptr(handle: *const Handle) -> *const u8 {
        unsafe { handle.as_ref() }.map_or(std::ptr::null(), |h| h.output.as_ptr())
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn md_delta_len(handle: *const Handle) -> usize {
        unsafe { handle.as_ref() }.map_or(0, |h| h.output.len())
    }

    // Keep `mem` referenced on wasm configurations whose optimizer diagnoses it.
    const _: usize = mem::size_of::<usize>();
}
