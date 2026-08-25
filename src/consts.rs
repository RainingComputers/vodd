use crate::sys;

pub const CL_SUCCESS: sys::cl_int = 0;
pub const CL_DEVICE_NOT_FOUND: sys::cl_int = -1;
pub const CL_MEM_OBJECT_ALLOCATION_FAILURE: sys::cl_int = -4;
pub const CL_OUT_OF_HOST_MEMORY: sys::cl_int = -6;
pub const CL_MEM_COPY_OVERLAP: sys::cl_int = -8;
pub const CL_MISALIGNED_SUB_BUFFER_OFFSET: sys::cl_int = -13;
pub const CL_EXEC_STATUS_ERROR_FOR_EVENTS_IN_WAIT_LIST: sys::cl_int = -14;
pub const CL_INVALID_VALUE: sys::cl_int = -30;
pub const CL_INVALID_DEVICE_TYPE: sys::cl_int = -31;
pub const CL_INVALID_PLATFORM: sys::cl_int = -32;
pub const CL_INVALID_DEVICE: sys::cl_int = -33;
pub const CL_INVALID_CONTEXT: sys::cl_int = -34;
pub const CL_INVALID_QUEUE_PROPERTIES: sys::cl_int = -35;
pub const CL_INVALID_COMMAND_QUEUE: sys::cl_int = -36;
pub const CL_INVALID_HOST_PTR: sys::cl_int = -37;
pub const CL_INVALID_MEM_OBJECT: sys::cl_int = -38;
pub const CL_INVALID_EVENT_WAIT_LIST: sys::cl_int = -57;
pub const CL_INVALID_EVENT: sys::cl_int = -58;
pub const CL_INVALID_OPERATION: sys::cl_int = -59;
pub const CL_INVALID_BUFFER_SIZE: sys::cl_int = -61;
pub const CL_INVALID_PROPERTY: sys::cl_int = -64;

pub const CL_FALSE: sys::cl_bool = 0;
pub const CL_TRUE: sys::cl_bool = 1;

pub const CL_NONE: sys::cl_uint = 0x0;
pub const CL_READ_ONLY_CACHE: sys::cl_device_mem_cache_type = 0x1;
pub const CL_READ_WRITE_CACHE: sys::cl_device_mem_cache_type = 0x2;
pub const CL_LOCAL: sys::cl_device_local_mem_type = 0x1;
pub const CL_GLOBAL: sys::cl_device_local_mem_type = 0x2;

pub const CL_DEVICE_TYPE_DEFAULT: sys::cl_device_type = 1 << 0;
pub const CL_DEVICE_TYPE_CPU: sys::cl_device_type = 1 << 1;
pub const CL_DEVICE_TYPE_GPU: sys::cl_device_type = 1 << 2;
pub const CL_DEVICE_TYPE_ACCELERATOR: sys::cl_device_type = 1 << 3;
pub const CL_DEVICE_TYPE_CUSTOM: sys::cl_device_type = 1 << 4;
pub const CL_DEVICE_TYPE_ALL: sys::cl_device_type = 0xFFFF_FFFF;

pub const CL_CONTEXT_REFERENCE_COUNT: sys::cl_context_info = 0x1080;
pub const CL_CONTEXT_DEVICES: sys::cl_context_info = 0x1081;
pub const CL_CONTEXT_PROPERTIES: sys::cl_context_info = 0x1082;
pub const CL_CONTEXT_NUM_DEVICES: sys::cl_context_info = 0x1083;

pub const CL_CONTEXT_PLATFORM: sys::cl_context_properties = 0x1084;
pub const CL_CONTEXT_INTEROP_USER_SYNC: sys::cl_context_properties = 0x1085;

pub const CL_QUEUE_CONTEXT: sys::cl_command_queue_info = 0x1090;
pub const CL_QUEUE_DEVICE: sys::cl_command_queue_info = 0x1091;
pub const CL_QUEUE_REFERENCE_COUNT: sys::cl_command_queue_info = 0x1092;
pub const CL_QUEUE_PROPERTIES: sys::cl_command_queue_info = 0x1093;

pub const CL_PLATFORM_PROFILE: sys::cl_platform_info = 0x0900;
pub const CL_PLATFORM_VERSION: sys::cl_platform_info = 0x0901;
pub const CL_PLATFORM_NAME: sys::cl_platform_info = 0x0902;
pub const CL_PLATFORM_VENDOR: sys::cl_platform_info = 0x0903;
pub const CL_PLATFORM_EXTENSIONS: sys::cl_platform_info = 0x0904;

pub const CL_DEVICE_TYPE: sys::cl_device_info = 0x1000;
pub const CL_DEVICE_VENDOR_ID: sys::cl_device_info = 0x1001;
pub const CL_DEVICE_MAX_COMPUTE_UNITS: sys::cl_device_info = 0x1002;
pub const CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS: sys::cl_device_info = 0x1003;
pub const CL_DEVICE_MAX_WORK_GROUP_SIZE: sys::cl_device_info = 0x1004;
pub const CL_DEVICE_MAX_WORK_ITEM_SIZES: sys::cl_device_info = 0x1005;
pub const CL_DEVICE_PREFERRED_VECTOR_WIDTH_CHAR: sys::cl_device_info = 0x1006;
pub const CL_DEVICE_PREFERRED_VECTOR_WIDTH_SHORT: sys::cl_device_info = 0x1007;
pub const CL_DEVICE_PREFERRED_VECTOR_WIDTH_INT: sys::cl_device_info = 0x1008;
pub const CL_DEVICE_PREFERRED_VECTOR_WIDTH_LONG: sys::cl_device_info = 0x1009;
pub const CL_DEVICE_PREFERRED_VECTOR_WIDTH_FLOAT: sys::cl_device_info = 0x100A;
pub const CL_DEVICE_PREFERRED_VECTOR_WIDTH_DOUBLE: sys::cl_device_info = 0x100B;
pub const CL_DEVICE_MAX_CLOCK_FREQUENCY: sys::cl_device_info = 0x100C;
pub const CL_DEVICE_ADDRESS_BITS: sys::cl_device_info = 0x100D;
pub const CL_DEVICE_MAX_READ_IMAGE_ARGS: sys::cl_device_info = 0x100E;
pub const CL_DEVICE_MAX_WRITE_IMAGE_ARGS: sys::cl_device_info = 0x100F;
pub const CL_DEVICE_MAX_MEM_ALLOC_SIZE: sys::cl_device_info = 0x1010;
pub const CL_DEVICE_IMAGE2D_MAX_WIDTH: sys::cl_device_info = 0x1011;
pub const CL_DEVICE_IMAGE2D_MAX_HEIGHT: sys::cl_device_info = 0x1012;
pub const CL_DEVICE_IMAGE3D_MAX_WIDTH: sys::cl_device_info = 0x1013;
pub const CL_DEVICE_IMAGE3D_MAX_HEIGHT: sys::cl_device_info = 0x1014;
pub const CL_DEVICE_IMAGE3D_MAX_DEPTH: sys::cl_device_info = 0x1015;
pub const CL_DEVICE_IMAGE_SUPPORT: sys::cl_device_info = 0x1016;
pub const CL_DEVICE_MAX_PARAMETER_SIZE: sys::cl_device_info = 0x1017;
pub const CL_DEVICE_MAX_SAMPLERS: sys::cl_device_info = 0x1018;
pub const CL_DEVICE_MEM_BASE_ADDR_ALIGN: sys::cl_device_info = 0x1019;
pub const CL_DEVICE_MIN_DATA_TYPE_ALIGN_SIZE: sys::cl_device_info = 0x101A;
pub const CL_DEVICE_SINGLE_FP_CONFIG: sys::cl_device_info = 0x101B;
pub const CL_DEVICE_GLOBAL_MEM_CACHE_TYPE: sys::cl_device_info = 0x101C;
pub const CL_DEVICE_GLOBAL_MEM_CACHELINE_SIZE: sys::cl_device_info = 0x101D;
pub const CL_DEVICE_GLOBAL_MEM_CACHE_SIZE: sys::cl_device_info = 0x101E;
pub const CL_DEVICE_GLOBAL_MEM_SIZE: sys::cl_device_info = 0x101F;
pub const CL_DEVICE_MAX_CONSTANT_BUFFER_SIZE: sys::cl_device_info = 0x1020;
pub const CL_DEVICE_MAX_CONSTANT_ARGS: sys::cl_device_info = 0x1021;
pub const CL_DEVICE_LOCAL_MEM_TYPE: sys::cl_device_info = 0x1022;
pub const CL_DEVICE_LOCAL_MEM_SIZE: sys::cl_device_info = 0x1023;
pub const CL_DEVICE_ERROR_CORRECTION_SUPPORT: sys::cl_device_info = 0x1024;
pub const CL_DEVICE_PROFILING_TIMER_RESOLUTION: sys::cl_device_info = 0x1025;
pub const CL_DEVICE_ENDIAN_LITTLE: sys::cl_device_info = 0x1026;
pub const CL_DEVICE_AVAILABLE: sys::cl_device_info = 0x1027;
pub const CL_DEVICE_COMPILER_AVAILABLE: sys::cl_device_info = 0x1028;
pub const CL_DEVICE_EXECUTION_CAPABILITIES: sys::cl_device_info = 0x1029;
pub const CL_DEVICE_QUEUE_PROPERTIES: sys::cl_device_info = 0x102A;
pub const CL_DEVICE_NAME: sys::cl_device_info = 0x102B;
pub const CL_DEVICE_VENDOR: sys::cl_device_info = 0x102C;
pub const CL_DRIVER_VERSION: sys::cl_device_info = 0x102D;
pub const CL_DEVICE_PROFILE: sys::cl_device_info = 0x102E;
pub const CL_DEVICE_VERSION: sys::cl_device_info = 0x102F;
pub const CL_DEVICE_EXTENSIONS: sys::cl_device_info = 0x1030;
pub const CL_DEVICE_PLATFORM: sys::cl_device_info = 0x1031;
pub const CL_DEVICE_DOUBLE_FP_CONFIG: sys::cl_device_info = 0x1032;
pub const CL_DEVICE_PREFERRED_VECTOR_WIDTH_HALF: sys::cl_device_info = 0x1034;
pub const CL_DEVICE_HOST_UNIFIED_MEMORY: sys::cl_device_info = 0x1035;
pub const CL_DEVICE_NATIVE_VECTOR_WIDTH_CHAR: sys::cl_device_info = 0x1036;
pub const CL_DEVICE_NATIVE_VECTOR_WIDTH_SHORT: sys::cl_device_info = 0x1037;
pub const CL_DEVICE_NATIVE_VECTOR_WIDTH_INT: sys::cl_device_info = 0x1038;
pub const CL_DEVICE_NATIVE_VECTOR_WIDTH_LONG: sys::cl_device_info = 0x1039;
pub const CL_DEVICE_NATIVE_VECTOR_WIDTH_FLOAT: sys::cl_device_info = 0x103A;
pub const CL_DEVICE_NATIVE_VECTOR_WIDTH_DOUBLE: sys::cl_device_info = 0x103B;
pub const CL_DEVICE_NATIVE_VECTOR_WIDTH_HALF: sys::cl_device_info = 0x103C;
pub const CL_DEVICE_OPENCL_C_VERSION: sys::cl_device_info = 0x103D;
pub const CL_DEVICE_LINKER_AVAILABLE: sys::cl_device_info = 0x103E;
pub const CL_DEVICE_BUILT_IN_KERNELS: sys::cl_device_info = 0x103F;
pub const CL_DEVICE_IMAGE_MAX_BUFFER_SIZE: sys::cl_device_info = 0x1040;
pub const CL_DEVICE_IMAGE_MAX_ARRAY_SIZE: sys::cl_device_info = 0x1041;
pub const CL_DEVICE_PARENT_DEVICE: sys::cl_device_info = 0x1042;
pub const CL_DEVICE_PARTITION_MAX_SUB_DEVICES: sys::cl_device_info = 0x1043;
pub const CL_DEVICE_PARTITION_PROPERTIES: sys::cl_device_info = 0x1044;
pub const CL_DEVICE_PARTITION_AFFINITY_DOMAIN: sys::cl_device_info = 0x1045;
pub const CL_DEVICE_PARTITION_TYPE: sys::cl_device_info = 0x1046;
pub const CL_DEVICE_REFERENCE_COUNT: sys::cl_device_info = 0x1047;
pub const CL_DEVICE_PREFERRED_INTEROP_USER_SYNC: sys::cl_device_info = 0x1048;
pub const CL_DEVICE_PRINTF_BUFFER_SIZE: sys::cl_device_info = 0x1049;

pub const CL_FP_DENORM: sys::cl_device_fp_config = 1 << 0;
pub const CL_FP_INF_NAN: sys::cl_device_fp_config = 1 << 1;
pub const CL_FP_ROUND_TO_NEAREST: sys::cl_device_fp_config = 1 << 2;
pub const CL_FP_ROUND_TO_ZERO: sys::cl_device_fp_config = 1 << 3;
pub const CL_FP_ROUND_TO_INF: sys::cl_device_fp_config = 1 << 4;
pub const CL_FP_FMA: sys::cl_device_fp_config = 1 << 5;
pub const CL_FP_SOFT_FLOAT: sys::cl_device_fp_config = 1 << 6;

pub const CL_EXEC_KERNEL: sys::cl_device_exec_capabilities = 1 << 0;
pub const CL_EXEC_NATIVE_KERNEL: sys::cl_device_exec_capabilities = 1 << 1;
pub const CL_QUEUE_OUT_OF_ORDER_EXEC_MODE_ENABLE: sys::cl_command_queue_properties = 1 << 0;
pub const CL_QUEUE_PROFILING_ENABLE: sys::cl_command_queue_properties = 1 << 1;

pub const CL_MEM_READ_WRITE: sys::cl_mem_flags = 1 << 0;
pub const CL_MEM_WRITE_ONLY: sys::cl_mem_flags = 1 << 1;
pub const CL_MEM_READ_ONLY: sys::cl_mem_flags = 1 << 2;
pub const CL_MEM_USE_HOST_PTR: sys::cl_mem_flags = 1 << 3;
pub const CL_MEM_ALLOC_HOST_PTR: sys::cl_mem_flags = 1 << 4;
pub const CL_MEM_COPY_HOST_PTR: sys::cl_mem_flags = 1 << 5;
pub const CL_MEM_HOST_WRITE_ONLY: sys::cl_mem_flags = 1 << 7;
pub const CL_MEM_HOST_READ_ONLY: sys::cl_mem_flags = 1 << 8;
pub const CL_MEM_HOST_NO_ACCESS: sys::cl_mem_flags = 1 << 9;

pub const CL_MIGRATE_MEM_OBJECT_HOST: sys::cl_mem_migration_flags = 1 << 0;
pub const CL_MIGRATE_MEM_OBJECT_CONTENT_UNDEFINED: sys::cl_mem_migration_flags = 1 << 1;

pub const CL_MEM_OBJECT_BUFFER: sys::cl_mem_object_type = 0x10F0;

pub const CL_MEM_TYPE: sys::cl_mem_info = 0x1100;
pub const CL_MEM_FLAGS: sys::cl_mem_info = 0x1101;
pub const CL_MEM_SIZE: sys::cl_mem_info = 0x1102;
pub const CL_MEM_HOST_PTR: sys::cl_mem_info = 0x1103;
pub const CL_MEM_MAP_COUNT: sys::cl_mem_info = 0x1104;
pub const CL_MEM_REFERENCE_COUNT: sys::cl_mem_info = 0x1105;
pub const CL_MEM_CONTEXT: sys::cl_mem_info = 0x1106;
pub const CL_MEM_ASSOCIATED_MEMOBJECT: sys::cl_mem_info = 0x1107;
pub const CL_MEM_OFFSET: sys::cl_mem_info = 0x1108;

pub const CL_MAP_READ: sys::cl_map_flags = 1 << 0;
pub const CL_MAP_WRITE: sys::cl_map_flags = 1 << 1;
pub const CL_MAP_WRITE_INVALIDATE_REGION: sys::cl_map_flags = 1 << 2;

pub const CL_EVENT_COMMAND_QUEUE: sys::cl_event_info = 0x11D0;
pub const CL_EVENT_COMMAND_TYPE: sys::cl_event_info = 0x11D1;
pub const CL_EVENT_REFERENCE_COUNT: sys::cl_event_info = 0x11D2;
pub const CL_EVENT_COMMAND_EXECUTION_STATUS: sys::cl_event_info = 0x11D3;
pub const CL_EVENT_CONTEXT: sys::cl_event_info = 0x11D4;

pub const CL_COMMAND_READ_BUFFER: sys::cl_command_type = 0x11F3;
pub const CL_COMMAND_WRITE_BUFFER: sys::cl_command_type = 0x11F4;
pub const CL_COMMAND_COPY_BUFFER: sys::cl_command_type = 0x11F5;
pub const CL_COMMAND_MAP_BUFFER: sys::cl_command_type = 0x11FB;
pub const CL_COMMAND_UNMAP_MEM_OBJECT: sys::cl_command_type = 0x11FD;
pub const CL_COMMAND_MARKER: sys::cl_command_type = 0x11FE;
pub const CL_COMMAND_READ_BUFFER_RECT: sys::cl_command_type = 0x1201;
pub const CL_COMMAND_WRITE_BUFFER_RECT: sys::cl_command_type = 0x1202;
pub const CL_COMMAND_COPY_BUFFER_RECT: sys::cl_command_type = 0x1203;
pub const CL_COMMAND_USER: sys::cl_command_type = 0x1204;
pub const CL_COMMAND_BARRIER: sys::cl_command_type = 0x1205;
pub const CL_COMMAND_MIGRATE_MEM_OBJECTS: sys::cl_command_type = 0x1206;
pub const CL_COMMAND_FILL_BUFFER: sys::cl_command_type = 0x1207;

pub const CL_COMPLETE: sys::cl_int = 0x0;
pub const CL_RUNNING: sys::cl_int = 0x1;
pub const CL_SUBMITTED: sys::cl_int = 0x2;
pub const CL_QUEUED: sys::cl_int = 0x3;

pub const CL_BUFFER_CREATE_TYPE_REGION: sys::cl_buffer_create_type = 0x1220;
