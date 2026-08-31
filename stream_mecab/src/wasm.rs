use crate::{DeltaStreamAnalyzer, Model, StreamDelta, encode_delta_into};
use std::str;

pub struct Handle {
    model: Model,
    stream: Option<DeltaStreamAnalyzer>,
    delta: StreamDelta,
    input: Vec<u8>,
    output: Vec<u8>,
    error: Vec<u8>,
}

impl Handle {
    fn new() -> Self {
        Self {
            model: Model::new(),
            stream: None,
            delta: StreamDelta::default(),
            input: Vec::new(),
            output: Vec::new(),
            error: Vec::new(),
        }
    }

    fn fail(&mut self, message: impl AsRef<str>) -> u32 {
        self.error.clear();
        self.error.extend_from_slice(message.as_ref().as_bytes());
        0
    }

    fn clear_error(&mut self) {
        self.error.clear();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sm_create() -> *mut Handle {
    Box::into_raw(Box::new(Handle::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_destroy(handle: *mut Handle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_input_reserve(handle: *mut Handle, len: usize) -> *mut u8 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return std::ptr::null_mut();
    };
    handle.input.resize(len, 0);
    if len == 0 {
        std::ptr::null_mut()
    } else {
        handle.input.as_mut_ptr()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_add_tsv_input(handle: *mut Handle, len: usize) -> u32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    if handle.stream.is_some() {
        return handle.fail("dictionary cannot be changed after sm_start");
    }
    if len > handle.input.len() {
        return handle.fail("input length exceeds reserved buffer");
    }
    let Ok(text) = str::from_utf8(&handle.input[..len]) else {
        return handle.fail("dictionary input is not UTF-8");
    };
    match handle.model.add_tsv(text) {
        Ok(_) => {
            handle.clear_error();
            1
        }
        Err(error) => handle.fail(error.to_string()),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_load_compiled_input(handle: *mut Handle, len: usize) -> u32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    if handle.stream.is_some() {
        return handle.fail("dictionary cannot be changed after sm_start");
    }
    if len > handle.input.len() {
        return handle.fail("input length exceeds reserved buffer");
    }
    match Model::from_compiled(&handle.input[..len]) {
        Ok(model) => {
            handle.model = model;
            handle.clear_error();
            1
        }
        Err(error) => handle.fail(error.to_string()),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_set_transition(
    handle: *mut Handle,
    previous: u32,
    next: u32,
    cost: i32,
) -> u32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    if handle.stream.is_some() {
        return handle.fail("model cannot be changed after sm_start");
    }
    let (Ok(previous), Ok(next)) = (u16::try_from(previous), u16::try_from(next)) else {
        return handle.fail("tag id exceeds u16");
    };
    handle.model.set_transition(previous, next, cost);
    handle.clear_error();
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_set_max_unknown_chars(handle: *mut Handle, chars: usize) -> u32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    if handle.stream.is_some() {
        return handle.fail("model cannot be changed after sm_start");
    }
    handle.model.set_max_unknown_chars(chars);
    handle.clear_error();
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_start(handle: *mut Handle) -> u32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    handle.stream = Some(handle.model.clone().stream_delta());
    handle.delta.retract = 0;
    handle.delta.push.clear();
    encode_delta_into(&handle.delta, &mut handle.output);
    handle.clear_error();
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_append_input(handle: *mut Handle, len: usize) -> u32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    if len > handle.input.len() {
        return handle.fail("input length exceeds reserved buffer");
    }
    let Ok(text) = str::from_utf8(&handle.input[..len]) else {
        return handle.fail("stream input is not UTF-8");
    };
    let Some(stream) = handle.stream.as_mut() else {
        return handle.fail("call sm_start before appending");
    };
    stream.append_into(text, &mut handle.delta);
    encode_delta_into(&handle.delta, &mut handle.output);
    handle.clear_error();
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_finish(handle: *mut Handle) -> u32 {
    let Some(handle) = (unsafe { handle.as_mut() }) else {
        return 0;
    };
    let Some(stream) = handle.stream.as_mut() else {
        return handle.fail("call sm_start before finishing");
    };
    stream.finish_into(&mut handle.delta);
    encode_delta_into(&handle.delta, &mut handle.output);
    handle.clear_error();
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_delta_ptr(handle: *const Handle) -> *const u8 {
    unsafe { handle.as_ref() }.map_or(std::ptr::null(), |handle| handle.output.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_delta_len(handle: *const Handle) -> usize {
    unsafe { handle.as_ref() }.map_or(0, |handle| handle.output.len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_error_ptr(handle: *const Handle) -> *const u8 {
    unsafe { handle.as_ref() }.map_or(std::ptr::null(), |handle| handle.error.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_error_len(handle: *const Handle) -> usize {
    unsafe { handle.as_ref() }.map_or(0, |handle| handle.error.len())
}
