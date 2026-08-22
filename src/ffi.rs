use crate::consts;
use crate::platform;
use crate::sys;

pub(crate) unsafe fn context_properties(
    properties: *const sys::cl_context_properties,
) -> Vec<sys::cl_context_properties> {
    if properties.is_null() {
        return Vec::new();
    }

    let mut collected = Vec::new();
    let mut cursor = properties;

    unsafe {
        while *cursor != 0 {
            collected.push(*cursor);
            cursor = cursor.add(1);
        }
    }
    collected.push(0);

    collected
}

pub(crate) unsafe fn create_context(
    properties: *const sys::cl_context_properties,
    devices: Vec<sys::cl_device_id>,
    pfn_notify: Option<sys::cl_context_callback>,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_context {
    if pfn_notify.is_none() && !user_data.is_null() {
        return unsafe { context_error(consts::CL_INVALID_VALUE, errcode_ret) };
    }

    let collected = unsafe { context_properties(properties) };
    if let Err(error) = platform::VoddContext::validate_properties(&collected) {
        return unsafe { context_error(error, errcode_ret) };
    }

    let id = platform::VoddContext::create(collected, devices, pfn_notify, user_data);

    unsafe { info_write(errcode_ret, consts::CL_SUCCESS) };

    object_handle(id)
}

pub(crate) unsafe fn context_error(
    error: sys::cl_int,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_context {
    unsafe { info_write(errcode_ret, error) };

    core::ptr::null_mut()
}

pub(crate) unsafe fn queue_error(
    error: sys::cl_int,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_command_queue {
    unsafe { info_write(errcode_ret, error) };
    core::ptr::null_mut()
}

pub(crate) fn object_handle<T>(id: sys::cl_uint) -> *mut T {
    id as usize as *mut T
}

pub(crate) fn object_id<T>(handle: *mut T) -> sys::cl_uint {
    handle as usize as sys::cl_uint
}

pub(crate) unsafe fn info_bytes(
    src: &[u8],
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    if !param_value.is_null() {
        if param_value_size < src.len() {
            return consts::CL_INVALID_VALUE;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), param_value.cast::<u8>(), src.len());
        }
    }

    if !param_value_size_ret.is_null() {
        unsafe {
            *param_value_size_ret = src.len();
        }
    }

    consts::CL_SUCCESS
}

pub(crate) unsafe fn info_slice<T>(
    values: &[T],
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    let bytes = unsafe {
        core::slice::from_raw_parts(values.as_ptr().cast::<u8>(), core::mem::size_of_val(values))
    };

    unsafe { info_bytes(bytes, param_value_size, param_value, param_value_size_ret) }
}

pub(crate) unsafe fn info_scalar<T>(
    value: T,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        info_slice(
            &[value],
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

pub(crate) unsafe fn info_value(
    value: platform::InfoValue<'_>,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        match value {
            platform::InfoValue::Uint(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Ulong(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Size(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Handle(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Handles(v) => {
                info_slice(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Sizes(v) => {
                info_slice(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Properties(v) => {
                info_slice(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Text(v) => {
                info_bytes(v, param_value_size, param_value, param_value_size_ret)
            }
        }
    }
}

pub(crate) unsafe fn info_write(out: *mut sys::cl_int, value: sys::cl_int) {
    if !out.is_null() {
        unsafe { *out = value };
    }
}
