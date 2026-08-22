#![allow(unused_variables)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

pub mod bitcode;
pub mod consts;
pub mod ffi;
pub mod interpreter;
pub mod parser;
pub mod platform;
pub mod sys;
pub mod value;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetPlatformIDs(
    num_entries: sys::cl_uint,
    platforms: *mut sys::cl_platform_id,
    num_platforms: *mut sys::cl_uint,
) -> sys::cl_int {
    if platforms.is_null() && num_platforms.is_null() {
        return consts::CL_INVALID_VALUE;
    }

    if !platforms.is_null() && num_entries == 0 {
        return consts::CL_INVALID_VALUE;
    }

    if !platforms.is_null() {
        unsafe {
            *platforms = platform::VoddPlatform::platform_id();
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
    platform: sys::cl_platform_id,
    param_name: sys::cl_platform_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    if !platform.is_null() && !platform::VoddPlatform::is_platform_id(platform) {
        return consts::CL_INVALID_PLATFORM;
    }

    let Some(text) = platform::VoddPlatform::platform_info(param_name) else {
        return consts::CL_INVALID_VALUE;
    };

    unsafe { ffi::info_bytes(text, param_value_size, param_value, param_value_size_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetDeviceIDs(
    platform: sys::cl_platform_id,
    device_type: sys::cl_device_type,
    num_entries: sys::cl_uint,
    devices: *mut sys::cl_device_id,
    num_devices: *mut sys::cl_uint,
) -> sys::cl_int {
    if !platform.is_null() && !platform::VoddPlatform::is_platform_id(platform) {
        return consts::CL_INVALID_PLATFORM;
    }

    if devices.is_null() && num_devices.is_null() {
        return consts::CL_INVALID_VALUE;
    }

    if !devices.is_null() && num_entries == 0 {
        return consts::CL_INVALID_VALUE;
    }

    if !platform::VoddPlatform::is_valid_device_type(device_type) {
        return consts::CL_INVALID_DEVICE_TYPE;
    }

    if !devices.is_null() {
        unsafe {
            *devices = platform::VoddPlatform::device_id();
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
    device: sys::cl_device_id,
    param_name: sys::cl_device_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    if !platform::VoddPlatform::is_device_id(device) {
        return consts::CL_INVALID_DEVICE;
    }

    let Some(value) = platform::VoddPlatform::device_info(param_name) else {
        return consts::CL_INVALID_VALUE;
    };

    unsafe { ffi::info_value(value, param_value_size, param_value, param_value_size_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSubDevices(
    in_device: sys::cl_device_id,
    properties: *const sys::cl_device_partition_property,
    num_devices: sys::cl_uint,
    out_devices: *mut sys::cl_device_id,
    num_devices_ret: *mut sys::cl_uint,
) -> sys::cl_int {
    if !platform::VoddPlatform::is_device_id(in_device) {
        return consts::CL_INVALID_DEVICE;
    }

    consts::CL_INVALID_VALUE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainDevice(device: sys::cl_device_id) -> sys::cl_int {
    if !platform::VoddPlatform::is_device_id(device) {
        return consts::CL_INVALID_DEVICE;
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseDevice(device: sys::cl_device_id) -> sys::cl_int {
    if !platform::VoddPlatform::is_device_id(device) {
        return consts::CL_INVALID_DEVICE;
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateContext(
    properties: *const sys::cl_context_properties,
    num_devices: sys::cl_uint,
    devices: *const sys::cl_device_id,
    pfn_notify: Option<sys::cl_context_callback>,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_context {
    if devices.is_null() || num_devices == 0 {
        return unsafe { ffi::context_error(consts::CL_INVALID_VALUE, errcode_ret) };
    }

    let requested = unsafe { core::slice::from_raw_parts(devices, num_devices as usize) };

    let selected = match platform::VoddContext::select_devices(requested) {
        Ok(selected) => selected,
        Err(error) => return unsafe { ffi::context_error(error, errcode_ret) },
    };

    unsafe { ffi::create_context(properties, selected, pfn_notify, user_data, errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateContextFromType(
    properties: *const sys::cl_context_properties,
    device_type: sys::cl_device_type,
    pfn_notify: Option<sys::cl_context_callback>,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_context {
    if !platform::VoddPlatform::is_valid_device_type(device_type) {
        return unsafe { ffi::context_error(consts::CL_INVALID_DEVICE_TYPE, errcode_ret) };
    }

    let selected = platform::VoddContext::devices_of_type(device_type);
    if selected.is_empty() {
        return unsafe { ffi::context_error(consts::CL_DEVICE_NOT_FOUND, errcode_ret) };
    }

    unsafe { ffi::create_context(properties, selected, pfn_notify, user_data, errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainContext(context: sys::cl_context) -> sys::cl_int {
    if !platform::VoddContext::retain(ffi::object_id(context)) {
        return consts::CL_INVALID_CONTEXT;
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseContext(context: sys::cl_context) -> sys::cl_int {
    if platform::VoddContext::release(ffi::object_id(context)).is_none() {
        return consts::CL_INVALID_CONTEXT;
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetContextInfo(
    context: sys::cl_context,
    param_name: sys::cl_context_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    let queried = platform::VoddContext::with(ffi::object_id(context), |context| {
        let value = context.info(param_name)?;

        Some(unsafe { ffi::info_value(value, param_value_size, param_value, param_value_size_ret) })
    });

    match queried {
        None => consts::CL_INVALID_CONTEXT,
        Some(None) => consts::CL_INVALID_VALUE,
        Some(Some(error)) => error,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainCommandQueue(command_queue: sys::cl_command_queue) -> sys::cl_int {
    if !platform::VoddCommandQueue::retain(ffi::object_id(command_queue)) {
        return consts::CL_INVALID_COMMAND_QUEUE;
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseCommandQueue(
    command_queue: sys::cl_command_queue,
) -> sys::cl_int {
    let Some(released) = platform::VoddCommandQueue::release(ffi::object_id(command_queue)) else {
        return consts::CL_INVALID_COMMAND_QUEUE;
    };

    if let Some(context) = released {
        platform::VoddContext::release(ffi::object_id(context));
    }

    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetCommandQueueInfo(
    command_queue: sys::cl_command_queue,
    param_name: sys::cl_command_queue_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    let queried = platform::VoddCommandQueue::with(ffi::object_id(command_queue), |queue| {
        let value = queue.info(param_name)?;

        Some(unsafe { ffi::info_value(value, param_value_size, param_value, param_value_size_ret) })
    });

    match queried {
        None => consts::CL_INVALID_COMMAND_QUEUE,
        Some(None) => consts::CL_INVALID_VALUE,
        Some(Some(error)) => error,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateBuffer(
    context: sys::cl_context,
    flags: sys::cl_mem_flags,
    size: usize,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    panic!("clCreateBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSubBuffer(
    buffer: sys::cl_mem,
    flags: sys::cl_mem_flags,
    buffer_create_type: sys::cl_buffer_create_type,
    buffer_create_info: *const core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    panic!("clCreateSubBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateImage(
    context: sys::cl_context,
    flags: sys::cl_mem_flags,
    image_format: *const sys::cl_image_format,
    image_desc: *const sys::cl_image_desc,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    panic!("clCreateImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainMemObject(memobj: sys::cl_mem) -> sys::cl_int {
    panic!("clRetainMemObject is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseMemObject(memobj: sys::cl_mem) -> sys::cl_int {
    panic!("clReleaseMemObject is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetSupportedImageFormats(
    context: sys::cl_context,
    flags: sys::cl_mem_flags,
    image_type: sys::cl_mem_object_type,
    num_entries: sys::cl_uint,
    image_formats: *mut sys::cl_image_format,
    num_image_formats: *mut sys::cl_uint,
) -> sys::cl_int {
    panic!("clGetSupportedImageFormats is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetMemObjectInfo(
    memobj: sys::cl_mem,
    param_name: sys::cl_mem_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetMemObjectInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetImageInfo(
    image: sys::cl_mem,
    param_name: sys::cl_image_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetImageInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetMemObjectDestructorCallback(
    memobj: sys::cl_mem,
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_mem, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    panic!("clSetMemObjectDestructorCallback is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainSampler(sampler: sys::cl_sampler) -> sys::cl_int {
    panic!("clRetainSampler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseSampler(sampler: sys::cl_sampler) -> sys::cl_int {
    panic!("clReleaseSampler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetSamplerInfo(
    sampler: sys::cl_sampler,
    param_name: sys::cl_sampler_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetSamplerInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithSource(
    context: sys::cl_context,
    count: sys::cl_uint,
    strings: *mut *const core::ffi::c_char,
    lengths: *const usize,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    panic!("clCreateProgramWithSource is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithBinary(
    context: sys::cl_context,
    num_devices: sys::cl_uint,
    device_list: *const sys::cl_device_id,
    lengths: *const usize,
    binaries: *mut *const core::ffi::c_uchar,
    binary_status: *mut sys::cl_int,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    panic!("clCreateProgramWithBinary is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithBuiltInKernels(
    context: sys::cl_context,
    num_devices: sys::cl_uint,
    device_list: *const sys::cl_device_id,
    kernel_names: *const core::ffi::c_char,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    panic!("clCreateProgramWithBuiltInKernels is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainProgram(program: sys::cl_program) -> sys::cl_int {
    panic!("clRetainProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseProgram(program: sys::cl_program) -> sys::cl_int {
    panic!("clReleaseProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clBuildProgram(
    program: sys::cl_program,
    num_devices: sys::cl_uint,
    device_list: *const sys::cl_device_id,
    options: *const core::ffi::c_char,
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    panic!("clBuildProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCompileProgram(
    program: sys::cl_program,
    num_devices: sys::cl_uint,
    device_list: *const sys::cl_device_id,
    options: *const core::ffi::c_char,
    num_input_headers: sys::cl_uint,
    input_headers: *const sys::cl_program,
    header_include_names: *mut *const core::ffi::c_char,
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    panic!("clCompileProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clLinkProgram(
    context: sys::cl_context,
    num_devices: sys::cl_uint,
    device_list: *const sys::cl_device_id,
    options: *const core::ffi::c_char,
    num_input_programs: sys::cl_uint,
    input_programs: *const sys::cl_program,
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    panic!("clLinkProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clUnloadPlatformCompiler(platform: sys::cl_platform_id) -> sys::cl_int {
    panic!("clUnloadPlatformCompiler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetProgramInfo(
    program: sys::cl_program,
    param_name: sys::cl_program_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetProgramInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetProgramBuildInfo(
    program: sys::cl_program,
    device: sys::cl_device_id,
    param_name: sys::cl_program_build_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetProgramBuildInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateKernel(
    program: sys::cl_program,
    kernel_name: *const core::ffi::c_char,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_kernel {
    panic!("clCreateKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateKernelsInProgram(
    program: sys::cl_program,
    num_kernels: sys::cl_uint,
    kernels: *mut sys::cl_kernel,
    num_kernels_ret: *mut sys::cl_uint,
) -> sys::cl_int {
    panic!("clCreateKernelsInProgram is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainKernel(kernel: sys::cl_kernel) -> sys::cl_int {
    panic!("clRetainKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseKernel(kernel: sys::cl_kernel) -> sys::cl_int {
    panic!("clReleaseKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetKernelArg(
    kernel: sys::cl_kernel,
    arg_index: sys::cl_uint,
    arg_size: usize,
    arg_value: *const core::ffi::c_void,
) -> sys::cl_int {
    panic!("clSetKernelArg is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelInfo(
    kernel: sys::cl_kernel,
    param_name: sys::cl_kernel_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetKernelInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelArgInfo(
    kernel: sys::cl_kernel,
    arg_indx: sys::cl_uint,
    param_name: sys::cl_kernel_arg_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetKernelArgInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelWorkGroupInfo(
    kernel: sys::cl_kernel,
    device: sys::cl_device_id,
    param_name: sys::cl_kernel_work_group_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetKernelWorkGroupInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clWaitForEvents(
    num_events: sys::cl_uint,
    event_list: *const sys::cl_event,
) -> sys::cl_int {
    panic!("clWaitForEvents is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetEventInfo(
    event: sys::cl_event,
    param_name: sys::cl_event_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetEventInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateUserEvent(
    context: sys::cl_context,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_event {
    panic!("clCreateUserEvent is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainEvent(event: sys::cl_event) -> sys::cl_int {
    panic!("clRetainEvent is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseEvent(event: sys::cl_event) -> sys::cl_int {
    panic!("clReleaseEvent is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetUserEventStatus(
    event: sys::cl_event,
    execution_status: sys::cl_int,
) -> sys::cl_int {
    panic!("clSetUserEventStatus is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetEventCallback(
    event: sys::cl_event,
    command_exec_callback_type: sys::cl_int,
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_event, sys::cl_int, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    panic!("clSetEventCallback is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetEventProfilingInfo(
    event: sys::cl_event,
    param_name: sys::cl_profiling_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    panic!("clGetEventProfilingInfo is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clFlush(command_queue: sys::cl_command_queue) -> sys::cl_int {
    panic!("clFlush is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clFinish(command_queue: sys::cl_command_queue) -> sys::cl_int {
    panic!("clFinish is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueReadBuffer(
    command_queue: sys::cl_command_queue,
    buffer: sys::cl_mem,
    blocking_read: sys::cl_bool,
    offset: usize,
    size: usize,
    ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueReadBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueReadBufferRect(
    command_queue: sys::cl_command_queue,
    buffer: sys::cl_mem,
    blocking_read: sys::cl_bool,
    buffer_origin: *const usize,
    host_origin: *const usize,
    region: *const usize,
    buffer_row_pitch: usize,
    buffer_slice_pitch: usize,
    host_row_pitch: usize,
    host_slice_pitch: usize,
    ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueReadBufferRect is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWriteBuffer(
    command_queue: sys::cl_command_queue,
    buffer: sys::cl_mem,
    blocking_write: sys::cl_bool,
    offset: usize,
    size: usize,
    ptr: *const core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueWriteBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWriteBufferRect(
    command_queue: sys::cl_command_queue,
    buffer: sys::cl_mem,
    blocking_write: sys::cl_bool,
    buffer_origin: *const usize,
    host_origin: *const usize,
    region: *const usize,
    buffer_row_pitch: usize,
    buffer_slice_pitch: usize,
    host_row_pitch: usize,
    host_slice_pitch: usize,
    ptr: *const core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueWriteBufferRect is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueFillBuffer(
    command_queue: sys::cl_command_queue,
    buffer: sys::cl_mem,
    pattern: *const core::ffi::c_void,
    pattern_size: usize,
    offset: usize,
    size: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueFillBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyBuffer(
    command_queue: sys::cl_command_queue,
    src_buffer: sys::cl_mem,
    dst_buffer: sys::cl_mem,
    src_offset: usize,
    dst_offset: usize,
    size: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueCopyBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyBufferRect(
    command_queue: sys::cl_command_queue,
    src_buffer: sys::cl_mem,
    dst_buffer: sys::cl_mem,
    src_origin: *const usize,
    dst_origin: *const usize,
    region: *const usize,
    src_row_pitch: usize,
    src_slice_pitch: usize,
    dst_row_pitch: usize,
    dst_slice_pitch: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueCopyBufferRect is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueReadImage(
    command_queue: sys::cl_command_queue,
    image: sys::cl_mem,
    blocking_read: sys::cl_bool,
    origin: *const usize,
    region: *const usize,
    row_pitch: usize,
    slice_pitch: usize,
    ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueReadImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWriteImage(
    command_queue: sys::cl_command_queue,
    image: sys::cl_mem,
    blocking_write: sys::cl_bool,
    origin: *const usize,
    region: *const usize,
    input_row_pitch: usize,
    input_slice_pitch: usize,
    ptr: *const core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueWriteImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueFillImage(
    command_queue: sys::cl_command_queue,
    image: sys::cl_mem,
    fill_color: *const core::ffi::c_void,
    origin: *const usize,
    region: *const usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueFillImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyImage(
    command_queue: sys::cl_command_queue,
    src_image: sys::cl_mem,
    dst_image: sys::cl_mem,
    src_origin: *const usize,
    dst_origin: *const usize,
    region: *const usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueCopyImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyImageToBuffer(
    command_queue: sys::cl_command_queue,
    src_image: sys::cl_mem,
    dst_buffer: sys::cl_mem,
    src_origin: *const usize,
    region: *const usize,
    dst_offset: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueCopyImageToBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueCopyBufferToImage(
    command_queue: sys::cl_command_queue,
    src_buffer: sys::cl_mem,
    dst_image: sys::cl_mem,
    src_offset: usize,
    dst_origin: *const usize,
    region: *const usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueCopyBufferToImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMapBuffer(
    command_queue: sys::cl_command_queue,
    buffer: sys::cl_mem,
    blocking_map: sys::cl_bool,
    map_flags: sys::cl_map_flags,
    offset: usize,
    size: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
    errcode_ret: *mut sys::cl_int,
) -> *mut core::ffi::c_void {
    panic!("clEnqueueMapBuffer is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMapImage(
    command_queue: sys::cl_command_queue,
    image: sys::cl_mem,
    blocking_map: sys::cl_bool,
    map_flags: sys::cl_map_flags,
    origin: *const usize,
    region: *const usize,
    image_row_pitch: *mut usize,
    image_slice_pitch: *mut usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
    errcode_ret: *mut sys::cl_int,
) -> *mut core::ffi::c_void {
    panic!("clEnqueueMapImage is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueUnmapMemObject(
    command_queue: sys::cl_command_queue,
    memobj: sys::cl_mem,
    mapped_ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueUnmapMemObject is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMigrateMemObjects(
    command_queue: sys::cl_command_queue,
    num_mem_objects: sys::cl_uint,
    mem_objects: *const sys::cl_mem,
    flags: sys::cl_mem_migration_flags,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueMigrateMemObjects is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueNDRangeKernel(
    command_queue: sys::cl_command_queue,
    kernel: sys::cl_kernel,
    work_dim: sys::cl_uint,
    global_work_offset: *const usize,
    global_work_size: *const usize,
    local_work_size: *const usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueNDRangeKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueNativeKernel(
    command_queue: sys::cl_command_queue,
    user_func: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
    args: *mut core::ffi::c_void,
    cb_args: usize,
    num_mem_objects: sys::cl_uint,
    mem_list: *const sys::cl_mem,
    args_mem_loc: *mut *const core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueNativeKernel is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMarkerWithWaitList(
    command_queue: sys::cl_command_queue,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueMarkerWithWaitList is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueBarrierWithWaitList(
    command_queue: sys::cl_command_queue,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueBarrierWithWaitList is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetExtensionFunctionAddressForPlatform(
    _platform: sys::cl_platform_id,
    _func_name: *const core::ffi::c_char,
) -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateImage2D(
    context: sys::cl_context,
    flags: sys::cl_mem_flags,
    image_format: *const sys::cl_image_format,
    image_width: usize,
    image_height: usize,
    image_row_pitch: usize,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    panic!("clCreateImage2D is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateImage3D(
    context: sys::cl_context,
    flags: sys::cl_mem_flags,
    image_format: *const sys::cl_image_format,
    image_width: usize,
    image_height: usize,
    image_depth: usize,
    image_row_pitch: usize,
    image_slice_pitch: usize,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    panic!("clCreateImage3D is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMarker(
    command_queue: sys::cl_command_queue,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueMarker is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWaitForEvents(
    command_queue: sys::cl_command_queue,
    num_events: sys::cl_uint,
    event_list: *const sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueWaitForEvents is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueBarrier(command_queue: sys::cl_command_queue) -> sys::cl_int {
    panic!("clEnqueueBarrier is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clUnloadCompiler() -> sys::cl_int {
    panic!("clUnloadCompiler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetExtensionFunctionAddress(
    func_name: *const core::ffi::c_char,
) -> *mut core::ffi::c_void {
    unsafe {
        clGetExtensionFunctionAddressForPlatform(platform::VoddPlatform::platform_id(), func_name)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateCommandQueue(
    context: sys::cl_context,
    device: sys::cl_device_id,
    properties: sys::cl_command_queue_properties,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_command_queue {
    let Some(associated) =
        platform::VoddContext::with(ffi::object_id(context), |owner| owner.has_device(device))
    else {
        return unsafe { ffi::queue_error(consts::CL_INVALID_CONTEXT, errcode_ret) };
    };

    if !associated {
        return unsafe { ffi::queue_error(consts::CL_INVALID_DEVICE, errcode_ret) };
    }

    if let Err(error) = platform::VoddCommandQueue::validate_properties(properties) {
        return unsafe { ffi::queue_error(error, errcode_ret) };
    }

    platform::VoddContext::retain(ffi::object_id(context));

    let id = platform::VoddCommandQueue::create(context, device, properties);

    unsafe { ffi::info_write(errcode_ret, consts::CL_SUCCESS) };
    ffi::object_handle(id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSampler(
    context: sys::cl_context,
    normalized_coords: sys::cl_bool,
    addressing_mode: sys::cl_addressing_mode,
    filter_mode: sys::cl_filter_mode,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_sampler {
    panic!("clCreateSampler is not implemented");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueTask(
    command_queue: sys::cl_command_queue,
    kernel: sys::cl_kernel,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    panic!("clEnqueueTask is not implemented");
}
