use crate::consts;
use crate::platform;
use crate::sys;
use std::ffi::CString;
use std::sync::Arc;

static PLATFORM_TOKEN: u8 = 0xA0;
static DEVICE_TOKEN: u8 = 0xD0;

#[derive(Clone, Copy)]
struct UserData(*mut core::ffi::c_void);

unsafe impl Send for UserData {}
unsafe impl Sync for UserData {}

impl UserData {
    fn pointer(self) -> *mut core::ffi::c_void {
        self.0
    }
}

pub(crate) fn platform_handle() -> sys::cl_platform_id {
    &raw const PLATFORM_TOKEN as *mut sys::_cl_platform_id
}

pub(crate) fn device_handle(_device: platform::Device) -> sys::cl_device_id {
    &raw const DEVICE_TOKEN as *mut sys::_cl_device_id
}

pub(crate) fn object_handle<T>(id: u32) -> *mut T {
    id as usize as *mut T
}

pub(crate) fn object_id<T>(handle: *mut T) -> u32 {
    handle as usize as u32
}

pub(crate) fn platform_id(handle: sys::cl_platform_id) -> platform::Result<()> {
    (handle == platform_handle())
        .then_some(())
        .ok_or(platform::Error::InvalidPlatform)
}

pub(crate) fn device(handle: sys::cl_device_id) -> platform::Result<platform::Device> {
    (handle == device_handle(platform::Device))
        .then_some(platform::Device)
        .ok_or(platform::Error::InvalidDevice)
}

pub(crate) fn context_id(handle: sys::cl_context) -> platform::ContextId {
    platform::ContextId::from_raw(object_id(handle))
}

pub(crate) fn queue_id(handle: sys::cl_command_queue) -> platform::QueueId {
    platform::QueueId::from_raw(object_id(handle))
}

pub(crate) fn buffer_id(handle: sys::cl_mem) -> platform::BufferId {
    platform::BufferId::from_raw(object_id(handle))
}

pub(crate) fn event_id(handle: sys::cl_event) -> platform::EventId {
    platform::EventId::from_raw(object_id(handle))
}

pub(crate) fn program_id(handle: sys::cl_program) -> platform::ProgramId {
    platform::ProgramId::from_raw(object_id(handle))
}

pub(crate) fn kernel_id(handle: sys::cl_kernel) -> platform::KernelId {
    platform::KernelId::from_raw(object_id(handle))
}

pub(crate) unsafe fn source(
    count: sys::cl_uint,
    strings: *mut *const core::ffi::c_char,
    lengths: *const usize,
) -> platform::Result<String> {
    if strings.is_null() || count == 0 {
        return Err(platform::Error::InvalidValue);
    }

    let listed = unsafe { core::slice::from_raw_parts(strings, count as usize) };
    let sizes = (!lengths.is_null())
        .then(|| unsafe { core::slice::from_raw_parts(lengths, count as usize) });

    listed
        .iter()
        .enumerate()
        .map(|(index, text)| {
            if text.is_null() {
                return Err(platform::Error::InvalidValue);
            }

            let bytes = match sizes.map(|sizes| sizes[index]) {
                Some(0) | None => unsafe { core::ffi::CStr::from_ptr(*text) }.to_bytes(),
                Some(length) => unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) },
            };

            Ok(String::from_utf8_lossy(bytes).into_owned())
        })
        .collect::<platform::Result<Vec<String>>>()
        .map(|parts| parts.concat())
}

pub(crate) unsafe fn options(options: *const core::ffi::c_char) -> String {
    if options.is_null() {
        return String::new();
    }

    unsafe { core::ffi::CStr::from_ptr(options) }
        .to_string_lossy()
        .into_owned()
}

pub(crate) unsafe fn binary(
    num_devices: sys::cl_uint,
    lengths: *const usize,
    binaries: *mut *const sys::cl_uchar,
) -> platform::Result<Vec<u8>> {
    if num_devices == 0 || lengths.is_null() || binaries.is_null() {
        return Err(platform::Error::InvalidValue);
    }

    let length = unsafe { *lengths };
    let pointer = unsafe { *binaries };

    if length == 0 || pointer.is_null() {
        return Err(platform::Error::InvalidValue);
    }

    Ok(unsafe { core::slice::from_raw_parts(pointer, length) }.to_vec())
}

pub(crate) unsafe fn programs(
    count: sys::cl_uint,
    listed: *const sys::cl_program,
) -> platform::Result<Vec<platform::ProgramId>> {
    if listed.is_null() || count == 0 {
        return Err(platform::Error::InvalidValue);
    }

    let listed = unsafe { core::slice::from_raw_parts(listed, count as usize) };

    Ok(listed.iter().map(|handle| program_id(*handle)).collect())
}

pub(crate) unsafe fn headers(
    count: sys::cl_uint,
    programs: *const sys::cl_program,
    names: *mut *const core::ffi::c_char,
) -> platform::Result<Vec<(String, platform::ProgramId)>> {
    if count == 0 {
        return Ok(Vec::new());
    }

    if programs.is_null() || names.is_null() {
        return Err(platform::Error::InvalidValue);
    }

    let programs = unsafe { core::slice::from_raw_parts(programs, count as usize) };
    let names = unsafe { core::slice::from_raw_parts(names, count as usize) };

    Ok(programs
        .iter()
        .zip(names)
        .map(|(handle, name)| {
            let text = unsafe { core::ffi::CStr::from_ptr(*name) }
                .to_string_lossy()
                .into_owned();

            (text, program_id(*handle))
        })
        .collect())
}

pub(crate) unsafe fn geometry(
    work_dim: sys::cl_uint,
    global_work_offset: *const usize,
    global_work_size: *const usize,
    local_work_size: *const usize,
) -> platform::Result<platform::Geometry> {
    if !(1..=3).contains(&work_dim) {
        return Err(platform::Error::InvalidWorkDimension);
    }

    if global_work_size.is_null() {
        return Err(platform::Error::InvalidGlobalWorkSize);
    }

    let dimensions = work_dim as usize;
    let read = |source: *const usize, fallback: u64| -> [u64; 3] {
        let mut collected = [fallback; 3];

        if !source.is_null() {
            let listed = unsafe { core::slice::from_raw_parts(source, dimensions) };

            for (slot, value) in collected.iter_mut().zip(listed) {
                *slot = *value as u64;
            }
        }

        collected
    };

    Ok(platform::Geometry {
        dimensions: work_dim,
        offset: read(global_work_offset, 0),
        global: read(global_work_size, 1),
        local: (!local_work_size.is_null()).then(|| read(local_work_size, 1)),
    })
}

pub(crate) unsafe fn shared_memory_pointer(
    pointer: *const core::ffi::c_void,
) -> platform::SharedMemoryPointer {
    unsafe { platform::SharedMemoryPointer::new(pointer.cast_mut().cast()) }
}

pub(crate) unsafe fn devices(
    num_devices: sys::cl_uint,
    devices: *const sys::cl_device_id,
) -> platform::Result<Vec<platform::Device>> {
    if devices.is_null() || num_devices == 0 {
        return Err(platform::Error::InvalidValue);
    }

    let listed = unsafe { core::slice::from_raw_parts(devices, num_devices as usize) };

    listed.iter().map(|handle| device(*handle)).collect()
}

pub(crate) unsafe fn buffers(
    num_mem_objects: sys::cl_uint,
    mem_objects: *const sys::cl_mem,
) -> platform::Result<Vec<platform::BufferId>> {
    if mem_objects.is_null() || num_mem_objects == 0 {
        return Err(platform::Error::InvalidValue);
    }

    let listed = unsafe { core::slice::from_raw_parts(mem_objects, num_mem_objects as usize) };

    Ok(listed.iter().map(|handle| buffer_id(*handle)).collect())
}

pub(crate) unsafe fn event_wait_list(
    num_events_in_wait_list: sys::cl_uint,
    event_wait_list: *const sys::cl_event,
) -> platform::Result<Vec<platform::EventId>> {
    if event_wait_list.is_null() != (num_events_in_wait_list == 0) {
        return Err(platform::Error::InvalidEventWaitList);
    }

    if event_wait_list.is_null() {
        return Ok(Vec::new());
    }

    let listed =
        unsafe { core::slice::from_raw_parts(event_wait_list, num_events_in_wait_list as usize) };

    listed
        .iter()
        .map(|handle| {
            let id = event_id(*handle);

            platform::Event::exists(id)
                .map(|()| id)
                .map_err(|_| platform::Error::InvalidEventWaitList)
        })
        .collect()
}

pub(crate) unsafe fn events(
    num_events: sys::cl_uint,
    event_list: *const sys::cl_event,
) -> platform::Result<Vec<platform::EventId>> {
    if event_list.is_null() || num_events == 0 {
        return Err(platform::Error::InvalidValue);
    }

    let listed = unsafe { core::slice::from_raw_parts(event_list, num_events as usize) };

    Ok(listed.iter().map(|handle| event_id(*handle)).collect())
}

pub(crate) unsafe fn pattern(
    pattern: *const core::ffi::c_void,
    pattern_size: usize,
) -> platform::Result<Vec<u8>> {
    if pattern.is_null() || pattern_size == 0 {
        return Err(platform::Error::InvalidValue);
    }

    Ok(unsafe { core::slice::from_raw_parts(pattern.cast::<u8>(), pattern_size) }.to_vec())
}

pub(crate) unsafe fn region(origin: *const usize) -> platform::Result<[usize; 3]> {
    if origin.is_null() {
        return Err(platform::Error::InvalidValue);
    }

    let mut collected = [0usize; 3];
    collected.copy_from_slice(unsafe { core::slice::from_raw_parts(origin, 3) });

    Ok(collected)
}

pub(crate) fn device_type(raw: sys::cl_device_type) -> platform::Result<platform::DeviceType> {
    let known = consts::CL_DEVICE_TYPE_DEFAULT
        | consts::CL_DEVICE_TYPE_CPU
        | consts::CL_DEVICE_TYPE_GPU
        | consts::CL_DEVICE_TYPE_ACCELERATOR
        | consts::CL_DEVICE_TYPE_CUSTOM;

    if raw == consts::CL_DEVICE_TYPE_ALL {
        return Ok(platform::DeviceType::ALL);
    }

    if raw == 0 || raw & !known != 0 {
        return Err(platform::Error::InvalidDeviceType);
    }

    Ok(platform::DeviceType {
        cpu: raw & consts::CL_DEVICE_TYPE_CPU != 0,
        gpu: raw & consts::CL_DEVICE_TYPE_GPU != 0,
        accelerator: raw & consts::CL_DEVICE_TYPE_ACCELERATOR != 0,
        custom: raw & consts::CL_DEVICE_TYPE_CUSTOM != 0,
        default: raw & consts::CL_DEVICE_TYPE_DEFAULT != 0,
    })
}

pub(crate) unsafe fn context_properties(
    properties: *const sys::cl_context_properties,
) -> platform::Result<Option<Vec<platform::ContextProperty>>> {
    if properties.is_null() {
        return Ok(None);
    }

    let mut listed: Vec<[sys::cl_context_properties; 2]> = Vec::new();
    let mut cursor = properties;

    unsafe {
        while *cursor != 0 {
            listed.push([*cursor, *cursor.add(1)]);
            cursor = cursor.add(2);
        }
    }

    let named: Vec<sys::cl_context_properties> = listed.iter().map(|pair| pair[0]).collect();
    if named
        .iter()
        .enumerate()
        .any(|(index, name)| named[..index].contains(name))
    {
        return Err(platform::Error::InvalidProperty);
    }

    listed
        .iter()
        .map(|pair| context_property(pair[0], pair[1]))
        .collect::<platform::Result<Vec<platform::ContextProperty>>>()
        .map(Some)
}

fn context_property(
    name: sys::cl_context_properties,
    value: sys::cl_context_properties,
) -> platform::Result<platform::ContextProperty> {
    match name {
        consts::CL_CONTEXT_PLATFORM => {
            platform_id(value as sys::cl_platform_id).map(|()| platform::ContextProperty::Platform)
        }
        consts::CL_CONTEXT_INTEROP_USER_SYNC => match value as sys::cl_bool {
            consts::CL_FALSE => Ok(platform::ContextProperty::InteropUserSync(false)),
            consts::CL_TRUE => Ok(platform::ContextProperty::InteropUserSync(true)),
            _ => Err(platform::Error::InvalidProperty),
        },
        _ => Err(platform::Error::InvalidProperty),
    }
}

fn encode_context_properties(
    properties: Option<Vec<platform::ContextProperty>>,
) -> Vec<sys::cl_context_properties> {
    let Some(properties) = properties else {
        return Vec::new();
    };

    let mut encoded: Vec<sys::cl_context_properties> = properties
        .iter()
        .flat_map(|property| match property {
            platform::ContextProperty::Platform => {
                [consts::CL_CONTEXT_PLATFORM, platform_handle() as isize]
            }
            platform::ContextProperty::InteropUserSync(enabled) => [
                consts::CL_CONTEXT_INTEROP_USER_SYNC,
                *enabled as sys::cl_context_properties,
            ],
        })
        .collect();

    encoded.push(0);

    encoded
}

pub(crate) fn queue_properties(
    raw: sys::cl_command_queue_properties,
) -> platform::Result<platform::QueueProperties> {
    let known = consts::CL_QUEUE_OUT_OF_ORDER_EXEC_MODE_ENABLE | consts::CL_QUEUE_PROFILING_ENABLE;

    if raw & !known != 0 {
        return Err(platform::Error::InvalidValue);
    }

    Ok(platform::QueueProperties {
        out_of_order: raw & consts::CL_QUEUE_OUT_OF_ORDER_EXEC_MODE_ENABLE != 0,
        profiling: raw & consts::CL_QUEUE_PROFILING_ENABLE != 0,
    })
}

fn encode_queue_properties(properties: platform::QueueProperties) -> sys::cl_bitfield {
    bits(&[
        (
            properties.out_of_order,
            consts::CL_QUEUE_OUT_OF_ORDER_EXEC_MODE_ENABLE,
        ),
        (properties.profiling, consts::CL_QUEUE_PROFILING_ENABLE),
    ])
}

pub(crate) unsafe fn mem_flags(
    raw: sys::cl_mem_flags,
    host_ptr: *mut core::ffi::c_void,
) -> platform::Result<platform::MemFlags> {
    let access_group =
        consts::CL_MEM_READ_WRITE | consts::CL_MEM_WRITE_ONLY | consts::CL_MEM_READ_ONLY;
    let host_group = consts::CL_MEM_HOST_WRITE_ONLY
        | consts::CL_MEM_HOST_READ_ONLY
        | consts::CL_MEM_HOST_NO_ACCESS;
    let location_group =
        consts::CL_MEM_USE_HOST_PTR | consts::CL_MEM_ALLOC_HOST_PTR | consts::CL_MEM_COPY_HOST_PTR;
    let known = access_group | host_group | location_group;

    let exclusive = [access_group, host_group]
        .into_iter()
        .all(|group| (raw & group).count_ones() <= 1);

    let borrowed = raw & consts::CL_MEM_USE_HOST_PTR != 0;
    let copied = raw & consts::CL_MEM_COPY_HOST_PTR != 0;
    let allocated = raw & consts::CL_MEM_ALLOC_HOST_PTR != 0;

    if raw & !known != 0 || !exclusive || (borrowed && (copied || allocated)) {
        return Err(platform::Error::InvalidValue);
    }

    if host_ptr.is_null() == (borrowed || copied) {
        return Err(platform::Error::InvalidHostPtr);
    }

    let pointer = unsafe { shared_memory_pointer(host_ptr) };

    Ok(platform::MemFlags {
        access: match raw & access_group {
            consts::CL_MEM_READ_WRITE => platform::Access::ReadWrite,
            consts::CL_MEM_WRITE_ONLY => platform::Access::WriteOnly,
            consts::CL_MEM_READ_ONLY => platform::Access::ReadOnly,
            _ => platform::Access::Unspecified,
        },
        host_access: match raw & host_group {
            consts::CL_MEM_HOST_WRITE_ONLY => platform::HostAccess::WriteOnly,
            consts::CL_MEM_HOST_READ_ONLY => platform::HostAccess::ReadOnly,
            consts::CL_MEM_HOST_NO_ACCESS => platform::HostAccess::NoAccess,
            _ => platform::HostAccess::Unspecified,
        },
        alloc_host: allocated,
        storage: match (borrowed, copied) {
            (true, _) => platform::Storage::Borrowed(pointer),
            (_, true) => platform::Storage::Copied(pointer),
            _ => platform::Storage::Owned,
        },
    })
}

fn encode_mem_flags(flags: platform::MemFlags) -> sys::cl_bitfield {
    let access = match flags.access {
        platform::Access::Unspecified => 0,
        platform::Access::ReadWrite => consts::CL_MEM_READ_WRITE,
        platform::Access::WriteOnly => consts::CL_MEM_WRITE_ONLY,
        platform::Access::ReadOnly => consts::CL_MEM_READ_ONLY,
    };
    let host_access = match flags.host_access {
        platform::HostAccess::Unspecified => 0,
        platform::HostAccess::WriteOnly => consts::CL_MEM_HOST_WRITE_ONLY,
        platform::HostAccess::ReadOnly => consts::CL_MEM_HOST_READ_ONLY,
        platform::HostAccess::NoAccess => consts::CL_MEM_HOST_NO_ACCESS,
    };
    let storage = match flags.storage {
        platform::Storage::Owned => 0,
        platform::Storage::Borrowed(_) => consts::CL_MEM_USE_HOST_PTR,
        platform::Storage::Copied(_) => consts::CL_MEM_COPY_HOST_PTR,
    };
    bits(&[
        (true, access),
        (true, host_access),
        (true, storage),
        (flags.alloc_host, consts::CL_MEM_ALLOC_HOST_PTR),
    ])
}

pub(crate) fn map_flags(raw: sys::cl_map_flags) -> platform::Result<platform::MapFlags> {
    let known = consts::CL_MAP_READ | consts::CL_MAP_WRITE | consts::CL_MAP_WRITE_INVALIDATE_REGION;

    if raw & !known != 0 {
        return Err(platform::Error::InvalidValue);
    }

    Ok(platform::MapFlags {
        read: raw & consts::CL_MAP_READ != 0,
        write: raw & consts::CL_MAP_WRITE != 0,
        write_invalidate: raw & consts::CL_MAP_WRITE_INVALIDATE_REGION != 0,
    })
}

pub(crate) fn migrate_flags(
    raw: sys::cl_mem_migration_flags,
) -> platform::Result<platform::MigrateFlags> {
    let known =
        consts::CL_MIGRATE_MEM_OBJECT_HOST | consts::CL_MIGRATE_MEM_OBJECT_CONTENT_UNDEFINED;

    if raw & !known != 0 {
        return Err(platform::Error::InvalidValue);
    }

    Ok(platform::MigrateFlags {
        host: raw & consts::CL_MIGRATE_MEM_OBJECT_HOST != 0,
        content_undefined: raw & consts::CL_MIGRATE_MEM_OBJECT_CONTENT_UNDEFINED != 0,
    })
}

pub(crate) fn buffer_region(
    buffer_create_type: sys::cl_buffer_create_type,
    buffer_create_info: *const core::ffi::c_void,
) -> platform::Result<sys::cl_buffer_region> {
    if buffer_create_type != consts::CL_BUFFER_CREATE_TYPE_REGION || buffer_create_info.is_null() {
        return Err(platform::Error::InvalidValue);
    }

    Ok(unsafe { *buffer_create_info.cast::<sys::cl_buffer_region>() })
}

pub(crate) fn user_status(raw: sys::cl_int) -> platform::Result<platform::Status> {
    match raw {
        consts::CL_COMPLETE => Ok(platform::Status::Complete),
        code if code < 0 => Ok(platform::Status::Terminated(code)),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn callback_status(raw: sys::cl_int) -> platform::Result<platform::Status> {
    match raw {
        consts::CL_SUBMITTED => Ok(platform::Status::Submitted),
        consts::CL_RUNNING => Ok(platform::Status::Running),
        consts::CL_COMPLETE => Ok(platform::Status::Complete),
        _ => Err(platform::Error::InvalidValue),
    }
}

fn encode_status(status: platform::Status) -> sys::cl_int {
    match status {
        platform::Status::Queued => consts::CL_QUEUED,
        platform::Status::Submitted => consts::CL_SUBMITTED,
        platform::Status::Running => consts::CL_RUNNING,
        platform::Status::Complete => consts::CL_COMPLETE,
        platform::Status::Terminated(code) => code,
        platform::Status::Failed(error) => code(error),
    }
}

fn encode_command_type(command_type: platform::CommandType) -> sys::cl_command_type {
    match command_type {
        platform::CommandType::ReadBuffer => consts::CL_COMMAND_READ_BUFFER,
        platform::CommandType::WriteBuffer => consts::CL_COMMAND_WRITE_BUFFER,
        platform::CommandType::CopyBuffer => consts::CL_COMMAND_COPY_BUFFER,
        platform::CommandType::ReadBufferRect => consts::CL_COMMAND_READ_BUFFER_RECT,
        platform::CommandType::WriteBufferRect => consts::CL_COMMAND_WRITE_BUFFER_RECT,
        platform::CommandType::CopyBufferRect => consts::CL_COMMAND_COPY_BUFFER_RECT,
        platform::CommandType::FillBuffer => consts::CL_COMMAND_FILL_BUFFER,
        platform::CommandType::MapBuffer => consts::CL_COMMAND_MAP_BUFFER,
        platform::CommandType::UnmapMemObject => consts::CL_COMMAND_UNMAP_MEM_OBJECT,
        platform::CommandType::MigrateMemObjects => consts::CL_COMMAND_MIGRATE_MEM_OBJECTS,
        platform::CommandType::Marker => consts::CL_COMMAND_MARKER,
        platform::CommandType::Barrier => consts::CL_COMMAND_BARRIER,
        platform::CommandType::User => consts::CL_COMMAND_USER,
        platform::CommandType::NdrangeKernel => consts::CL_COMMAND_NDRANGE_KERNEL,
        platform::CommandType::Task => consts::CL_COMMAND_TASK,
    }
}

pub(crate) unsafe fn context_notify(
    pfn_notify: Option<sys::cl_context_callback>,
    user_data: *mut core::ffi::c_void,
) -> platform::Result<Option<platform::ContextNotify>> {
    match pfn_notify {
        None if user_data.is_null() => Ok(None),
        None => Err(platform::Error::InvalidValue),
        Some(notify) => {
            let carried = UserData(user_data);

            Ok(Some(Arc::new(move |message: &str| {
                let text = CString::new(message).unwrap_or_default();

                unsafe { notify(text.as_ptr(), core::ptr::null(), 0, carried.pointer()) };
            })))
        }
    }
}

pub(crate) unsafe fn event_notify(
    pfn_notify: Option<sys::cl_event_callback>,
    user_data: *mut core::ffi::c_void,
) -> platform::Result<platform::EventNotify> {
    let notify = pfn_notify.ok_or(platform::Error::InvalidValue)?;
    let carried = UserData(user_data);

    Ok(Box::new(move |id: platform::EventId, status| unsafe {
        notify(
            object_handle(id.into_raw()),
            encode_status(status),
            carried.pointer(),
        );
    }))
}

pub(crate) unsafe fn program_notify(
    pfn_notify: Option<unsafe extern "C" fn(sys::cl_program, *mut core::ffi::c_void)>,
    user_data: *mut core::ffi::c_void,
) -> Option<platform::ProgramNotify> {
    let notify = pfn_notify?;
    let carried = UserData(user_data);

    Some(Box::new(move |id: platform::ProgramId| unsafe {
        notify(object_handle(id.into_raw()), carried.pointer());
    }))
}

pub(crate) unsafe fn destructor_notify(
    pfn_notify: Option<sys::cl_mem_destructor_callback>,
    user_data: *mut core::ffi::c_void,
) -> platform::Result<platform::DestructorNotify> {
    let notify = pfn_notify.ok_or(platform::Error::InvalidValue)?;
    let carried = UserData(user_data);

    Ok(Box::new(move |id: platform::BufferId| unsafe {
        notify(object_handle(id.into_raw()), carried.pointer());
    }))
}

pub(crate) fn platform_info(
    raw: sys::cl_platform_info,
) -> platform::Result<platform::PlatformInfo> {
    match raw {
        consts::CL_PLATFORM_PROFILE => Ok(platform::PlatformInfo::Profile),
        consts::CL_PLATFORM_VERSION => Ok(platform::PlatformInfo::Version),
        consts::CL_PLATFORM_NAME => Ok(platform::PlatformInfo::Name),
        consts::CL_PLATFORM_VENDOR => Ok(platform::PlatformInfo::Vendor),
        consts::CL_PLATFORM_EXTENSIONS => Ok(platform::PlatformInfo::Extensions),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn device_info(raw: sys::cl_device_info) -> platform::Result<platform::DeviceInfo> {
    match raw {
        consts::CL_DEVICE_TYPE => Ok(platform::DeviceInfo::Type),
        consts::CL_DEVICE_VENDOR_ID => Ok(platform::DeviceInfo::VendorId),
        consts::CL_DEVICE_MAX_COMPUTE_UNITS => Ok(platform::DeviceInfo::MaxComputeUnits),
        consts::CL_DEVICE_MAX_WORK_ITEM_DIMENSIONS => {
            Ok(platform::DeviceInfo::MaxWorkItemDimensions)
        }
        consts::CL_DEVICE_MAX_WORK_GROUP_SIZE => Ok(platform::DeviceInfo::MaxWorkGroupSize),
        consts::CL_DEVICE_MAX_WORK_ITEM_SIZES => Ok(platform::DeviceInfo::MaxWorkItemSizes),
        consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_CHAR => {
            Ok(platform::DeviceInfo::PreferredVectorWidthChar)
        }
        consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_SHORT => {
            Ok(platform::DeviceInfo::PreferredVectorWidthShort)
        }
        consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_INT => {
            Ok(platform::DeviceInfo::PreferredVectorWidthInt)
        }
        consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_LONG => {
            Ok(platform::DeviceInfo::PreferredVectorWidthLong)
        }
        consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_FLOAT => {
            Ok(platform::DeviceInfo::PreferredVectorWidthFloat)
        }
        consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_DOUBLE => {
            Ok(platform::DeviceInfo::PreferredVectorWidthDouble)
        }
        consts::CL_DEVICE_PREFERRED_VECTOR_WIDTH_HALF => {
            Ok(platform::DeviceInfo::PreferredVectorWidthHalf)
        }
        consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_CHAR => {
            Ok(platform::DeviceInfo::NativeVectorWidthChar)
        }
        consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_SHORT => {
            Ok(platform::DeviceInfo::NativeVectorWidthShort)
        }
        consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_INT => Ok(platform::DeviceInfo::NativeVectorWidthInt),
        consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_LONG => {
            Ok(platform::DeviceInfo::NativeVectorWidthLong)
        }
        consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_FLOAT => {
            Ok(platform::DeviceInfo::NativeVectorWidthFloat)
        }
        consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_DOUBLE => {
            Ok(platform::DeviceInfo::NativeVectorWidthDouble)
        }
        consts::CL_DEVICE_NATIVE_VECTOR_WIDTH_HALF => {
            Ok(platform::DeviceInfo::NativeVectorWidthHalf)
        }
        consts::CL_DEVICE_MAX_CLOCK_FREQUENCY => Ok(platform::DeviceInfo::MaxClockFrequency),
        consts::CL_DEVICE_ADDRESS_BITS => Ok(platform::DeviceInfo::AddressBits),
        consts::CL_DEVICE_MAX_READ_IMAGE_ARGS => Ok(platform::DeviceInfo::MaxReadImageArgs),
        consts::CL_DEVICE_MAX_WRITE_IMAGE_ARGS => Ok(platform::DeviceInfo::MaxWriteImageArgs),
        consts::CL_DEVICE_MAX_MEM_ALLOC_SIZE => Ok(platform::DeviceInfo::MaxMemAllocSize),
        consts::CL_DEVICE_IMAGE2D_MAX_WIDTH => Ok(platform::DeviceInfo::Image2dMaxWidth),
        consts::CL_DEVICE_IMAGE2D_MAX_HEIGHT => Ok(platform::DeviceInfo::Image2dMaxHeight),
        consts::CL_DEVICE_IMAGE3D_MAX_WIDTH => Ok(platform::DeviceInfo::Image3dMaxWidth),
        consts::CL_DEVICE_IMAGE3D_MAX_HEIGHT => Ok(platform::DeviceInfo::Image3dMaxHeight),
        consts::CL_DEVICE_IMAGE3D_MAX_DEPTH => Ok(platform::DeviceInfo::Image3dMaxDepth),
        consts::CL_DEVICE_IMAGE_MAX_BUFFER_SIZE => Ok(platform::DeviceInfo::ImageMaxBufferSize),
        consts::CL_DEVICE_IMAGE_MAX_ARRAY_SIZE => Ok(platform::DeviceInfo::ImageMaxArraySize),
        consts::CL_DEVICE_IMAGE_SUPPORT => Ok(platform::DeviceInfo::ImageSupport),
        consts::CL_DEVICE_MAX_PARAMETER_SIZE => Ok(platform::DeviceInfo::MaxParameterSize),
        consts::CL_DEVICE_MAX_SAMPLERS => Ok(platform::DeviceInfo::MaxSamplers),
        consts::CL_DEVICE_MEM_BASE_ADDR_ALIGN => Ok(platform::DeviceInfo::MemBaseAddrAlign),
        consts::CL_DEVICE_MIN_DATA_TYPE_ALIGN_SIZE => {
            Ok(platform::DeviceInfo::MinDataTypeAlignSize)
        }
        consts::CL_DEVICE_SINGLE_FP_CONFIG => Ok(platform::DeviceInfo::SingleFpConfig),
        consts::CL_DEVICE_DOUBLE_FP_CONFIG => Ok(platform::DeviceInfo::DoubleFpConfig),
        consts::CL_DEVICE_GLOBAL_MEM_CACHE_TYPE => Ok(platform::DeviceInfo::GlobalMemCacheType),
        consts::CL_DEVICE_GLOBAL_MEM_CACHELINE_SIZE => {
            Ok(platform::DeviceInfo::GlobalMemCachelineSize)
        }
        consts::CL_DEVICE_GLOBAL_MEM_CACHE_SIZE => Ok(platform::DeviceInfo::GlobalMemCacheSize),
        consts::CL_DEVICE_GLOBAL_MEM_SIZE => Ok(platform::DeviceInfo::GlobalMemSize),
        consts::CL_DEVICE_MAX_CONSTANT_BUFFER_SIZE => {
            Ok(platform::DeviceInfo::MaxConstantBufferSize)
        }
        consts::CL_DEVICE_MAX_CONSTANT_ARGS => Ok(platform::DeviceInfo::MaxConstantArgs),
        consts::CL_DEVICE_LOCAL_MEM_TYPE => Ok(platform::DeviceInfo::LocalMemType),
        consts::CL_DEVICE_LOCAL_MEM_SIZE => Ok(platform::DeviceInfo::LocalMemSize),
        consts::CL_DEVICE_ERROR_CORRECTION_SUPPORT => {
            Ok(platform::DeviceInfo::ErrorCorrectionSupport)
        }
        consts::CL_DEVICE_PROFILING_TIMER_RESOLUTION => {
            Ok(platform::DeviceInfo::ProfilingTimerResolution)
        }
        consts::CL_DEVICE_ENDIAN_LITTLE => Ok(platform::DeviceInfo::EndianLittle),
        consts::CL_DEVICE_AVAILABLE => Ok(platform::DeviceInfo::Available),
        consts::CL_DEVICE_COMPILER_AVAILABLE => Ok(platform::DeviceInfo::CompilerAvailable),
        consts::CL_DEVICE_LINKER_AVAILABLE => Ok(platform::DeviceInfo::LinkerAvailable),
        consts::CL_DEVICE_EXECUTION_CAPABILITIES => Ok(platform::DeviceInfo::ExecutionCapabilities),
        consts::CL_DEVICE_QUEUE_PROPERTIES => Ok(platform::DeviceInfo::QueueProperties),
        consts::CL_DEVICE_HOST_UNIFIED_MEMORY => Ok(platform::DeviceInfo::HostUnifiedMemory),
        consts::CL_DEVICE_PLATFORM => Ok(platform::DeviceInfo::Platform),
        consts::CL_DEVICE_PARENT_DEVICE => Ok(platform::DeviceInfo::ParentDevice),
        consts::CL_DEVICE_PARTITION_MAX_SUB_DEVICES => {
            Ok(platform::DeviceInfo::PartitionMaxSubDevices)
        }
        consts::CL_DEVICE_PARTITION_PROPERTIES => Ok(platform::DeviceInfo::PartitionProperties),
        consts::CL_DEVICE_PARTITION_AFFINITY_DOMAIN => {
            Ok(platform::DeviceInfo::PartitionAffinityDomain)
        }
        consts::CL_DEVICE_PARTITION_TYPE => Ok(platform::DeviceInfo::PartitionType),
        consts::CL_DEVICE_REFERENCE_COUNT => Ok(platform::DeviceInfo::ReferenceCount),
        consts::CL_DEVICE_PREFERRED_INTEROP_USER_SYNC => {
            Ok(platform::DeviceInfo::PreferredInteropUserSync)
        }
        consts::CL_DEVICE_PRINTF_BUFFER_SIZE => Ok(platform::DeviceInfo::PrintfBufferSize),
        consts::CL_DEVICE_NAME => Ok(platform::DeviceInfo::Name),
        consts::CL_DEVICE_VENDOR => Ok(platform::DeviceInfo::Vendor),
        consts::CL_DRIVER_VERSION => Ok(platform::DeviceInfo::DriverVersion),
        consts::CL_DEVICE_PROFILE => Ok(platform::DeviceInfo::Profile),
        consts::CL_DEVICE_VERSION => Ok(platform::DeviceInfo::Version),
        consts::CL_DEVICE_OPENCL_C_VERSION => Ok(platform::DeviceInfo::OpenclCVersion),
        consts::CL_DEVICE_EXTENSIONS => Ok(platform::DeviceInfo::Extensions),
        consts::CL_DEVICE_BUILT_IN_KERNELS => Ok(platform::DeviceInfo::BuiltInKernels),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn context_info(raw: sys::cl_context_info) -> platform::Result<platform::ContextInfo> {
    match raw {
        consts::CL_CONTEXT_REFERENCE_COUNT => Ok(platform::ContextInfo::ReferenceCount),
        consts::CL_CONTEXT_NUM_DEVICES => Ok(platform::ContextInfo::NumDevices),
        consts::CL_CONTEXT_DEVICES => Ok(platform::ContextInfo::Devices),
        consts::CL_CONTEXT_PROPERTIES => Ok(platform::ContextInfo::Properties),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn queue_info(raw: sys::cl_command_queue_info) -> platform::Result<platform::QueueInfo> {
    match raw {
        consts::CL_QUEUE_CONTEXT => Ok(platform::QueueInfo::Context),
        consts::CL_QUEUE_DEVICE => Ok(platform::QueueInfo::Device),
        consts::CL_QUEUE_REFERENCE_COUNT => Ok(platform::QueueInfo::ReferenceCount),
        consts::CL_QUEUE_PROPERTIES => Ok(platform::QueueInfo::Properties),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn mem_info(raw: sys::cl_mem_info) -> platform::Result<platform::MemInfo> {
    match raw {
        consts::CL_MEM_TYPE => Ok(platform::MemInfo::Type),
        consts::CL_MEM_FLAGS => Ok(platform::MemInfo::Flags),
        consts::CL_MEM_SIZE => Ok(platform::MemInfo::Size),
        consts::CL_MEM_HOST_PTR => Ok(platform::MemInfo::HostPtr),
        consts::CL_MEM_MAP_COUNT => Ok(platform::MemInfo::MapCount),
        consts::CL_MEM_REFERENCE_COUNT => Ok(platform::MemInfo::ReferenceCount),
        consts::CL_MEM_CONTEXT => Ok(platform::MemInfo::Context),
        consts::CL_MEM_ASSOCIATED_MEMOBJECT => Ok(platform::MemInfo::AssociatedMemObject),
        consts::CL_MEM_OFFSET => Ok(platform::MemInfo::Offset),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn event_info(raw: sys::cl_event_info) -> platform::Result<platform::EventInfo> {
    match raw {
        consts::CL_EVENT_COMMAND_QUEUE => Ok(platform::EventInfo::CommandQueue),
        consts::CL_EVENT_CONTEXT => Ok(platform::EventInfo::Context),
        consts::CL_EVENT_COMMAND_TYPE => Ok(platform::EventInfo::CommandType),
        consts::CL_EVENT_COMMAND_EXECUTION_STATUS => Ok(platform::EventInfo::ExecutionStatus),
        consts::CL_EVENT_REFERENCE_COUNT => Ok(platform::EventInfo::ReferenceCount),
        _ => Err(platform::Error::InvalidValue),
    }
}

fn bits(flags: &[(bool, sys::cl_bitfield)]) -> sys::cl_bitfield {
    flags
        .iter()
        .filter(|(set, _)| *set)
        .fold(0, |raw, (_, bit)| raw | bit)
}

pub(crate) fn program_info(raw: sys::cl_program_info) -> platform::Result<platform::ProgramInfo> {
    match raw {
        consts::CL_PROGRAM_REFERENCE_COUNT => Ok(platform::ProgramInfo::ReferenceCount),
        consts::CL_PROGRAM_CONTEXT => Ok(platform::ProgramInfo::Context),
        consts::CL_PROGRAM_NUM_DEVICES => Ok(platform::ProgramInfo::NumDevices),
        consts::CL_PROGRAM_DEVICES => Ok(platform::ProgramInfo::Devices),
        consts::CL_PROGRAM_SOURCE => Ok(platform::ProgramInfo::Source),
        consts::CL_PROGRAM_BINARY_SIZES => Ok(platform::ProgramInfo::BinarySizes),
        consts::CL_PROGRAM_BINARIES => Ok(platform::ProgramInfo::Binaries),
        consts::CL_PROGRAM_NUM_KERNELS => Ok(platform::ProgramInfo::NumKernels),
        consts::CL_PROGRAM_KERNEL_NAMES => Ok(platform::ProgramInfo::KernelNames),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn program_build_info(
    raw: sys::cl_program_build_info,
) -> platform::Result<platform::ProgramBuildInfo> {
    match raw {
        consts::CL_PROGRAM_BUILD_STATUS => Ok(platform::ProgramBuildInfo::Status),
        consts::CL_PROGRAM_BUILD_OPTIONS => Ok(platform::ProgramBuildInfo::Options),
        consts::CL_PROGRAM_BUILD_LOG => Ok(platform::ProgramBuildInfo::Log),
        consts::CL_PROGRAM_BINARY_TYPE => Ok(platform::ProgramBuildInfo::BinaryType),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn kernel_info(raw: sys::cl_kernel_info) -> platform::Result<platform::KernelInfo> {
    match raw {
        consts::CL_KERNEL_FUNCTION_NAME => Ok(platform::KernelInfo::FunctionName),
        consts::CL_KERNEL_NUM_ARGS => Ok(platform::KernelInfo::NumArgs),
        consts::CL_KERNEL_REFERENCE_COUNT => Ok(platform::KernelInfo::ReferenceCount),
        consts::CL_KERNEL_CONTEXT => Ok(platform::KernelInfo::Context),
        consts::CL_KERNEL_PROGRAM => Ok(platform::KernelInfo::Program),
        consts::CL_KERNEL_ATTRIBUTES => Ok(platform::KernelInfo::Attributes),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn kernel_work_group_info(
    raw: sys::cl_kernel_work_group_info,
) -> platform::Result<platform::KernelWorkGroupInfo> {
    match raw {
        consts::CL_KERNEL_WORK_GROUP_SIZE => Ok(platform::KernelWorkGroupInfo::WorkGroupSize),
        consts::CL_KERNEL_COMPILE_WORK_GROUP_SIZE => {
            Ok(platform::KernelWorkGroupInfo::CompileWorkGroupSize)
        }
        consts::CL_KERNEL_LOCAL_MEM_SIZE => Ok(platform::KernelWorkGroupInfo::LocalMemSize),
        consts::CL_KERNEL_PREFERRED_WORK_GROUP_SIZE_MULTIPLE => {
            Ok(platform::KernelWorkGroupInfo::PreferredWorkGroupSizeMultiple)
        }
        consts::CL_KERNEL_PRIVATE_MEM_SIZE => Ok(platform::KernelWorkGroupInfo::PrivateMemSize),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) fn code(error: platform::Error) -> sys::cl_int {
    match error {
        platform::Error::DeviceNotFound => consts::CL_DEVICE_NOT_FOUND,
        platform::Error::MemCopyOverlap => consts::CL_MEM_COPY_OVERLAP,
        platform::Error::MisalignedSubBufferOffset => consts::CL_MISALIGNED_SUB_BUFFER_OFFSET,
        platform::Error::ExecStatusErrorForEventsInWaitList => {
            consts::CL_EXEC_STATUS_ERROR_FOR_EVENTS_IN_WAIT_LIST
        }
        platform::Error::InvalidValue => consts::CL_INVALID_VALUE,
        platform::Error::InvalidDeviceType => consts::CL_INVALID_DEVICE_TYPE,
        platform::Error::InvalidPlatform => consts::CL_INVALID_PLATFORM,
        platform::Error::InvalidDevice => consts::CL_INVALID_DEVICE,
        platform::Error::InvalidContext => consts::CL_INVALID_CONTEXT,
        platform::Error::InvalidQueueProperties => consts::CL_INVALID_QUEUE_PROPERTIES,
        platform::Error::InvalidCommandQueue => consts::CL_INVALID_COMMAND_QUEUE,
        platform::Error::InvalidHostPtr => consts::CL_INVALID_HOST_PTR,
        platform::Error::InvalidMemObject => consts::CL_INVALID_MEM_OBJECT,
        platform::Error::InvalidEventWaitList => consts::CL_INVALID_EVENT_WAIT_LIST,
        platform::Error::InvalidEvent => consts::CL_INVALID_EVENT,
        platform::Error::InvalidOperation => consts::CL_INVALID_OPERATION,
        platform::Error::InvalidBufferSize => consts::CL_INVALID_BUFFER_SIZE,
        platform::Error::InvalidProperty => consts::CL_INVALID_PROPERTY,
        platform::Error::CompilerNotAvailable => consts::CL_COMPILER_NOT_AVAILABLE,
        platform::Error::LinkerNotAvailable => consts::CL_LINKER_NOT_AVAILABLE,
        platform::Error::OutOfResources => consts::CL_OUT_OF_RESOURCES,
        platform::Error::ProfilingInfoNotAvailable => consts::CL_PROFILING_INFO_NOT_AVAILABLE,
        platform::Error::BuildProgramFailure => consts::CL_BUILD_PROGRAM_FAILURE,
        platform::Error::CompileProgramFailure => consts::CL_COMPILE_PROGRAM_FAILURE,
        platform::Error::LinkProgramFailure => consts::CL_LINK_PROGRAM_FAILURE,
        platform::Error::KernelArgInfoNotAvailable => consts::CL_KERNEL_ARG_INFO_NOT_AVAILABLE,
        platform::Error::InvalidBinary => consts::CL_INVALID_BINARY,
        platform::Error::InvalidProgram => consts::CL_INVALID_PROGRAM,
        platform::Error::InvalidProgramExecutable => consts::CL_INVALID_PROGRAM_EXECUTABLE,
        platform::Error::InvalidKernelName => consts::CL_INVALID_KERNEL_NAME,
        platform::Error::InvalidKernelDefinition => consts::CL_INVALID_KERNEL_DEFINITION,
        platform::Error::InvalidKernel => consts::CL_INVALID_KERNEL,
        platform::Error::InvalidArgIndex => consts::CL_INVALID_ARG_INDEX,
        platform::Error::InvalidArgValue => consts::CL_INVALID_ARG_VALUE,
        platform::Error::InvalidArgSize => consts::CL_INVALID_ARG_SIZE,
        platform::Error::InvalidKernelArgs => consts::CL_INVALID_KERNEL_ARGS,
        platform::Error::InvalidWorkDimension => consts::CL_INVALID_WORK_DIMENSION,
        platform::Error::InvalidWorkGroupSize => consts::CL_INVALID_WORK_GROUP_SIZE,
        platform::Error::InvalidWorkItemSize => consts::CL_INVALID_WORK_ITEM_SIZE,
        platform::Error::InvalidGlobalOffset => consts::CL_INVALID_GLOBAL_OFFSET,
        platform::Error::InvalidGlobalWorkSize => consts::CL_INVALID_GLOBAL_WORK_SIZE,
        platform::Error::InvalidSampler => consts::CL_INVALID_SAMPLER,
    }
}

pub(crate) fn status(result: platform::Result<()>) -> sys::cl_int {
    match result {
        Ok(()) => consts::CL_SUCCESS,
        Err(error) => code(error),
    }
}

pub(crate) unsafe fn write_code(out: *mut sys::cl_int, value: sys::cl_int) {
    if !out.is_null() {
        unsafe { *out = value };
    }
}

pub(crate) unsafe fn object<T>(
    result: platform::Result<u32>,
    errcode_ret: *mut sys::cl_int,
) -> *mut T {
    match result {
        Ok(id) => {
            unsafe { write_code(errcode_ret, consts::CL_SUCCESS) };

            object_handle(id)
        }
        Err(error) => {
            unsafe { write_code(errcode_ret, code(error)) };

            core::ptr::null_mut()
        }
    }
}

pub(crate) unsafe fn write_handles<T>(
    values: &[T],
    num_entries: sys::cl_uint,
    out: *mut T,
    num_out: *mut sys::cl_uint,
) -> sys::cl_int
where
    T: Copy,
{
    if (out.is_null() && num_out.is_null()) || (!out.is_null() && num_entries == 0) {
        return consts::CL_INVALID_VALUE;
    }

    if !out.is_null() {
        let taken = values.len().min(num_entries as usize);

        unsafe { core::ptr::copy_nonoverlapping(values.as_ptr(), out, taken) };
    }

    if !num_out.is_null() {
        unsafe { *num_out = values.len() as sys::cl_uint };
    }

    consts::CL_SUCCESS
}

pub(crate) unsafe fn kernel_argument(
    kernel: platform::KernelId,
    index: sys::cl_uint,
    arg_size: usize,
    arg_value: *const core::ffi::c_void,
) -> platform::Result<platform::KernelArgument> {
    match platform::Kernel::argument_kind(kernel, index)? {
        platform::ArgumentKind::Local => {
            if !arg_value.is_null() {
                return Err(platform::Error::InvalidArgValue);
            }

            Ok(platform::KernelArgument::Local(arg_size))
        }
        platform::ArgumentKind::Global | platform::ArgumentKind::Constant => {
            if arg_size != core::mem::size_of::<sys::cl_mem>() {
                return Err(platform::Error::InvalidArgSize);
            }

            if arg_value.is_null() {
                return Ok(platform::KernelArgument::Memory(None));
            }

            let handle = unsafe { *arg_value.cast::<sys::cl_mem>() };
            if handle.is_null() {
                return Ok(platform::KernelArgument::Memory(None));
            }

            Ok(platform::KernelArgument::Memory(Some(buffer_id(handle))))
        }
        platform::ArgumentKind::Value(expected) => {
            if arg_size != expected || arg_value.is_null() {
                return Err(platform::Error::InvalidArgSize);
            }

            Ok(platform::KernelArgument::Value(
                unsafe { core::slice::from_raw_parts(arg_value.cast::<u8>(), arg_size) }.to_vec(),
            ))
        }
    }
}

pub(crate) unsafe fn created(
    kernels: platform::Result<Vec<platform::KernelId>>,
    num_kernels: sys::cl_uint,
    out: *mut sys::cl_kernel,
    num_out: *mut sys::cl_uint,
) -> sys::cl_int {
    let kernels = match kernels {
        Ok(kernels) => kernels,
        Err(error) => return code(error),
    };

    if !out.is_null() && (num_kernels as usize) < kernels.len() {
        return consts::CL_INVALID_VALUE;
    }

    let listed: Vec<sys::cl_kernel> = kernels
        .iter()
        .map(|id| object_handle(id.into_raw()))
        .collect();

    if out.is_null() {
        for id in kernels {
            let _released = platform::Kernel::release(id);
        }
    }

    unsafe { write_handles(&listed, num_kernels, out, num_out) }
}

pub(crate) fn profiling_info(
    raw: sys::cl_profiling_info,
) -> platform::Result<platform::ProfilingInfo> {
    match raw {
        consts::CL_PROFILING_COMMAND_QUEUED => Ok(platform::ProfilingInfo::Queued),
        consts::CL_PROFILING_COMMAND_SUBMIT => Ok(platform::ProfilingInfo::Submit),
        consts::CL_PROFILING_COMMAND_START => Ok(platform::ProfilingInfo::Start),
        consts::CL_PROFILING_COMMAND_END => Ok(platform::ProfilingInfo::End),
        _ => Err(platform::Error::InvalidValue),
    }
}

pub(crate) unsafe fn linked(
    result: core::result::Result<platform::ProgramId, platform::LinkFailure>,
    errcode_ret: *mut sys::cl_int,
) -> sys::cl_program {
    match result {
        Ok(id) => {
            unsafe { write_code(errcode_ret, consts::CL_SUCCESS) };

            object_handle(id.into_raw())
        }
        Err(failure) => {
            unsafe { write_code(errcode_ret, code(failure.error)) };

            failure
                .program
                .map_or(core::ptr::null_mut(), |id| object_handle(id.into_raw()))
        }
    }
}

pub(crate) unsafe fn found(
    devices: platform::Result<Vec<platform::Device>>,
    num_entries: sys::cl_uint,
    out: *mut sys::cl_device_id,
    num_out: *mut sys::cl_uint,
) -> sys::cl_int {
    let devices = match devices {
        Ok(devices) => devices,
        Err(error) => return code(error),
    };

    let listed: Vec<sys::cl_device_id> = devices.into_iter().map(device_handle).collect();

    unsafe { write_handles(&listed, num_entries, out, num_out) }
}

pub(crate) unsafe fn enqueue(
    blocking: sys::cl_bool,
    event: *mut sys::cl_event,
    queued: impl FnOnce() -> platform::Result<platform::EventId>,
) -> sys::cl_int {
    let id = match queued() {
        Ok(id) => id,
        Err(error) => return code(error),
    };

    let waited = if blocking == consts::CL_TRUE {
        status(platform::Event::wait(&[id]))
    } else {
        consts::CL_SUCCESS
    };

    unsafe { deliver_event(id, event) };

    waited
}

pub(crate) unsafe fn map(
    blocking: sys::cl_bool,
    event: *mut sys::cl_event,
    errcode_ret: *mut sys::cl_int,
    mapped: impl FnOnce() -> platform::Result<(platform::SharedMemoryPointer, platform::EventId)>,
) -> *mut core::ffi::c_void {
    let (pointer, id) = match mapped() {
        Ok(mapped) => mapped,
        Err(error) => {
            unsafe { write_code(errcode_ret, code(error)) };

            return core::ptr::null_mut();
        }
    };

    let waited = if blocking == consts::CL_TRUE {
        status(platform::Event::wait(&[id]))
    } else {
        consts::CL_SUCCESS
    };

    unsafe { deliver_event(id, event) };
    unsafe { write_code(errcode_ret, waited) };

    if waited == consts::CL_SUCCESS {
        pointer.as_ptr().cast()
    } else {
        core::ptr::null_mut()
    }
}

unsafe fn deliver_event(id: platform::EventId, event: *mut sys::cl_event) {
    if event.is_null() {
        let _released = platform::Event::release(id);
    } else {
        unsafe { *event = object_handle(id.into_raw()) };
    }
}

pub(crate) unsafe fn info(
    value: platform::Result<platform::InfoValue>,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    let value = match value {
        Ok(value) => value,
        Err(error) => return code(error),
    };

    unsafe { write_info(value, param_value_size, param_value, param_value_size_ret) }
}

unsafe fn write_info(
    value: platform::InfoValue,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        match value {
            platform::InfoValue::Bool(v) => scalar(
                v as sys::cl_bool,
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Uint(v) => {
                scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Ulong(v) => {
                scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Size(v) => {
                scalar(v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Sizes(v) => {
                slice(&v, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Text(v) => {
                let mut text = v.as_bytes().to_vec();
                text.push(0);

                bytes(&text, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Platform => scalar(
                platform_handle(),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Device(v) => scalar(
                v.map_or(core::ptr::null_mut(), device_handle),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Devices(v) => {
                let handles: Vec<sys::cl_device_id> = v.into_iter().map(device_handle).collect();

                slice(
                    &handles,
                    param_value_size,
                    param_value,
                    param_value_size_ret,
                )
            }
            platform::InfoValue::DeviceType(v) => {
                let raw = bits(&[
                    (v.cpu, consts::CL_DEVICE_TYPE_CPU),
                    (v.gpu, consts::CL_DEVICE_TYPE_GPU),
                    (v.accelerator, consts::CL_DEVICE_TYPE_ACCELERATOR),
                    (v.custom, consts::CL_DEVICE_TYPE_CUSTOM),
                    (v.default, consts::CL_DEVICE_TYPE_DEFAULT),
                ]);

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::MemCacheType(v) => {
                let raw = match v {
                    platform::MemCacheType::None => consts::CL_NONE,
                    platform::MemCacheType::ReadOnly => consts::CL_READ_ONLY_CACHE,
                    platform::MemCacheType::ReadWrite => consts::CL_READ_WRITE_CACHE,
                };

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::LocalMemType(v) => {
                let raw = match v {
                    platform::LocalMemType::None => consts::CL_NONE,
                    platform::LocalMemType::Local => consts::CL_LOCAL,
                    platform::LocalMemType::Global => consts::CL_GLOBAL,
                };

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::FpConfig(v) => {
                let raw = bits(&[
                    (v.denorm, consts::CL_FP_DENORM),
                    (v.inf_nan, consts::CL_FP_INF_NAN),
                    (v.round_to_nearest, consts::CL_FP_ROUND_TO_NEAREST),
                    (v.round_to_zero, consts::CL_FP_ROUND_TO_ZERO),
                    (v.round_to_inf, consts::CL_FP_ROUND_TO_INF),
                    (v.fma, consts::CL_FP_FMA),
                    (v.soft_float, consts::CL_FP_SOFT_FLOAT),
                ]);

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::ExecCapabilities(v) => {
                let raw = bits(&[
                    (v.kernel, consts::CL_EXEC_KERNEL),
                    (v.native_kernel, consts::CL_EXEC_NATIVE_KERNEL),
                ]);

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::QueueProperties(v) => scalar(
                encode_queue_properties(v),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::ContextProperties(v) => slice(
                &encode_context_properties(v),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::PartitionProperties(v) => {
                let raw: Vec<sys::cl_device_partition_property> = v.iter().map(|()| 0).collect();

                slice(&raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::AffinityDomain => scalar(
                0 as sys::cl_device_affinity_domain,
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::MemObjectType(v) => {
                let raw = match v {
                    platform::MemObjectType::Buffer => consts::CL_MEM_OBJECT_BUFFER,
                };

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::MemFlags(v) => scalar(
                encode_mem_flags(v),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::CommandType(v) => scalar(
                encode_command_type(v),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Status(v) => scalar(
                encode_status(v),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::HostPointer(v) => scalar(
                v.as_ptr().cast::<core::ffi::c_void>(),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Context(v) => scalar(
                object_handle::<sys::_cl_context>(v.into_raw()),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Queue(v) => scalar(
                v.map_or(core::ptr::null_mut(), |id| {
                    object_handle::<sys::_cl_command_queue>(id.into_raw())
                }),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Binaries(v) => {
                let pointers: Vec<*const u8> = v.iter().map(|binary| binary.as_ptr()).collect();

                if !param_value.is_null() {
                    let out = core::slice::from_raw_parts(
                        param_value.cast::<*mut u8>(),
                        v.len()
                            .min(param_value_size / core::mem::size_of::<*mut u8>()),
                    );

                    for (binary, into) in v.iter().zip(out) {
                        if !into.is_null() {
                            core::ptr::copy_nonoverlapping(binary.as_ptr(), *into, binary.len());
                        }
                    }
                }

                if !param_value_size_ret.is_null() {
                    *param_value_size_ret = core::mem::size_of_val(&pointers[..]);
                }

                consts::CL_SUCCESS
            }
            platform::InfoValue::BuildStatus(v) => {
                let raw = match v {
                    platform::BuildStatus::None => consts::CL_BUILD_NONE,
                    platform::BuildStatus::Error => consts::CL_BUILD_ERROR,
                    platform::BuildStatus::Success => consts::CL_BUILD_SUCCESS,
                    platform::BuildStatus::InProgress => consts::CL_BUILD_IN_PROGRESS,
                };

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::BinaryType(v) => {
                let raw = match v {
                    platform::BinaryType::None => consts::CL_PROGRAM_BINARY_TYPE_NONE,
                    platform::BinaryType::CompiledObject => {
                        consts::CL_PROGRAM_BINARY_TYPE_COMPILED_OBJECT
                    }
                    platform::BinaryType::Library => consts::CL_PROGRAM_BINARY_TYPE_LIBRARY,
                    platform::BinaryType::Executable => consts::CL_PROGRAM_BINARY_TYPE_EXECUTABLE,
                };

                scalar(raw, param_value_size, param_value, param_value_size_ret)
            }
            platform::InfoValue::Program(v) => scalar(
                object_handle::<sys::_cl_program>(v.into_raw()),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
            platform::InfoValue::Buffer(v) => scalar(
                v.map_or(core::ptr::null_mut(), |id| {
                    object_handle::<sys::_cl_mem>(id.into_raw())
                }),
                param_value_size,
                param_value,
                param_value_size_ret,
            ),
        }
    }
}

unsafe fn bytes(
    source: &[u8],
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    if !param_value.is_null() {
        if param_value_size < source.len() {
            return consts::CL_INVALID_VALUE;
        }

        unsafe {
            core::ptr::copy_nonoverlapping(source.as_ptr(), param_value.cast::<u8>(), source.len());
        }
    }

    if !param_value_size_ret.is_null() {
        unsafe { *param_value_size_ret = source.len() };
    }

    consts::CL_SUCCESS
}

unsafe fn slice<T>(
    values: &[T],
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    let raw = unsafe {
        core::slice::from_raw_parts(values.as_ptr().cast::<u8>(), core::mem::size_of_val(values))
    };

    unsafe { bytes(raw, param_value_size, param_value, param_value_size_ret) }
}

unsafe fn scalar<T>(
    value: T,
    param_value_size: usize,
    param_value: *mut core::ffi::c_void,
    param_value_size_ret: *mut usize,
) -> sys::cl_int {
    unsafe {
        slice(
            &[value],
            param_value_size,
            param_value,
            param_value_size_ret,
        )
    }
}
