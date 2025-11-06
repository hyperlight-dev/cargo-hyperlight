#![no_std]
#![no_main]

extern crate alloc;

const _: () = {
    #[cfg(not(hyperlight))]
    compile_error!("This crate can only be compiled for hyperlight targets");
};

use alloc::format;
use alloc::vec::Vec;

use hyperlight_common::flatbuffer_wrappers::function_call::*;
use hyperlight_common::flatbuffer_wrappers::function_types::*;
use hyperlight_common::flatbuffer_wrappers::guest_error::ErrorCode;
use hyperlight_common::flatbuffer_wrappers::util::*;
use hyperlight_guest::error::{HyperlightGuestError, Result};
use hyperlight_guest_bin::guest_function::definition::*;
use hyperlight_guest_bin::guest_function::register::*;

mod ffi {
    #![allow(dead_code, non_camel_case_types)] // generated code
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

fn host_print(s: impl AsRef<[u8]>) -> i32 {
    let s = s.as_ref();
    unsafe { ffi::host_print(s.as_ptr() as _, s.len()) }
}

pub fn say_hello(func: &FunctionCall) -> Result<Vec<u8>> {
    let params = func.parameters.as_deref().unwrap_or_default();
    let Some(ParameterValue::String(name)) = params.first() else {
        return Err(HyperlightGuestError::new(
            ErrorCode::GuestError,
            "Expected a string parameter".into(),
        ));
    };

    let n = host_print(format!("Hello {name}\n"));
    Ok(get_flatbuffer_result(n))
}

#[unsafe(no_mangle)]
pub extern "C" fn hyperlight_main() {
    register_function(GuestFunctionDefinition::new(
        "SayHello".into(),
        [ParameterType::String].into(),
        ReturnType::Int,
        say_hello as usize,
    ));
}

#[unsafe(no_mangle)]
pub fn guest_dispatch_function(_: FunctionCall) -> Result<Vec<u8>> {
    panic!("Invalid guest function call");
}
