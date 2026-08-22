use crate::consts;
use crate::ffi;

static VODD_PLATFORM_TOKEN: u8 = 0xA0;
static VODD_DEVICE_TOKEN: u8 = 0xD0;
const VODD_DEVICE_TYPE: ffi::cl_device_type = consts::CL_DEVICE_TYPE_GPU;

pub enum InfoValue {
    Uint(ffi::cl_uint),
    Ulong(ffi::cl_ulong),
    Size(usize),
    Sizes(&'static [usize]),
    Handle(*mut core::ffi::c_void),
    Text(&'static [u8]),
    Properties(&'static [ffi::cl_device_partition_property]),
}

pub struct VoddDevice {}

impl VoddDevice {
    pub fn platform_id() -> ffi::cl_platform_id {
        &raw const VODD_PLATFORM_TOKEN as *mut ffi::_cl_platform_id
    }

    pub fn device_id() -> ffi::cl_device_id {
        &raw const VODD_DEVICE_TOKEN as *mut ffi::_cl_device_id
    }

    pub fn is_platform_id(platform: ffi::cl_platform_id) -> bool {
        platform == Self::platform_id()
    }

    pub fn is_device_id(device: ffi::cl_device_id) -> bool {
        device == Self::device_id()
    }

    pub fn is_valid_device_type(device_type: ffi::cl_device_type) -> bool {
        let known = consts::CL_DEVICE_TYPE_DEFAULT
            | consts::CL_DEVICE_TYPE_CPU
            | consts::CL_DEVICE_TYPE_GPU
            | consts::CL_DEVICE_TYPE_ACCELERATOR
            | consts::CL_DEVICE_TYPE_CUSTOM;

        device_type == consts::CL_DEVICE_TYPE_ALL || (device_type != 0 && device_type & !known == 0)
    }

    pub fn platform_info(param_name: ffi::cl_platform_info) -> Option<&'static [u8]> {
        let text: &'static [u8] = match param_name {
            consts::CL_PLATFORM_PROFILE => b"EMBEDDED_PROFILE\0",
            consts::CL_PLATFORM_VERSION => b"OpenCL 1.2 vodd\0",
            consts::CL_PLATFORM_NAME => b"vodd\0",
            consts::CL_PLATFORM_VENDOR => b"vodd\0",
            consts::CL_PLATFORM_EXTENSIONS => b"cles_khr_int64\0",
            _ => return None,
        };

        Some(text)
    }

    pub fn device_info(param_name: ffi::cl_device_info) -> Option<InfoValue> {
        let value = match param_name {
            consts::CL_DEVICE_TYPE => InfoValue::Ulong(VODD_DEVICE_TYPE),
            consts::CL_DEVICE_VENDOR_ID => InfoValue::Uint(0),
            consts::CL_DEVICE_MAX_COMPUTE_UNITS => InfoValue::Uint(1), // TODO: configurable
            consts::CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS => InfoValue::Uint(3), // TODO: configurable
            consts::CL_DEVICE_MAX_WORK_GROUP_SIZE => InfoValue::Size(256), // TODO: configurable
            consts::CL_DEVICE_MAX_WORK_ITEM_SIZES => InfoValue::Sizes(&[256, 256, 256]), // TODO: configurable
            consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_CHAR => InfoValue::Uint(16),
            consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_SHORT => InfoValue::Uint(8),
            consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_INT => InfoValue::Uint(4),
            consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_LONG => InfoValue::Uint(2),
            consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_FLOAT => InfoValue::Uint(4),
            consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_DOUBLE => InfoValue::Uint(0),
            consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_HALF => InfoValue::Uint(0),
            consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_CHAR => InfoValue::Uint(16),
            consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_SHORT => InfoValue::Uint(8),
            consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_INT => InfoValue::Uint(4),
            consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_LONG => InfoValue::Uint(2),
            consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_FLOAT => InfoValue::Uint(4),
            consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_DOUBLE => InfoValue::Uint(0),
            consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_HALF => InfoValue::Uint(0),
            consts::CL_DEVICE_MAX_CLOCK_FREQUENCY => InfoValue::Uint(1000), // TODO: configurable
            consts::CL_DEVICE_ADDRESS_BITS => InfoValue::Uint(64),          // TODO: configurable
            consts::CL_DEVICE_MAX_READ_IMAGE_ARGS => InfoValue::Uint(0),
            consts::CL_DEVICE_MAX_WRITE_IMAGE_ARGS => InfoValue::Uint(0),
            consts::CL_DEVICE_MAX_MEM_ALLOC_SIZE => InfoValue::Ulong(1024 * 1024 * 1024), // TODO: configurable
            consts::CL_DEVICE_IMAGE2D_MAX_WIDTH => InfoValue::Size(0),
            consts::CL_DEVICE_IMAGE2D_MAX_HEIGHT => InfoValue::Size(0),
            consts::CL_DEVICE_IMAGE3D_MAX_WIDTH => InfoValue::Size(0),
            consts::CL_DEVICE_IMAGE3D_MAX_HEIGHT => InfoValue::Size(0),
            consts::CL_DEVICE_IMAGE3D_MAX_DEPTH => InfoValue::Size(0),
            consts::CL_DEVICE_IMAGE_MAX_BUFFER_SIZE => InfoValue::Size(0),
            consts::CL_DEVICE_IMAGE_MAX_ARRAY_SIZE => InfoValue::Size(0),
            consts::CL_DEVICE_IMAGE_SUPPORT => InfoValue::Uint(consts::CL_FALSE),
            consts::CL_DEVICE_MAX_PARAMETER_SIZE => InfoValue::Size(1024), // TODO: configurable
            consts::CL_DEVICE_MAX_SAMPLERS => InfoValue::Uint(0),
            consts::CL_DEVICE_MEM_BASE_ADDR_ALIGN => InfoValue::Uint(1024), // TODO
            consts::CL_DEVICE_MIN_DATA_TYPE_ALIGN_SIZE => InfoValue::Uint(128), // TODO
            consts::CL_DEVICE_SINGLE_FP_CONFIG => {
                InfoValue::Ulong(consts::CL_FP_ROUND_TO_NEAREST | consts::CL_FP_INF_NAN)
            }
            consts::CL_DEVICE_DOUBLE_FP_CONFIG => InfoValue::Ulong(0),
            consts::CL_DEVICE_GLOBAL_MEM_CACHE_TYPE => InfoValue::Uint(consts::CL_READ_WRITE_CACHE), // TODO: configurable
            consts::CL_DEVICE_GLOBAL_MEM_CACHELINE_SIZE => InfoValue::Uint(64), // TODO: configurable
            consts::CL_DEVICE_GLOBAL_MEM_CACHE_SIZE => InfoValue::Ulong(32 * 1024), // TODO: configurable
            consts::CL_DEVICE_GLOBAL_MEM_SIZE => InfoValue::Ulong(4 * 1024 * 1024 * 1024), // TODO: configurable
            consts::CL_DEVICE_MAX_CONSTANT_BUFFER_SIZE => InfoValue::Ulong(64 * 1024), // TODO: configurable
            consts::CL_DEVICE_MAX_CONSTANT_ARGS => InfoValue::Uint(8), // TODO: configurable
            consts::CL_DEVICE_LOCAL_MEM_TYPE => InfoValue::Uint(consts::CL_LOCAL),
            consts::CL_DEVICE_LOCAL_MEM_SIZE => InfoValue::Ulong(32 * 1024), // TODO: configurable
            consts::CL_DEVICE_ERROR_CORRECTION_SUPPORT => InfoValue::Uint(consts::CL_FALSE),
            consts::CL_DEVICE_PROFILING_TIMER_RESOLUTION => InfoValue::Size(1), // TODO
            consts::CL_DEVICE_ENDIAN_LITTLE => InfoValue::Uint(consts::CL_TRUE),
            consts::CL_DEVICE_AVAILABLE => InfoValue::Uint(consts::CL_TRUE),
            consts::CL_DEVICE_COMPILER_AVAILABLE => InfoValue::Uint(consts::CL_TRUE),
            consts::CL_DEVICE_LINKER_AVAILABLE => InfoValue::Uint(consts::CL_TRUE),
            consts::CL_DEVICE_EXECUTION_CAPABILITIES => InfoValue::Ulong(consts::CL_EXEC_KERNEL),
            consts::CL_DEVICE_QUEUE_PROPERTIES => {
                InfoValue::Ulong(consts::CL_QUEUE_PROFILING_ENABLE)
            }
            consts::CL_DEVICE_HOST_UNIFIED_MEMORY => InfoValue::Uint(consts::CL_TRUE),
            consts::CL_DEVICE_PLATFORM => InfoValue::Handle(Self::platform_id().cast()),
            consts::CL_DEVICE_PARENT_DEVICE => InfoValue::Handle(core::ptr::null_mut()),
            consts::CL_DEVICE_PARTITION_MAX_SUB_DEVICES => InfoValue::Uint(0),
            consts::CL_DEVICE_PARTITION_PROPERTIES => InfoValue::Properties(&[0]), // TODO
            consts::CL_DEVICE_PARTITION_AFFINITY_DOMAIN => InfoValue::Ulong(0),    // TODO
            consts::CL_DEVICE_PARTITION_TYPE => InfoValue::Properties(&[]),        // TODO
            consts::CL_DEVICE_REFERENCE_COUNT => InfoValue::Uint(1),               // TODO
            consts::CL_DEVICE_PREFERRED_INTEROP_USER_SYNC => InfoValue::Uint(consts::CL_TRUE),
            consts::CL_DEVICE_PRINTF_BUFFER_SIZE => InfoValue::Size(1024 * 1024),
            consts::CL_DEVICE_NAME => InfoValue::Text(b"vodd\0"),
            consts::CL_DEVICE_VENDOR => InfoValue::Text(b"vodd\0"),
            consts::CL_DRIVER_VERSION => InfoValue::Text(b"0.0.1\0"),
            consts::CL_DEVICE_PROFILE => InfoValue::Text(b"EMBEDDED_PROFILE\0"),
            consts::CL_DEVICE_VERSION => InfoValue::Text(b"OpenCL 1.2 vodd\0"),
            consts::CL_DEVICE_OPENCL_C_VERSION => InfoValue::Text(b"OpenCL C 1.2 \0"),
            consts::CL_DEVICE_EXTENSIONS => InfoValue::Text(b"cles_khr_int64\0"),
            consts::CL_DEVICE_BUILT_IN_KERNELS => InfoValue::Text(b"\0"),
            _ => return None,
        };

        Some(value)
    }
}
