pub type cl_char = i8;
pub type cl_uchar = u8;
pub type cl_short = i16;
pub type cl_ushort = u16;
pub type cl_int = i32;
pub type cl_uint = u32;
pub type cl_long = i64;
pub type cl_ulong = u64;
pub type cl_half = u16;
pub type cl_float = f32;
pub type cl_double = f64;

pub type cl_bitfield = cl_ulong;
pub type cl_properties = cl_ulong;
pub type cl_bool = cl_uint;

pub type cl_addressing_mode = cl_uint;
pub type cl_buffer_create_type = cl_uint;
pub type cl_channel_order = cl_uint;
pub type cl_channel_type = cl_uint;
pub type cl_command_queue_info = cl_uint;
pub type cl_command_type = cl_uint;
pub type cl_context_info = cl_uint;
pub type cl_context_callback = unsafe extern "C" fn(
    errinfo: *const core::ffi::c_char,
    private_info: *const core::ffi::c_void,
    cb: usize,
    user_data: *mut core::ffi::c_void,
);
pub type cl_device_info = cl_uint;
pub type cl_device_local_mem_type = cl_uint;
pub type cl_device_mem_cache_type = cl_uint;
pub type cl_event_callback = unsafe extern "C" fn(
    event: cl_event,
    event_command_status: cl_int,
    user_data: *mut core::ffi::c_void,
);
pub type cl_event_info = cl_uint;
pub type cl_filter_mode = cl_uint;
pub type cl_image_info = cl_uint;
pub type cl_kernel_arg_access_qualifier = cl_uint;
pub type cl_kernel_arg_address_qualifier = cl_uint;
pub type cl_kernel_arg_info = cl_uint;
pub type cl_kernel_info = cl_uint;
pub type cl_kernel_work_group_info = cl_uint;
pub type cl_mem_destructor_callback =
    unsafe extern "C" fn(memobj: cl_mem, user_data: *mut core::ffi::c_void);
pub type cl_mem_info = cl_uint;
pub type cl_mem_object_type = cl_uint;
pub type cl_platform_info = cl_uint;
pub type cl_profiling_info = cl_uint;
pub type cl_program_binary_type = cl_uint;
pub type cl_program_build_info = cl_uint;
pub type cl_program_info = cl_uint;
pub type cl_sampler_info = cl_uint;
pub type cl_build_status = cl_int;

pub type cl_command_queue_properties = cl_bitfield;
pub type cl_device_affinity_domain = cl_bitfield;
pub type cl_device_exec_capabilities = cl_bitfield;
pub type cl_device_fp_config = cl_bitfield;
pub type cl_device_type = cl_bitfield;
pub type cl_kernel_arg_type_qualifier = cl_bitfield;
pub type cl_map_flags = cl_bitfield;
pub type cl_mem_flags = cl_bitfield;
pub type cl_mem_migration_flags = cl_bitfield;

pub type cl_context_properties = isize;
pub type cl_device_partition_property = isize;

#[repr(C)]
pub struct _cl_platform_id {
    _opaque: [u8; 0],
}
pub type cl_platform_id = *mut _cl_platform_id;

#[repr(C)]
pub struct _cl_device_id {
    _opaque: [u8; 0],
}
pub type cl_device_id = *mut _cl_device_id;

#[repr(C)]
pub struct _cl_context {
    _opaque: [u8; 0],
}
pub type cl_context = *mut _cl_context;

#[repr(C)]
pub struct _cl_command_queue {
    _opaque: [u8; 0],
}
pub type cl_command_queue = *mut _cl_command_queue;

#[repr(C)]
pub struct _cl_mem {
    _opaque: [u8; 0],
}
pub type cl_mem = *mut _cl_mem;

#[repr(C)]
pub struct _cl_program {
    _opaque: [u8; 0],
}
pub type cl_program = *mut _cl_program;

#[repr(C)]
pub struct _cl_kernel {
    _opaque: [u8; 0],
}
pub type cl_kernel = *mut _cl_kernel;

#[repr(C)]
pub struct _cl_event {
    _opaque: [u8; 0],
}
pub type cl_event = *mut _cl_event;

#[repr(C)]
pub struct _cl_sampler {
    _opaque: [u8; 0],
}
pub type cl_sampler = *mut _cl_sampler;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct cl_image_format {
    pub image_channel_order: cl_channel_order,
    pub image_channel_data_type: cl_channel_type,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct cl_image_desc {
    pub image_type: cl_mem_object_type,
    pub image_width: usize,
    pub image_height: usize,
    pub image_depth: usize,
    pub image_array_size: usize,
    pub image_row_pitch: usize,
    pub image_slice_pitch: usize,
    pub num_mip_levels: cl_uint,
    pub num_samples: cl_uint,
    pub buffer: cl_mem,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct cl_buffer_region {
    pub origin: usize,
    pub size: usize,
}
