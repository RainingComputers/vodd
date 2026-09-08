#![allow(unused_variables)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

pub mod address;
pub mod bitcode;
pub mod compiler;
pub mod consts;
pub mod detectors;
pub mod ffi;
pub mod interpreter;
pub mod logger;
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
    unsafe {
        ffi::write_handles(
            &[ffi::platform_handle()],
            num_entries,
            platforms,
            num_platforms,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetPlatformInfo(
    platform: sys::cl_platform_id,
    param_name: sys::cl_platform_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::platform_id(platform)
                .and_then(|()| ffi::platform_info(param_name))
                .map(platform::Platform::info),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetDeviceIDs(
    platform: sys::cl_platform_id,
    device_type: sys::cl_device_type,
    num_entries: sys::cl_uint,
    devices: *mut sys::cl_device_id,
    num_devices: *mut sys::cl_uint,
) -> sys::cl_int {
    unsafe {
        ffi::found(
            ffi::platform_id(platform)
                .and_then(|()| ffi::device_type(device_type))
                .and_then(platform::Platform::devices),
            num_entries,
            devices,
            num_devices,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetDeviceInfo(
    device: sys::cl_device_id,
    param_name: sys::cl_device_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::device(device).and_then(|device| Ok(device.info(ffi::device_info(param_name)?))),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSubDevices(
    in_device: sys::cl_device_id,
    properties: *const sys::cl_device_partition_property,
    num_devices: sys::cl_uint,
    out_devices: *mut sys::cl_device_id,
    num_devices_ret: *mut sys::cl_uint,
) -> sys::cl_int {
    ffi::status(ffi::device(in_device).and(Err(platform::Error::InvalidValue)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainDevice(device: sys::cl_device_id) -> sys::cl_int {
    ffi::status(ffi::device(device).map(|_| ()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseDevice(device: sys::cl_device_id) -> sys::cl_int {
    ffi::status(ffi::device(device).map(|_| ()))
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
    unsafe {
        ffi::object(
            (|| {
                platform::Context::create(
                    ffi::context_properties(properties)?,
                    ffi::devices(num_devices, devices)?,
                    ffi::context_notify(pfn_notify, user_data)?,
                )
                .map(platform::ContextId::into_raw)
            })(),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateContextFromType(
    properties: *const sys::cl_context_properties,
    device_type: sys::cl_device_type,
    pfn_notify: Option<sys::cl_context_callback>,
    user_data: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_context {
    unsafe {
        ffi::object(
            (|| {
                platform::Context::create(
                    ffi::context_properties(properties)?,
                    platform::Platform::devices(ffi::device_type(device_type)?)?,
                    ffi::context_notify(pfn_notify, user_data)?,
                )
                .map(platform::ContextId::into_raw)
            })(),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainContext(context: sys::cl_context) -> sys::cl_int {
    ffi::status(platform::Context::retain(ffi::context_id(context)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseContext(context: sys::cl_context) -> sys::cl_int {
    ffi::status(platform::Context::release(ffi::context_id(context)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetContextInfo(
    context: sys::cl_context,
    param_name: sys::cl_context_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::context_info(param_name)
                .and_then(|param| platform::Context::info(ffi::context_id(context), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainCommandQueue(command_queue: sys::cl_command_queue) -> sys::cl_int {
    ffi::status(platform::CommandQueue::retain(ffi::queue_id(command_queue)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseCommandQueue(
    command_queue: sys::cl_command_queue,
) -> sys::cl_int {
    ffi::status(platform::CommandQueue::release(ffi::queue_id(
        command_queue,
    )))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetCommandQueueInfo(
    command_queue: sys::cl_command_queue,
    param_name: sys::cl_command_queue_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::queue_info(param_name).and_then(|param| {
                platform::CommandQueue::info(ffi::queue_id(command_queue), param)
            }),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
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
    unsafe {
        ffi::object(
            (|| {
                platform::Buffer::create(
                    ffi::context_id(context),
                    ffi::mem_flags(flags, host_ptr)?,
                    size,
                )
                .map(platform::BufferId::into_raw)
            })(),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSubBuffer(
    buffer: sys::cl_mem,
    flags: sys::cl_mem_flags,
    buffer_create_type: sys::cl_buffer_create_type,
    buffer_create_info: *const core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    unsafe {
        ffi::object(
            (|| {
                let region = ffi::buffer_region(buffer_create_type, buffer_create_info)?;

                platform::Buffer::create_sub(
                    ffi::buffer_id(buffer),
                    ffi::mem_flags(flags, core::ptr::null_mut())?,
                    region.origin,
                    region.size,
                )
                .map(platform::BufferId::into_raw)
            })(),
            errcode_ret,
        )
    }
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
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainMemObject(memobj: sys::cl_mem) -> sys::cl_int {
    ffi::status(platform::Buffer::retain(ffi::buffer_id(memobj)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseMemObject(memobj: sys::cl_mem) -> sys::cl_int {
    ffi::status(platform::Buffer::release(ffi::buffer_id(memobj)))
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
    unsafe { ffi::write_handles(&[], num_entries, image_formats, num_image_formats) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetMemObjectInfo(
    memobj: sys::cl_mem,
    param_name: sys::cl_mem_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::mem_info(param_name)
                .and_then(|param| platform::Buffer::info(ffi::buffer_id(memobj), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetImageInfo(
    image: sys::cl_mem,
    param_name: sys::cl_image_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidMemObject))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetMemObjectDestructorCallback(
    memobj: sys::cl_mem,
    pfn_notify: Option<sys::cl_mem_destructor_callback>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    ffi::status(unsafe {
        ffi::destructor_notify(pfn_notify, user_data)
            .and_then(|notify| platform::Buffer::add_destructor(ffi::buffer_id(memobj), notify))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainSampler(sampler: sys::cl_sampler) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidSampler))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseSampler(sampler: sys::cl_sampler) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidSampler))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetSamplerInfo(
    sampler: sys::cl_sampler,
    param_name: sys::cl_sampler_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidSampler))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithSource(
    context: sys::cl_context,
    count: sys::cl_uint,
    strings: *mut *const core::ffi::c_char,
    lengths: *const usize,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    unsafe {
        ffi::object(
            (|| {
                platform::Program::create_with_source(
                    ffi::context_id(context),
                    ffi::source(count, strings, lengths)?,
                )
                .map(platform::ProgramId::into_raw)
            })(),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithBinary(
    context: sys::cl_context,
    num_devices: sys::cl_uint,
    device_list: *const sys::cl_device_id,
    lengths: *const usize,
    binaries: *mut *const sys::cl_uchar,
    binary_status: *mut sys::cl_int,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    unsafe {
        ffi::object(
            (|| {
                ffi::devices(num_devices, device_list)?;

                let created = platform::Program::create_with_binary(
                    ffi::context_id(context),
                    ffi::binary(num_devices, lengths, binaries)?,
                );

                ffi::write_code(
                    binary_status,
                    ffi::status(created.as_ref().map(|_| ()).map_err(|error| *error)),
                );

                created.map(platform::ProgramId::into_raw)
            })(),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithBuiltInKernels(
    context: sys::cl_context,
    num_devices: sys::cl_uint,
    device_list: *const sys::cl_device_id,
    kernel_names: *const core::ffi::c_char,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    unsafe { ffi::object(Err(platform::Error::InvalidValue), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainProgram(program: sys::cl_program) -> sys::cl_int {
    ffi::status(platform::Program::retain(ffi::program_id(program)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseProgram(program: sys::cl_program) -> sys::cl_int {
    ffi::status(platform::Program::release(ffi::program_id(program)))
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
    ffi::status(unsafe {
        platform::Program::build(
            ffi::program_id(program),
            ffi::options(options),
            ffi::program_notify(pfn_notify, user_data),
        )
    })
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
    ffi::status((|| unsafe {
        platform::Program::compile(
            ffi::program_id(program),
            ffi::options(options),
            ffi::headers(num_input_headers, input_headers, header_include_names)?,
            ffi::program_notify(pfn_notify, user_data),
        )
    })())
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
    unsafe {
        ffi::linked(
            ffi::programs(num_input_programs, input_programs)
                .map_err(|error| platform::LinkFailure { program: None, error })
                .and_then(|inputs| {
                    platform::Program::link(
                        ffi::context_id(context),
                        ffi::options(options),
                        inputs,
                        ffi::program_notify(pfn_notify, user_data),
                    )
                }),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clUnloadPlatformCompiler(platform: sys::cl_platform_id) -> sys::cl_int {
    ffi::status(ffi::platform_id(platform))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetProgramInfo(
    program: sys::cl_program,
    param_name: sys::cl_program_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::program_info(param_name)
                .and_then(|param| platform::Program::info(ffi::program_id(program), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
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
    unsafe {
        ffi::info(
            ffi::device(device)
                .and_then(|_| ffi::program_build_info(param_name))
                .and_then(|param| platform::Program::build_info(ffi::program_id(program), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateKernel(
    program: sys::cl_program,
    kernel_name: *const core::ffi::c_char,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_kernel {
    unsafe {
        ffi::object(
            (|| {
                if kernel_name.is_null() {
                    return Err(platform::Error::InvalidValue);
                }

                platform::Kernel::create(ffi::program_id(program), ffi::options(kernel_name))
                    .map(platform::KernelId::into_raw)
            })(),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateKernelsInProgram(
    program: sys::cl_program,
    num_kernels: sys::cl_uint,
    kernels: *mut sys::cl_kernel,
    num_kernels_ret: *mut sys::cl_uint,
) -> sys::cl_int {
    unsafe {
        ffi::created(
            platform::Kernel::create_all(ffi::program_id(program)),
            num_kernels,
            kernels,
            num_kernels_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainKernel(kernel: sys::cl_kernel) -> sys::cl_int {
    ffi::status(platform::Kernel::retain(ffi::kernel_id(kernel)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseKernel(kernel: sys::cl_kernel) -> sys::cl_int {
    ffi::status(platform::Kernel::release(ffi::kernel_id(kernel)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetKernelArg(
    kernel: sys::cl_kernel,
    arg_index: sys::cl_uint,
    arg_size: usize,
    arg_value: *const core::ffi::c_void,
) -> sys::cl_int {
    ffi::status((|| unsafe {
        platform::Kernel::set_argument(
            ffi::kernel_id(kernel),
            arg_index,
            ffi::kernel_argument(ffi::kernel_id(kernel), arg_index, arg_size, arg_value)?,
        )
    })())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelInfo(
    kernel: sys::cl_kernel,
    param_name: sys::cl_kernel_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::kernel_info(param_name)
                .and_then(|param| platform::Kernel::info(ffi::kernel_id(kernel), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
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
    ffi::status(
        platform::Kernel::exists(ffi::kernel_id(kernel))
            .and(Err(platform::Error::KernelArgInfoNotAvailable)),
    )
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
    unsafe {
        ffi::info(
            ffi::device(device)
                .and_then(|_| ffi::kernel_work_group_info(param_name))
                .and_then(|param| platform::Kernel::work_group_info(ffi::kernel_id(kernel), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clWaitForEvents(
    num_events: sys::cl_uint,
    event_list: *const sys::cl_event,
) -> sys::cl_int {
    ffi::status(unsafe {
        ffi::events(num_events, event_list).and_then(|events| platform::Event::wait(&events))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetEventInfo(
    event: sys::cl_event,
    param_name: sys::cl_event_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::event_info(param_name)
                .and_then(|param| platform::Event::info(ffi::event_id(event), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateUserEvent(
    context: sys::cl_context,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_event {
    unsafe {
        ffi::object(
            platform::Event::create_user(ffi::context_id(context)).map(platform::EventId::into_raw),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clRetainEvent(event: sys::cl_event) -> sys::cl_int {
    ffi::status(platform::Event::retain(ffi::event_id(event)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clReleaseEvent(event: sys::cl_event) -> sys::cl_int {
    ffi::status(platform::Event::release(ffi::event_id(event)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetUserEventStatus(
    event: sys::cl_event,
    execution_status: sys::cl_int,
) -> sys::cl_int {
    ffi::status(
        ffi::user_status(execution_status)
            .and_then(|status| platform::Event::set_user_status(ffi::event_id(event), status)),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetEventCallback(
    event: sys::cl_event,
    command_exec_callback_type: sys::cl_int,
    pfn_notify: Option<sys::cl_event_callback>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    ffi::status((|| unsafe {
        platform::Event::add_callback(
            ffi::event_id(event),
            ffi::callback_status(command_exec_callback_type)?,
            ffi::event_notify(pfn_notify, user_data)?,
        )
    })())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetEventProfilingInfo(
    event: sys::cl_event,
    param_name: sys::cl_profiling_info,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        ffi::info(
            ffi::profiling_info(param_name)
                .and_then(|param| platform::Event::profiling_info(ffi::event_id(event), param)),
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clFlush(command_queue: sys::cl_command_queue) -> sys::cl_int {
    ffi::status(platform::CommandQueue::flush(ffi::queue_id(command_queue)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clFinish(command_queue: sys::cl_command_queue) -> sys::cl_int {
    ffi::status(platform::CommandQueue::finish(ffi::queue_id(command_queue)))
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
    unsafe {
        ffi::enqueue(blocking_read, event, || {
            platform::CommandQueue::read_buffer(
                ffi::queue_id(command_queue),
                ffi::buffer_id(buffer),
                offset,
                size,
                ffi::shared_memory_pointer(ptr),
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(blocking_read, event, || {
            platform::CommandQueue::transfer(
                ffi::queue_id(command_queue),
                platform::Slab::rect(
                    platform::Target::Buffer(ffi::buffer_id(buffer)),
                    ffi::region(buffer_origin)?,
                    buffer_row_pitch,
                    buffer_slice_pitch,
                ),
                platform::Slab::rect(
                    platform::Target::Host(ffi::shared_memory_pointer(ptr)),
                    ffi::region(host_origin)?,
                    host_row_pitch,
                    host_slice_pitch,
                ),
                ffi::region(region)?,
                platform::CommandType::ReadBufferRect,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(blocking_write, event, || {
            platform::CommandQueue::write_buffer(
                ffi::queue_id(command_queue),
                ffi::buffer_id(buffer),
                offset,
                size,
                ffi::shared_memory_pointer(ptr),
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(blocking_write, event, || {
            platform::CommandQueue::transfer(
                ffi::queue_id(command_queue),
                platform::Slab::rect(
                    platform::Target::Host(ffi::shared_memory_pointer(ptr)),
                    ffi::region(host_origin)?,
                    host_row_pitch,
                    host_slice_pitch,
                ),
                platform::Slab::rect(
                    platform::Target::Buffer(ffi::buffer_id(buffer)),
                    ffi::region(buffer_origin)?,
                    buffer_row_pitch,
                    buffer_slice_pitch,
                ),
                ffi::region(region)?,
                platform::CommandType::WriteBufferRect,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::fill_buffer(
                ffi::queue_id(command_queue),
                ffi::buffer_id(buffer),
                offset,
                size,
                ffi::pattern(pattern, pattern_size)?,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::copy_buffer(
                ffi::queue_id(command_queue),
                ffi::buffer_id(src_buffer),
                src_offset,
                ffi::buffer_id(dst_buffer),
                dst_offset,
                size,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::transfer(
                ffi::queue_id(command_queue),
                platform::Slab::rect(
                    platform::Target::Buffer(ffi::buffer_id(src_buffer)),
                    ffi::region(src_origin)?,
                    src_row_pitch,
                    src_slice_pitch,
                ),
                platform::Slab::rect(
                    platform::Target::Buffer(ffi::buffer_id(dst_buffer)),
                    ffi::region(dst_origin)?,
                    dst_row_pitch,
                    dst_slice_pitch,
                ),
                ffi::region(region)?,
                platform::CommandType::CopyBufferRect,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    ffi::status(Err(platform::Error::InvalidOperation))
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
    ffi::status(Err(platform::Error::InvalidOperation))
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
    ffi::status(Err(platform::Error::InvalidOperation))
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
    ffi::status(Err(platform::Error::InvalidOperation))
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
    ffi::status(Err(platform::Error::InvalidOperation))
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
    ffi::status(Err(platform::Error::InvalidOperation))
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
    unsafe {
        ffi::map(blocking_map, event, errcode_ret, || {
            platform::CommandQueue::map_buffer(
                ffi::queue_id(command_queue),
                ffi::buffer_id(buffer),
                offset,
                size,
                ffi::map_flags(map_flags)?,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::map(blocking_map, event, errcode_ret, || {
            Err(platform::Error::InvalidOperation)
        })
    }
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
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::unmap(
                ffi::queue_id(command_queue),
                ffi::buffer_id(memobj),
                ffi::shared_memory_pointer(mapped_ptr),
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::migrate(
                ffi::queue_id(command_queue),
                &ffi::buffers(num_mem_objects, mem_objects)?,
                ffi::migrate_flags(flags)?,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::ndrange(
                ffi::queue_id(command_queue),
                ffi::kernel_id(kernel),
                ffi::geometry(
                    work_dim,
                    global_work_offset,
                    global_work_size,
                    local_work_size,
                )?,
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMarkerWithWaitList(
    command_queue: sys::cl_command_queue,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::marker(
                ffi::queue_id(command_queue),
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueBarrierWithWaitList(
    command_queue: sys::cl_command_queue,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::barrier(
                ffi::queue_id(command_queue),
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
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
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
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
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueMarker(
    command_queue: sys::cl_command_queue,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    unsafe { clEnqueueMarkerWithWaitList(command_queue, 0, core::ptr::null(), event) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueWaitForEvents(
    command_queue: sys::cl_command_queue,
    num_events: sys::cl_uint,
    event_list: *const sys::cl_event,
) -> sys::cl_int {
    unsafe {
        clEnqueueBarrierWithWaitList(command_queue, num_events, event_list, core::ptr::null_mut())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueBarrier(command_queue: sys::cl_command_queue) -> sys::cl_int {
    unsafe {
        clEnqueueBarrierWithWaitList(command_queue, 0, core::ptr::null(), core::ptr::null_mut())
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clUnloadCompiler() -> sys::cl_int {
    consts::CL_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetExtensionFunctionAddress(
    func_name: *const core::ffi::c_char,
) -> *mut core::ffi::c_void {
    unsafe { clGetExtensionFunctionAddressForPlatform(ffi::platform_handle(), func_name) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateCommandQueue(
    context: sys::cl_context,
    device: sys::cl_device_id,
    properties: sys::cl_command_queue_properties,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_command_queue {
    unsafe {
        ffi::object(
            (|| {
                platform::CommandQueue::create(
                    ffi::context_id(context),
                    ffi::device(device)?,
                    ffi::queue_properties(properties)?,
                )
                .map(platform::QueueId::into_raw)
            })(),
            errcode_ret,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSampler(
    context: sys::cl_context,
    normalized_coords: sys::cl_bool,
    addressing_mode: sys::cl_addressing_mode,
    filter_mode: sys::cl_filter_mode,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_sampler {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueTask(
    command_queue: sys::cl_command_queue,
    kernel: sys::cl_kernel,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    unsafe {
        ffi::enqueue(consts::CL_FALSE, event, || {
            platform::CommandQueue::task(
                ffi::queue_id(command_queue),
                ffi::kernel_id(kernel),
                ffi::event_wait_list(num_events_in_wait_list, event_wait_list)?,
            )
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithIL(
    context: sys::cl_context,
    il: *const core::ffi::c_void,
    length: usize,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateCommandQueueWithProperties(
    context: sys::cl_context,
    device: sys::cl_device_id,
    properties: *const sys::cl_properties,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_command_queue {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateBufferWithProperties(
    context: sys::cl_context,
    properties: *const sys::cl_properties,
    flags: sys::cl_mem_flags,
    size: usize,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateImageWithProperties(
    context: sys::cl_context,
    properties: *const sys::cl_properties,
    flags: sys::cl_mem_flags,
    image_format: *const sys::cl_image_format,
    image_desc: *const sys::cl_image_desc,
    host_ptr: *mut core::ffi::c_void,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateSamplerWithProperties(
    context: sys::cl_context,
    sampler_properties: *const sys::cl_properties,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_sampler {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreatePipe(
    context: sys::cl_context,
    flags: sys::cl_mem_flags,
    pipe_packet_size: sys::cl_uint,
    pipe_max_packets: sys::cl_uint,
    properties: *const sys::cl_properties,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_mem {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCloneKernel(
    source_kernel: sys::cl_kernel,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_kernel {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSVMAlloc(
    context: sys::cl_context,
    flags: sys::cl_mem_flags,
    size: usize,
    alignment: sys::cl_uint,
) -> *mut core::ffi::c_void {
    core::ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSVMFree(context: sys::cl_context, svm_pointer: *mut core::ffi::c_void) {}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetPipeInfo(
    pipe: sys::cl_mem,
    param_name: sys::cl_uint,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidMemObject))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelSubGroupInfo(
    kernel: sys::cl_kernel,
    device: sys::cl_device_id,
    param_name: sys::cl_uint,
    input_value_size: usize,
    input_value: *const core::ffi::c_void,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetKernelArgSVMPointer(
    kernel: sys::cl_kernel,
    arg_index: sys::cl_uint,
    arg_value: *const core::ffi::c_void,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetKernelExecInfo(
    kernel: sys::cl_kernel,
    param_name: sys::cl_uint,
    param_value_size: usize,
    param_value: *const core::ffi::c_void,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetDefaultDeviceCommandQueue(
    context: sys::cl_context,
    device: sys::cl_device_id,
    command_queue: sys::cl_command_queue,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetProgramReleaseCallback(
    program: sys::cl_program,
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetProgramSpecializationConstant(
    program: sys::cl_program,
    spec_id: sys::cl_uint,
    spec_size: usize,
    spec_value: *const core::ffi::c_void,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetContextDestructorCallback(
    context: sys::cl_context,
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_context, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clSetCommandQueueProperty(
    command_queue: sys::cl_command_queue,
    properties: sys::cl_command_queue_properties,
    enable: sys::cl_bool,
    old_properties: *mut sys::cl_command_queue_properties,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetDeviceAndHostTimer(
    device: sys::cl_device_id,
    device_timestamp: *mut sys::cl_ulong,
    host_timestamp: *mut sys::cl_ulong,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetHostTimer(
    device: sys::cl_device_id,
    host_timestamp: *mut sys::cl_ulong,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueSVMFree(
    command_queue: sys::cl_command_queue,
    num_svm_pointers: sys::cl_uint,
    svm_pointers: *mut *mut core::ffi::c_void,
    pfn_free_func: *mut core::ffi::c_void,
    user_data: *mut core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueSVMMemcpy(
    command_queue: sys::cl_command_queue,
    blocking_copy: sys::cl_bool,
    dst_ptr: *mut core::ffi::c_void,
    src_ptr: *const core::ffi::c_void,
    size: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueSVMMemFill(
    command_queue: sys::cl_command_queue,
    svm_ptr: *mut core::ffi::c_void,
    pattern: *const core::ffi::c_void,
    pattern_size: usize,
    size: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueSVMMap(
    command_queue: sys::cl_command_queue,
    blocking_map: sys::cl_bool,
    flags: sys::cl_map_flags,
    svm_ptr: *mut core::ffi::c_void,
    size: usize,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueSVMUnmap(
    command_queue: sys::cl_command_queue,
    svm_ptr: *mut core::ffi::c_void,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clEnqueueSVMMigrateMem(
    command_queue: sys::cl_command_queue,
    num_svm_pointers: sys::cl_uint,
    svm_pointers: *const *const core::ffi::c_void,
    sizes: *const usize,
    flags: sys::cl_mem_migration_flags,
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
    event: *mut sys::cl_event,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clGetKernelSuggestedLocalWorkSizeKHR(
    command_queue: sys::cl_command_queue,
    kernel: sys::cl_kernel,
    work_dim: sys::cl_uint,
    global_work_offset: *const usize,
    global_work_size: *const usize,
    suggested_local_work_size: *mut usize,
) -> sys::cl_int {
    ffi::status(Err(platform::Error::InvalidOperation))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateProgramWithILKHR(
    context: sys::cl_context,
    il: *const core::ffi::c_void,
    length: usize,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clCreateCommandQueueWithPropertiesKHR(
    context: sys::cl_context,
    device: sys::cl_device_id,
    properties: *const sys::cl_properties,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_command_queue {
    unsafe { ffi::object(Err(platform::Error::InvalidOperation), errcode_ret) }
}
