#![allow(unused_variables)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

pub mod consts;
pub mod device;
pub mod ffi;
pub mod interpreter;
pub mod spirv;

unsafe fn info_bytes(
    src: &[u8],
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
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

unsafe fn info_slice<T>(
    values: &[T],
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    let bytes = unsafe {
        core::slice::from_raw_parts(values.as_ptr().cast::<u8>(), core::mem::size_of_val(values))
    };

    unsafe { info_bytes(bytes, param_value_size, param_value, param_value_size_ret) }
}

unsafe fn info_scalar<T>(
    value: T,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    unsafe {
        info_slice(
            &[value],
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

unsafe fn info_value(
    value: device::InfoValue,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    unsafe {
        match value {
            device::InfoValue::Uint(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            device::InfoValue::Ulong(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            device::InfoValue::Size(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            device::InfoValue::Handle(v) => {
                info_scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            device::InfoValue::Sizes(v) => {
                info_slice(v, param_value_size, param_value, param_value_size_ret)
            }
            device::InfoValue::Properties(v) => {
                info_slice(v, param_value_size, param_value, param_value_size_ret)
            }
            device::InfoValue::Text(v) => {
                info_bytes(v, param_value_size, param_value, param_value_size_ret)
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetPlatformIDs(
    num_entries: ffi::cl_uint,
    platforms: *mut ffi::cl_platform_id,
    num_platforms: *mut ffi::cl_uint,
) -> ffi::cl_int {
    if platforms.is_null() && num_platforms.is_null() {
        return consts::CL_INVALID_VALUE;
    }

    if !platforms.is_null() && num_entries == 0 {
        return consts::CL_INVALID_VALUE;
    }

    if !platforms.is_null() {
        unsafe {
            *platforms = device::VoddDevice::platform_id();
        }
    }

    if !num_platforms.is_null() {
        unsafe {
            *num_platforms = 1;
        }
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetPlatformInfo(
    platform: ffi::cl_platform_id,
    param_name: ffi::cl_platform_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    if !platform.is_null() && !device::VoddDevice::is_platform_id(platform) {
        return consts::CL_INVALID_PLATFORM;
    }

    let Some(text) = device::VoddDevice::platform_info(param_name) else {
        return consts::CL_INVALID_VALUE;
    };

    unsafe { info_bytes(text, param_value_size, param_value, param_value_size_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetDeviceIDs(
    platform: ffi::cl_platform_id,
    device_type: ffi::cl_device_type,
    num_entries: ffi::cl_uint,
    devices: *mut ffi::cl_device_id,
    num_devices: *mut ffi::cl_uint,
) -> ffi::cl_int {
    if !platform.is_null() && !device::VoddDevice::is_platform_id(platform) {
        return consts::CL_INVALID_PLATFORM;
    }

    if devices.is_null() && num_devices.is_null() {
        return consts::CL_INVALID_VALUE;
    }

    if !devices.is_null() && num_entries == 0 {
        return consts::CL_INVALID_VALUE;
    }

    if !device::VoddDevice::is_valid_device_type(device_type) {
        return consts::CL_INVALID_DEVICE_TYPE;
    }

    if !devices.is_null() {
        unsafe {
            *devices = device::VoddDevice::device_id();
        }
    }

    if !num_devices.is_null() {
        unsafe {
            *num_devices = 1;
        }
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetDeviceInfo(
    device: ffi::cl_device_id,
    param_name: ffi::cl_device_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    if !device::VoddDevice::is_device_id(device) {
        return consts::CL_INVALID_DEVICE;
    }

    let Some(value) = device::VoddDevice::device_info(param_name) else {
        return consts::CL_INVALID_VALUE;
    };

    unsafe { info_value(value, param_value_size, param_value, param_value_size_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSubDevices(
    in_device: ffi::cl_device_id,
    properties: *const ffi::cl_device_partition_property,
    num_devices: ffi::cl_uint,
    out_devices: *mut ffi::cl_device_id,
    num_devices_ret: *mut ffi::cl_uint,
) -> ffi::cl_int {
    if !device::VoddDevice::is_device_id(in_device) {
        return consts::CL_INVALID_DEVICE;
    }

    consts::CL_INVALID_VALUE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainDevice(device: ffi::cl_device_id) -> ffi::cl_int {
    if !device::VoddDevice::is_device_id(device) {
        return consts::CL_INVALID_DEVICE;
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseDevice(device: ffi::cl_device_id) -> ffi::cl_int {
    if !device::VoddDevice::is_device_id(device) {
        return consts::CL_INVALID_DEVICE;
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateContext(
    properties: *const ffi::cl_context_properties,
    num_devices: ffi::cl_uint,
    devices: *const ffi::cl_device_id,
    pfn_notify: Option<
        unsafe extern "C" fn(
            *const core::ffi::c_char,
            *const core::ffi::c_void,
            usize,
            *mut core::ffi::c_void,
        ),
    >,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_context {
    panic!("clCreateContext is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateContextFromType(
    properties: *const ffi::cl_context_properties,
    device_type: ffi::cl_device_type,
    pfn_notify: Option<
        unsafe extern "C" fn(
            *const core::ffi::c_char,
            *const core::ffi::c_void,
            usize,
            *mut core::ffi::c_void,
        ),
    >,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_context {
    panic!("clCreateContextFromType is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainContext(context: ffi::cl_context) -> ffi::cl_int {
    panic!("clRetainContext is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseContext(context: ffi::cl_context) -> ffi::cl_int {
    panic!("clReleaseContext is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetContextInfo(
    context: ffi::cl_context,
    param_name: ffi::cl_context_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetContextInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainCommandQueue(command_queue: ffi::cl_command_queue) -> ffi::cl_int {
    panic!("clRetainCommandQueue is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseCommandQueue(
    command_queue: ffi::cl_command_queue,
) -> ffi::cl_int {
    panic!("clReleaseCommandQueue is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetCommandQueueInfo(
    command_queue: ffi::cl_command_queue,
    param_name: ffi::cl_command_queue_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetCommandQueueInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateBuffer(
    context: ffi::cl_context,
    flags: ffi::cl_mem_flags,
    size: usize,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_mem {
    panic!("clCreateBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSubBuffer(
    buffer: ffi::cl_mem,
    flags: ffi::cl_mem_flags,
    buffer_create_type: ffi::cl_buffer_create_type,
    buffer_create_info: *const core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_mem {
    panic!("clCreateSubBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateImage(
    context: ffi::cl_context,
    flags: ffi::cl_mem_flags,
    image_format: *const ffi::cl_image_format,
    image_desc: *const ffi::cl_image_desc,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_mem {
    panic!("clCreateImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainMemObject(memobj: ffi::cl_mem) -> ffi::cl_int {
    panic!("clRetainMemObject is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseMemObject(memobj: ffi::cl_mem) -> ffi::cl_int {
    panic!("clReleaseMemObject is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetSupportedImageFormats(
    context: ffi::cl_context,
    flags: ffi::cl_mem_flags,
    image_type: ffi::cl_mem_object_type,
    num_entries: ffi::cl_uint,
    image_formats: *mut ffi::cl_image_format,
    num_image_formats: *mut ffi::cl_uint,
) -> ffi::cl_int {
    panic!("clGetSupportedImageFormats is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetMemObjectInfo(
    memobj: ffi::cl_mem,
    param_name: ffi::cl_mem_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetMemObjectInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetImageInfo(
    image: ffi::cl_mem,
    param_name: ffi::cl_image_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetImageInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetMemObjectDestructorCallback(
    memobj: ffi::cl_mem,
    pfn_notify: Option<unsafe extern "C" fn(ffi::cl_mem, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> ffi::cl_int {
    panic!("clSetMemObjectDestructorCallback is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainSampler(sampler: ffi::cl_sampler) -> ffi::cl_int {
    panic!("clRetainSampler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseSampler(sampler: ffi::cl_sampler) -> ffi::cl_int {
    panic!("clReleaseSampler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetSamplerInfo(
    sampler: ffi::cl_sampler,
    param_name: ffi::cl_sampler_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetSamplerInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithSource(
    context: ffi::cl_context,
    count: ffi::cl_uint,
    strings: *mut *const core::ffi::c_char,
    lengths: *const usize,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_program {
    panic!("clCreateProgramWithSource is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithBinary(
    context: ffi::cl_context,
    num_devices: ffi::cl_uint,
    device_list: *const ffi::cl_device_id,
    lengths: *const usize,
    binaries: *mut *const core::ffi::c_uchar,
    binary_status: *mut ffi::cl_int,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_program {
    panic!("clCreateProgramWithBinary is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithBuiltInKernels(
    context: ffi::cl_context,
    num_devices: ffi::cl_uint,
    device_list: *const ffi::cl_device_id,
    kernel_names: *const core::ffi::c_char,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_program {
    panic!("clCreateProgramWithBuiltInKernels is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainProgram(program: ffi::cl_program) -> ffi::cl_int {
    panic!("clRetainProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseProgram(program: ffi::cl_program) -> ffi::cl_int {
    panic!("clReleaseProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clBuildProgram(
    program: ffi::cl_program,
    num_devices: ffi::cl_uint,
    device_list: *const ffi::cl_device_id,
    options: *const core::ffi::c_char,
    pfn_notify: Option<unsafe extern "C" fn(ffi::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> ffi::cl_int {
    panic!("clBuildProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCompileProgram(
    program: ffi::cl_program,
    num_devices: ffi::cl_uint,
    device_list: *const ffi::cl_device_id,
    options: *const core::ffi::c_char,
    num_input_headers: ffi::cl_uint,
    input_headers: *const ffi::cl_program,
    header_include_names: *mut *const core::ffi::c_char,
    pfn_notify: Option<unsafe extern "C" fn(ffi::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> ffi::cl_int {
    panic!("clCompileProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clLinkProgram(
    context: ffi::cl_context,
    num_devices: ffi::cl_uint,
    device_list: *const ffi::cl_device_id,
    options: *const core::ffi::c_char,
    num_input_programs: ffi::cl_uint,
    input_programs: *const ffi::cl_program,
    pfn_notify: Option<unsafe extern "C" fn(ffi::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_program {
    panic!("clLinkProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clUnloadPlatformCompiler(platform: ffi::cl_platform_id) -> ffi::cl_int {
    panic!("clUnloadPlatformCompiler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetProgramInfo(
    program: ffi::cl_program,
    param_name: ffi::cl_program_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetProgramInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetProgramBuildInfo(
    program: ffi::cl_program,
    device: ffi::cl_device_id,
    param_name: ffi::cl_program_build_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetProgramBuildInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateKernel(
    program: ffi::cl_program,
    kernel_name: *const core::ffi::c_char,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_kernel {
    panic!("clCreateKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateKernelsInProgram(
    program: ffi::cl_program,
    num_kernels: ffi::cl_uint,
    kernels: *mut ffi::cl_kernel,
    num_kernels_ret: *mut ffi::cl_uint,
) -> ffi::cl_int {
    panic!("clCreateKernelsInProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainKernel(kernel: ffi::cl_kernel) -> ffi::cl_int {
    panic!("clRetainKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseKernel(kernel: ffi::cl_kernel) -> ffi::cl_int {
    panic!("clReleaseKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetKernelArg(
    kernel: ffi::cl_kernel,
    arg_index: ffi::cl_uint,
    arg_size: usize,
    arg_value: *const core::ffi::c_void,
) -> ffi::cl_int {
    panic!("clSetKernelArg is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelInfo(
    kernel: ffi::cl_kernel,
    param_name: ffi::cl_kernel_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetKernelInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelArgInfo(
    kernel: ffi::cl_kernel,
    arg_indx: ffi::cl_uint,
    param_name: ffi::cl_kernel_arg_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetKernelArgInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelWorkGroupInfo(
    kernel: ffi::cl_kernel,
    device: ffi::cl_device_id,
    param_name: ffi::cl_kernel_work_group_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetKernelWorkGroupInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clWaitForEvents(
    num_events: ffi::cl_uint,
    event_list: *const ffi::cl_event,
) -> ffi::cl_int {
    panic!("clWaitForEvents is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetEventInfo(
    event: ffi::cl_event,
    param_name: ffi::cl_event_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetEventInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateUserEvent(
    context: ffi::cl_context,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_event {
    panic!("clCreateUserEvent is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainEvent(event: ffi::cl_event) -> ffi::cl_int {
    panic!("clRetainEvent is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseEvent(event: ffi::cl_event) -> ffi::cl_int {
    panic!("clReleaseEvent is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetUserEventStatus(
    event: ffi::cl_event,
    execution_status: ffi::cl_int,
) -> ffi::cl_int {
    panic!("clSetUserEventStatus is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetEventCallback(
    event: ffi::cl_event,
    command_exec_callback_type: ffi::cl_int,
    pfn_notify: Option<unsafe extern "C" fn(ffi::cl_event, ffi::cl_int, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> ffi::cl_int {
    panic!("clSetEventCallback is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetEventProfilingInfo(
    event: ffi::cl_event,
    param_name: ffi::cl_profiling_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> ffi::cl_int {
    panic!("clGetEventProfilingInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clFlush(command_queue: ffi::cl_command_queue) -> ffi::cl_int {
    panic!("clFlush is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clFinish(command_queue: ffi::cl_command_queue) -> ffi::cl_int {
    panic!("clFinish is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueReadBuffer(
    command_queue: ffi::cl_command_queue,
    buffer: ffi::cl_mem,
    blocking_read: ffi::cl_bool,
    offset: usize,
    size: usize,
    ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueReadBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueReadBufferRect(
    command_queue: ffi::cl_command_queue,
    buffer: ffi::cl_mem,
    blocking_read: ffi::cl_bool,
    buffer_origin: *const usize,
    host_origin: *const usize,
    region: *const usize,
    buffer_row_pitch: usize,
    buffer_slice_pitch: usize,
    host_row_pitch: usize,
    host_slice_pitch: usize,
    ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueReadBufferRect is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWriteBuffer(
    command_queue: ffi::cl_command_queue,
    buffer: ffi::cl_mem,
    blocking_write: ffi::cl_bool,
    offset: usize,
    size: usize,
    ptr: *const core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueWriteBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWriteBufferRect(
    command_queue: ffi::cl_command_queue,
    buffer: ffi::cl_mem,
    blocking_write: ffi::cl_bool,
    buffer_origin: *const usize,
    host_origin: *const usize,
    region: *const usize,
    buffer_row_pitch: usize,
    buffer_slice_pitch: usize,
    host_row_pitch: usize,
    host_slice_pitch: usize,
    ptr: *const core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueWriteBufferRect is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueFillBuffer(
    command_queue: ffi::cl_command_queue,
    buffer: ffi::cl_mem,
    pattern: *const core::ffi::c_void,
    pattern_size: usize,
    offset: usize,
    size: usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueFillBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyBuffer(
    command_queue: ffi::cl_command_queue,
    src_buffer: ffi::cl_mem,
    dst_buffer: ffi::cl_mem,
    src_offset: usize,
    dst_offset: usize,
    size: usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueCopyBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyBufferRect(
    command_queue: ffi::cl_command_queue,
    src_buffer: ffi::cl_mem,
    dst_buffer: ffi::cl_mem,
    src_origin: *const usize,
    dst_origin: *const usize,
    region: *const usize,
    src_row_pitch: usize,
    src_slice_pitch: usize,
    dst_row_pitch: usize,
    dst_slice_pitch: usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueCopyBufferRect is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueReadImage(
    command_queue: ffi::cl_command_queue,
    image: ffi::cl_mem,
    blocking_read: ffi::cl_bool,
    origin: *const usize,
    region: *const usize,
    row_pitch: usize,
    slice_pitch: usize,
    ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueReadImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWriteImage(
    command_queue: ffi::cl_command_queue,
    image: ffi::cl_mem,
    blocking_write: ffi::cl_bool,
    origin: *const usize,
    region: *const usize,
    input_row_pitch: usize,
    input_slice_pitch: usize,
    ptr: *const core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueWriteImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueFillImage(
    command_queue: ffi::cl_command_queue,
    image: ffi::cl_mem,
    fill_color: *const core::ffi::c_void,
    origin: *const usize,
    region: *const usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueFillImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyImage(
    command_queue: ffi::cl_command_queue,
    src_image: ffi::cl_mem,
    dst_image: ffi::cl_mem,
    src_origin: *const usize,
    dst_origin: *const usize,
    region: *const usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueCopyImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyImageToBuffer(
    command_queue: ffi::cl_command_queue,
    src_image: ffi::cl_mem,
    dst_buffer: ffi::cl_mem,
    src_origin: *const usize,
    region: *const usize,
    dst_offset: usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueCopyImageToBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyBufferToImage(
    command_queue: ffi::cl_command_queue,
    src_buffer: ffi::cl_mem,
    dst_image: ffi::cl_mem,
    src_offset: usize,
    dst_origin: *const usize,
    region: *const usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueCopyBufferToImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMapBuffer(
    command_queue: ffi::cl_command_queue,
    buffer: ffi::cl_mem,
    blocking_map: ffi::cl_bool,
    map_flags: ffi::cl_map_flags,
    offset: usize,
    size: usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
    errcode_ret: *mut ffi::cl_int,
) -> *mut core::ffi::c_void {
    panic!("clEnqueueMapBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMapImage(
    command_queue: ffi::cl_command_queue,
    image: ffi::cl_mem,
    blocking_map: ffi::cl_bool,
    map_flags: ffi::cl_map_flags,
    origin: *const usize,
    region: *const usize,
    image_row_pitch: *mut usize,
    image_slice_pitch: *mut usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
    errcode_ret: *mut ffi::cl_int,
) -> *mut core::ffi::c_void {
    panic!("clEnqueueMapImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueUnmapMemObject(
    command_queue: ffi::cl_command_queue,
    memobj: ffi::cl_mem,
    mapped_ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueUnmapMemObject is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMigrateMemObjects(
    command_queue: ffi::cl_command_queue,
    num_mem_objects: ffi::cl_uint,
    mem_objects: *const ffi::cl_mem,
    flags: ffi::cl_mem_migration_flags,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueMigrateMemObjects is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueNDRangeKernel(
    command_queue: ffi::cl_command_queue,
    kernel: ffi::cl_kernel,
    work_dim: ffi::cl_uint,
    global_work_offset: *const usize,
    global_work_size: *const usize,
    local_work_size: *const usize,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueNDRangeKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueNativeKernel(
    command_queue: ffi::cl_command_queue,
    user_func: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
    args: *mut core::ffi::c_void,
    cb_args: usize,
    num_mem_objects: ffi::cl_uint,
    mem_list: *const ffi::cl_mem,
    args_mem_loc: *mut *const core::ffi::c_void,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueNativeKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMarkerWithWaitList(
    command_queue: ffi::cl_command_queue,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueMarkerWithWaitList is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueBarrierWithWaitList(
    command_queue: ffi::cl_command_queue,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueBarrierWithWaitList is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetExtensionFunctionAddressForPlatform(
    platform: ffi::cl_platform_id,
    func_name: *const core::ffi::c_char,
) -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateImage2D(
    context: ffi::cl_context,
    flags: ffi::cl_mem_flags,
    image_format: *const ffi::cl_image_format,
    image_width: usize,
    image_height: usize,
    image_row_pitch: usize,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_mem {
    panic!("clCreateImage2D is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateImage3D(
    context: ffi::cl_context,
    flags: ffi::cl_mem_flags,
    image_format: *const ffi::cl_image_format,
    image_width: usize,
    image_height: usize,
    image_depth: usize,
    image_row_pitch: usize,
    image_slice_pitch: usize,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_mem {
    panic!("clCreateImage3D is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMarker(
    command_queue: ffi::cl_command_queue,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueMarker is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWaitForEvents(
    command_queue: ffi::cl_command_queue,
    num_events: ffi::cl_uint,
    event_list: *const ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueWaitForEvents is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueBarrier(command_queue: ffi::cl_command_queue) -> ffi::cl_int {
    panic!("clEnqueueBarrier is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clUnloadCompiler() -> ffi::cl_int {
    panic!("clUnloadCompiler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetExtensionFunctionAddress(
    func_name: *const core::ffi::c_char,
) -> *mut core::ffi::c_void {
    unsafe {
        clGetExtensionFunctionAddressForPlatform(device::VoddDevice::platform_id(), func_name)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateCommandQueue(
    context: ffi::cl_context,
    device: ffi::cl_device_id,
    properties: ffi::cl_command_queue_properties,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_command_queue {
    panic!("clCreateCommandQueue is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSampler(
    context: ffi::cl_context,
    normalized_coords: ffi::cl_bool,
    addressing_mode: ffi::cl_addressing_mode,
    filter_mode: ffi::cl_filter_mode,
    errcode_ret: *mut ffi::cl_int,
) -> ffi::cl_sampler {
    panic!("clCreateSampler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueTask(
    command_queue: ffi::cl_command_queue,
    kernel: ffi::cl_kernel,
    num_events_in_wait_list: ffi::cl_uint,
    event_wait_list: *const ffi::cl_event,
    event: *mut ffi::cl_event,
) -> ffi::cl_int {
    panic!("clEnqueueTask is not implemented");
}
