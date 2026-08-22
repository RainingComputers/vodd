use crate::consts;
use crate::sys;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

static VODD_PLATFORM_TOKEN: u8 = 0xA0;
static VODD_DEVICE_TOKEN: u8 = 0xD0;
const VODD_DEVICE_TYPE: sys::cl_device_type = consts::CL_DEVICE_TYPE_GPU;

static NEXT_OBJECT_ID: AtomicU32 = AtomicU32::new(1);
static CONTEXTS: Mutex<BTreeMap<sys::cl_uint, VoddContext>> = Mutex::new(BTreeMap::new());
static QUEUES: Mutex<BTreeMap<sys::cl_uint, VoddCommandQueue>> = Mutex::new(BTreeMap::new());

fn next_object_id() -> sys::cl_uint {
    NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed)
}

pub enum InfoValue<'a> {
    Uint(sys::cl_uint),
    Ulong(sys::cl_ulong),
    Size(usize),
    Sizes(&'a [usize]),
    Handle(*mut core::ffi::c_void),
    Handles(&'a [sys::cl_device_id]),
    Text(&'a [u8]),
    Properties(&'a [sys::cl_device_partition_property]),
}

pub struct VoddPlatform {}

impl VoddPlatform {
    pub fn platform_id() -> sys::cl_platform_id {
        &raw const VODD_PLATFORM_TOKEN as *mut sys::_cl_platform_id
    }

    pub fn device_id() -> sys::cl_device_id {
        &raw const VODD_DEVICE_TOKEN as *mut sys::_cl_device_id
    }

    pub fn is_platform_id(platform: sys::cl_platform_id) -> bool {
        platform == Self::platform_id()
    }

    pub fn is_device_id(device: sys::cl_device_id) -> bool {
        device == Self::device_id()
    }

    pub fn is_valid_device_type(device_type: sys::cl_device_type) -> bool {
        let known = consts::CL_DEVICE_TYPE_DEFAULT
            | consts::CL_DEVICE_TYPE_CPU
            | consts::CL_DEVICE_TYPE_GPU
            | consts::CL_DEVICE_TYPE_ACCELERATOR
            | consts::CL_DEVICE_TYPE_CUSTOM;

        device_type == consts::CL_DEVICE_TYPE_ALL || (device_type != 0 && device_type & !known == 0)
    }

    pub fn platform_info(param_name: sys::cl_platform_info) -> Option<&'static [u8]> {
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

    pub fn device_info(param_name: sys::cl_device_info) -> Option<InfoValue<'static>> {
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

unsafe impl Send for VoddContext {}

pub struct VoddContext {
    reference_count: sys::cl_uint,
    devices: Vec<sys::cl_device_id>,
    properties: Vec<sys::cl_context_properties>,
    notify: Option<sys::cl_context_callback>,
    user_data: *mut core::ffi::c_void,
}

impl VoddContext {
    pub fn create(
        properties: Vec<sys::cl_context_properties>,
        devices: Vec<sys::cl_device_id>,
        notify: Option<sys::cl_context_callback>,
        user_data: *mut core::ffi::c_void,
    ) -> sys::cl_uint {
        let id = next_object_id();

        CONTEXTS.lock().expect("contexts").insert(
            id,
            Self { reference_count: 1, devices, properties, notify, user_data },
        );

        id
    }

    pub fn retain(id: sys::cl_uint) -> bool {
        CONTEXTS
            .lock()
            .expect("contexts")
            .get_mut(&id)
            .map(|context| context.reference_count += 1)
            .is_some()
    }

    pub fn release(id: sys::cl_uint) -> Option<bool> {
        let mut contexts = CONTEXTS.lock().expect("contexts");
        let context = contexts.get_mut(&id)?;

        context.reference_count -= 1;

        if context.reference_count == 0 {
            contexts.remove(&id);
            return Some(true);
        }

        Some(false)
    }

    pub fn with<T>(id: sys::cl_uint, action: impl FnOnce(&VoddContext) -> T) -> Option<T> {
        CONTEXTS.lock().expect("contexts").get(&id).map(action)
    }

    pub fn has_device(&self, device: sys::cl_device_id) -> bool {
        self.devices.contains(&device)
    }

    pub fn info(&self, param_name: sys::cl_context_info) -> Option<InfoValue<'_>> {
        match param_name {
            consts::CL_CONTEXT_REFERENCE_COUNT => Some(InfoValue::Uint(self.reference_count)),
            consts::CL_CONTEXT_NUM_DEVICES => {
                Some(InfoValue::Uint(self.devices.len() as sys::cl_uint))
            }
            consts::CL_CONTEXT_DEVICES => Some(InfoValue::Handles(&self.devices)),
            consts::CL_CONTEXT_PROPERTIES => Some(InfoValue::Properties(&self.properties)),
            _ => None,
        }
    }

    pub fn validate_properties(
        properties: &[sys::cl_context_properties],
    ) -> Result<(), sys::cl_int> {
        let listed = properties.strip_suffix(&[0]).unwrap_or(properties);
        let mut pairs = listed.chunks_exact(2);

        if !pairs.remainder().is_empty() {
            return Err(consts::CL_INVALID_PROPERTY);
        }

        let named: Vec<sys::cl_context_properties> = pairs.clone().map(|pair| pair[0]).collect();

        if named
            .iter()
            .enumerate()
            .any(|(index, name)| named[..index].contains(name))
        {
            return Err(consts::CL_INVALID_PROPERTY);
        }

        pairs.try_for_each(|pair| Self::validate_property(pair[0], pair[1]))
    }

    fn validate_property(
        name: sys::cl_context_properties,
        value: sys::cl_context_properties,
    ) -> Result<(), sys::cl_int> {
        match name {
            consts::CL_CONTEXT_PLATFORM => {
                VoddPlatform::is_platform_id(value as sys::cl_platform_id)
                    .then_some(())
                    .ok_or(consts::CL_INVALID_PLATFORM)
            }
            consts::CL_CONTEXT_INTEROP_USER_SYNC => {
                matches!(value as sys::cl_bool, consts::CL_FALSE | consts::CL_TRUE)
                    .then_some(())
                    .ok_or(consts::CL_INVALID_PROPERTY)
            }
            _ => Err(consts::CL_INVALID_PROPERTY),
        }
    }

    pub fn select_devices(
        devices: &[sys::cl_device_id],
    ) -> Result<Vec<sys::cl_device_id>, sys::cl_int> {
        let mut selected: Vec<sys::cl_device_id> = Vec::new();

        for device in devices {
            if !VoddPlatform::is_device_id(*device) {
                return Err(consts::CL_INVALID_DEVICE);
            }

            if !selected.contains(device) {
                selected.push(*device);
            }
        }

        Ok(selected)
    }

    pub fn devices_of_type(device_type: sys::cl_device_type) -> Vec<sys::cl_device_id> {
        if VoddPlatform::is_valid_device_type(device_type)
            && (device_type & VODD_DEVICE_TYPE != 0
                || device_type == consts::CL_DEVICE_TYPE_DEFAULT)
        {
            vec![VoddPlatform::device_id()]
        } else {
            Vec::new()
        }
    }

    pub unsafe fn notify(&self, errinfo: *const core::ffi::c_char) {
        if let Some(notify) = self.notify {
            unsafe { notify(errinfo, core::ptr::null(), 0, self.user_data) };
        }
    }
}

pub struct Command {}

unsafe impl Send for VoddCommandQueue {}

pub struct VoddCommandQueue {
    reference_count: sys::cl_uint,
    context: sys::cl_context,
    device: sys::cl_device_id,
    properties: sys::cl_command_queue_properties,
    commands: VecDeque<Command>,
}

impl VoddCommandQueue {
    pub fn create(
        context: sys::cl_context,
        device: sys::cl_device_id,
        properties: sys::cl_command_queue_properties,
    ) -> sys::cl_uint {
        let id = next_object_id();

        QUEUES.lock().expect("queues").insert(
            id,
            Self {
                reference_count: 1,
                context,
                device,
                properties,
                commands: VecDeque::new(),
            },
        );

        id
    }

    pub fn retain(id: sys::cl_uint) -> bool {
        QUEUES
            .lock()
            .expect("queues")
            .get_mut(&id)
            .map(|queue| queue.reference_count += 1)
            .is_some()
    }

    pub fn release(id: sys::cl_uint) -> Option<Option<sys::cl_context>> {
        let mut queues = QUEUES.lock().expect("queues");
        let queue = queues.get_mut(&id)?;

        queue.reference_count -= 1;

        if queue.reference_count == 0 {
            let context = queue.context;
            queues.remove(&id);

            return Some(Some(context));
        }

        Some(None)
    }

    pub fn with<T>(id: sys::cl_uint, action: impl FnOnce(&VoddCommandQueue) -> T) -> Option<T> {
        QUEUES.lock().expect("queues").get(&id).map(action)
    }

    pub fn push(id: sys::cl_uint, command: Command) -> bool {
        QUEUES
            .lock()
            .expect("queues")
            .get_mut(&id)
            .map(|queue| queue.commands.push_back(command))
            .is_some()
    }

    pub fn pop(id: sys::cl_uint) -> Option<Command> {
        QUEUES
            .lock()
            .expect("queues")
            .get_mut(&id)?
            .commands
            .pop_front()
    }

    pub fn validate_properties(
        properties: sys::cl_command_queue_properties,
    ) -> Result<(), sys::cl_int> {
        let known =
            consts::CL_QUEUE_OUT_OF_ORDER_EXEC_MODE_ENABLE | consts::CL_QUEUE_PROFILING_ENABLE;

        if properties & !known != 0 {
            return Err(consts::CL_INVALID_VALUE);
        }

        let Some(InfoValue::Ulong(supported)) =
            VoddPlatform::device_info(consts::CL_DEVICE_QUEUE_PROPERTIES)
        else {
            return Err(consts::CL_INVALID_QUEUE_PROPERTIES);
        };

        if properties & !supported != 0 {
            return Err(consts::CL_INVALID_QUEUE_PROPERTIES);
        }

        Ok(())
    }

    pub fn info(&self, param_name: sys::cl_command_queue_info) -> Option<InfoValue<'_>> {
        match param_name {
            consts::CL_QUEUE_CONTEXT => Some(InfoValue::Handle(self.context.cast())),
            consts::CL_QUEUE_DEVICE => Some(InfoValue::Handle(self.device.cast())),
            consts::CL_QUEUE_REFERENCE_COUNT => Some(InfoValue::Uint(self.reference_count)),
            consts::CL_QUEUE_PROPERTIES => Some(InfoValue::Ulong(self.properties)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> sys::cl_uint {
        VoddCommandQueue::create(core::ptr::null_mut(), VoddPlatform::device_id(), 0)
    }

    #[test]
    fn commands_are_pushed_and_popped() {
        let id = queue();

        assert!(VoddCommandQueue::pop(id).is_none());
        assert!(VoddCommandQueue::push(id, Command {}));
        assert!(VoddCommandQueue::push(id, Command {}));
        assert!(VoddCommandQueue::pop(id).is_some());
        assert!(VoddCommandQueue::pop(id).is_some());
        assert!(VoddCommandQueue::pop(id).is_none());
    }

    #[test]
    fn pushing_to_an_unknown_queue_fails() {
        assert!(!VoddCommandQueue::push(0, Command {}));
        assert!(VoddCommandQueue::pop(0).is_none());
    }

    #[test]
    fn commands_survive_concurrent_pushes() {
        let id = queue();
        let threads = 8;
        let per_thread = 2000;

        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    for _ in 0..per_thread {
                        assert!(VoddCommandQueue::push(id, Command {}));
                    }
                });
            }
        });

        let popped = core::iter::from_fn(|| VoddCommandQueue::pop(id)).count();

        assert_eq!(popped, threads * per_thread);
    }
}
