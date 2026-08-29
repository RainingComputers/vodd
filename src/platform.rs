use crate::bitcode;
use crate::compiler;
use crate::interpreter;
use crate::parser;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

const BINARY_MAGIC: &[u8] = b"VODD";
const EXTENSIONS: &str = "cl_khr_byte_addressable_store cl_khr_global_int32_base_atomics cl_khr_global_int32_extended_atomics cl_khr_local_int32_base_atomics cl_khr_local_int32_extended_atomics cles_khr_int64";
const MAX_WORK_GROUP_SIZE: usize = 256;
const MAX_WORK_ITEM_SIZE: u64 = 256; // TODO: what does this mean?
const LOCAL_MEM_SIZE: u64 = 32 * 1024;
const GLOBAL_MEM_SIZE: u64 = 64 * 1024 * 1024;
const MAX_MEM_ALLOC_SIZE: u64 = GLOBAL_MEM_SIZE / 4;
const MEM_BASE_ADDR_ALIGN_BITS: u32 = 1024; // TODO: what does this mean?
const FUEL: usize = 1 << 28;

static NEXT_OBJECT_ID: AtomicU32 = AtomicU32::new(1);
static CONTEXTS: Mutex<BTreeMap<ContextId, Context>> = Mutex::new(BTreeMap::new());
static QUEUES: Mutex<BTreeMap<QueueId, CommandQueue>> = Mutex::new(BTreeMap::new());
static BUFFERS: Mutex<BTreeMap<BufferId, Buffer>> = Mutex::new(BTreeMap::new());
static EVENTS: Mutex<BTreeMap<EventId, Event>> = Mutex::new(BTreeMap::new());
static PROGRAMS: Mutex<BTreeMap<ProgramId, Program>> = Mutex::new(BTreeMap::new());
static KERNELS: Mutex<BTreeMap<KernelId, Kernel>> = Mutex::new(BTreeMap::new());

static PROGRESS: Mutex<u64> = Mutex::new(0);
static PROGRESSED: Condvar = Condvar::new();
static WORKER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn align_up(size: usize) -> usize {
    size.next_multiple_of(Device::MEM_BASE_ADDR_ALIGN)
}

fn next_object_id() -> u32 {
    NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed)
}

fn now() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_nanos() as u64)
}

fn progress() -> u64 {
    *PROGRESS.lock().expect("progress")
}

fn signal_progress() {
    *PROGRESS.lock().expect("progress") += 1;
    PROGRESSED.notify_all();
}

fn await_progress(generation: u64) {
    // TODO: how does this work
    let progress = PROGRESS.lock().expect("progress");
    let _settled = PROGRESSED
        .wait_while(progress, |current| *current == generation)
        .expect("progress");
}

// TODO: ensure all of these errors are used
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    DeviceNotFound,
    MemObjectAllocationFailure,
    OutOfHostMemory,
    MemCopyOverlap,
    MisalignedSubBufferOffset,
    ExecStatusErrorForEventsInWaitList,
    InvalidValue,
    InvalidDeviceType,
    InvalidPlatform,
    InvalidDevice,
    InvalidContext,
    InvalidQueueProperties,
    InvalidCommandQueue,
    InvalidHostPtr,
    InvalidMemObject,
    InvalidEventWaitList,
    InvalidEvent,
    InvalidOperation,
    InvalidBufferSize,
    InvalidProperty,
    CompilerNotAvailable,
    LinkerNotAvailable,
    OutOfResources,
    ProfilingInfoNotAvailable,
    BuildProgramFailure,
    CompileProgramFailure,
    LinkProgramFailure,
    KernelArgInfoNotAvailable,
    InvalidBinary,
    InvalidBuildOptions,
    InvalidCompilerOptions,
    InvalidLinkerOptions,
    InvalidProgram,
    InvalidProgramExecutable,
    InvalidKernelName,
    InvalidKernelDefinition,
    InvalidKernel,
    InvalidArgIndex,
    InvalidArgValue,
    InvalidArgSize,
    InvalidKernelArgs,
    InvalidWorkDimension,
    InvalidWorkGroupSize,
    InvalidWorkItemSize,
    InvalidGlobalOffset,
    InvalidGlobalWorkSize,
    InvalidSampler,
    InvalidImageSize,
    InvalidImageFormatDescriptor,
    ImageFormatNotSupported,
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ContextId(u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct QueueId(u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BufferId(u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct EventId(u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProgramId(u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct KernelId(u32);

macro_rules! object_id {
    ($name:ident) => {
        impl $name {
            pub fn from_raw(raw: u32) -> Self {
                Self(raw)
            }

            pub fn into_raw(self) -> u32 {
                self.0
            }
        }
    };
}

object_id!(ContextId);
object_id!(QueueId);
object_id!(BufferId);
object_id!(EventId);
object_id!(ProgramId);
object_id!(KernelId);

pub type Notify = Arc<dyn Fn(&str) + Send + Sync>; // TODO: rename this to ContextNotify
pub type EventNotify = Box<dyn FnOnce(EventId, Status) + Send>;
pub type DestructorNotify = Box<dyn FnOnce(BufferId) + Send>;
pub type ProgramNotify = Box<dyn FnOnce(ProgramId) + Send>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SharedMemoryPointer(*mut u8);

unsafe impl Send for SharedMemoryPointer {}

impl SharedMemoryPointer {
    pub const NULL: Self = Self(core::ptr::null_mut());

    pub unsafe fn new(pointer: *mut u8) -> Self {
        Self(pointer)
    }

    pub fn as_ptr(self) -> *mut u8 {
        self.0
    }

    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    fn offset(self, count: usize) -> Self {
        Self(unsafe { self.0.add(count) })
    }

    fn distance(self, from: Self) -> usize {
        (self.0 as usize).wrapping_sub(from.0 as usize)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Device;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeviceType {
    pub cpu: bool,
    pub gpu: bool,
    pub accelerator: bool,
    pub custom: bool,
    pub default: bool,
}

impl DeviceType {
    pub const NONE: Self = Self {
        cpu: false,
        gpu: false,
        accelerator: false,
        custom: false,
        default: false,
    };

    pub const GPU: Self = Self { gpu: true, ..Self::NONE };

    pub const ALL: Self = Self {
        cpu: true,
        gpu: true,
        accelerator: true,
        custom: true,
        default: true,
    };

    fn matches(self, wanted: Self) -> bool {
        (wanted.cpu && self.cpu)
            || (wanted.gpu && self.gpu)
            || (wanted.accelerator && self.accelerator)
            || (wanted.custom && self.custom)
            || wanted.default
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemCacheType {
    None,
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocalMemType {
    None,
    Local,
    Global,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FpConfig {
    pub denorm: bool,
    pub inf_nan: bool,
    pub round_to_nearest: bool,
    pub round_to_zero: bool,
    pub round_to_inf: bool,
    pub fma: bool,
    pub soft_float: bool,
}

impl FpConfig {
    pub const NONE: Self = Self {
        denorm: false,
        inf_nan: false,
        round_to_nearest: false,
        round_to_zero: false,
        round_to_inf: false,
        fma: false,
        soft_float: false,
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExecCapabilities {
    pub kernel: bool,
    pub native_kernel: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct QueueProperties {
    pub out_of_order: bool,
    pub profiling: bool,
}

impl QueueProperties {
    pub const SUPPORTED: Self = Self { out_of_order: false, profiling: true };

    fn within(self, supported: Self) -> bool {
        (supported.out_of_order || !self.out_of_order) && (supported.profiling || !self.profiling)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContextProperty {
    Platform,
    InteropUserSync(bool),
}

impl ContextProperty {
    fn name(&self) -> core::mem::Discriminant<Self> {
        core::mem::discriminant(self)
    }

    fn repeated(properties: &[ContextProperty]) -> bool {
        properties.iter().enumerate().any(|(index, property)| {
            properties[..index]
                .iter()
                .any(|earlier| earlier.name() == property.name())
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Unspecified,
    ReadWrite,
    WriteOnly,
    ReadOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostAccess {
    Unspecified,
    WriteOnly,
    ReadOnly,
    NoAccess,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Storage {
    Owned,
    Borrowed(SharedMemoryPointer),
    Copied(SharedMemoryPointer),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MemFlags {
    pub access: Access,
    pub host_access: HostAccess,
    pub alloc_host: bool,
    pub storage: Storage,
}

impl MemFlags {
    pub const DEFAULT: Self = Self {
        access: Access::Unspecified,
        host_access: HostAccess::Unspecified,
        alloc_host: false,
        storage: Storage::Owned,
    };

    fn host_pointer(self) -> SharedMemoryPointer {
        match self.storage {
            Storage::Borrowed(pointer) => pointer,
            Storage::Owned | Storage::Copied(_) => SharedMemoryPointer::NULL,
        }
    }

    fn narrows(self, parent: Self) -> bool {
        let access = match (parent.access, self.access) {
            (_, Access::Unspecified) => true,
            (Access::ReadOnly, wanted) => wanted == Access::ReadOnly,
            (Access::WriteOnly, wanted) => wanted == Access::WriteOnly,
            _ => true,
        };

        let host = !matches!(
            (parent.host_access, self.host_access),
            (HostAccess::WriteOnly, HostAccess::ReadOnly)
                | (HostAccess::ReadOnly, HostAccess::WriteOnly)
                | (HostAccess::NoAccess, HostAccess::ReadOnly)
                | (HostAccess::NoAccess, HostAccess::WriteOnly)
        );

        access && host
    }

    fn inherit(self, parent: Self) -> Self {
        Self {
            access: match self.access {
                Access::Unspecified => parent.access,
                access => access,
            },
            host_access: match self.host_access {
                HostAccess::Unspecified => parent.host_access,
                host_access => host_access,
            },
            alloc_host: parent.alloc_host,
            storage: parent.storage,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapFlags {
    pub read: bool,
    pub write: bool,
    pub write_invalidate: bool,
}

impl MapFlags {
    fn writes(self) -> bool {
        self.write || self.write_invalidate
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MigrateFlags {
    pub host: bool,
    pub content_undefined: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemObjectType {
    Buffer,
}

// TODO: are all of these used?
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommandType {
    ReadBuffer,
    WriteBuffer,
    CopyBuffer,
    ReadBufferRect,
    WriteBufferRect,
    CopyBufferRect,
    FillBuffer,
    MapBuffer,
    UnmapMemObject,
    MigrateMemObjects,
    Marker,
    Barrier,
    User,
    NdrangeKernel,
    Task,
    NativeKernel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Queued,
    Submitted,
    Running,
    Complete,
    Terminated(i32),
    Failed(Error),
}

impl Status {
    fn rank(self) -> i32 {
        match self {
            Status::Queued => 3,
            Status::Submitted => 2,
            Status::Running => 1,
            Status::Complete => 0,
            Status::Terminated(code) => code,
            Status::Failed(_) => -1,
        }
    }

    fn reached(self, wanted: Status) -> bool {
        self.is_terminated() || self.rank() <= wanted.rank()
    }

    fn is_terminated(self) -> bool {
        matches!(self, Status::Terminated(_) | Status::Failed(_))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlatformInfo {
    Profile,
    Version,
    Name,
    Vendor,
    Extensions,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceInfo {
    Type,
    VendorId,
    MaxComputeUnits,
    MaxWorkItemDimensions,
    MaxWorkGroupSize,
    MaxWorkItemSizes,
    PreferredVectorWidthChar,
    PreferredVectorWidthShort,
    PreferredVectorWidthInt,
    PreferredVectorWidthLong,
    PreferredVectorWidthFloat,
    PreferredVectorWidthDouble,
    PreferredVectorWidthHalf,
    NativeVectorWidthChar,
    NativeVectorWidthShort,
    NativeVectorWidthInt,
    NativeVectorWidthLong,
    NativeVectorWidthFloat,
    NativeVectorWidthDouble,
    NativeVectorWidthHalf,
    MaxClockFrequency,
    AddressBits,
    MaxReadImageArgs,
    MaxWriteImageArgs,
    MaxMemAllocSize,
    Image2dMaxWidth,
    Image2dMaxHeight,
    Image3dMaxWidth,
    Image3dMaxHeight,
    Image3dMaxDepth,
    ImageMaxBufferSize,
    ImageMaxArraySize,
    ImageSupport,
    MaxParameterSize,
    MaxSamplers,
    MemBaseAddrAlign,
    MinDataTypeAlignSize,
    SingleFpConfig,
    DoubleFpConfig,
    GlobalMemCacheType,
    GlobalMemCachelineSize,
    GlobalMemCacheSize,
    GlobalMemSize,
    MaxConstantBufferSize,
    MaxConstantArgs,
    LocalMemType,
    LocalMemSize,
    ErrorCorrectionSupport,
    ProfilingTimerResolution,
    EndianLittle,
    Available,
    CompilerAvailable,
    LinkerAvailable,
    ExecutionCapabilities,
    QueueProperties,
    HostUnifiedMemory,
    Platform,
    ParentDevice,
    PartitionMaxSubDevices,
    PartitionProperties,
    PartitionAffinityDomain,
    PartitionType,
    ReferenceCount,
    PreferredInteropUserSync,
    PrintfBufferSize,
    Name,
    Vendor,
    DriverVersion,
    Profile,
    Version,
    OpenclCVersion,
    Extensions,
    BuiltInKernels,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContextInfo {
    ReferenceCount,
    NumDevices,
    Devices,
    Properties,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueInfo {
    Context,
    Device,
    ReferenceCount,
    Properties,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemInfo {
    Type,
    Flags,
    Size,
    HostPtr,
    MapCount,
    ReferenceCount,
    Context,
    AssociatedMemObject,
    Offset,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventInfo {
    CommandQueue,
    Context,
    CommandType,
    ExecutionStatus,
    ReferenceCount,
}

pub enum InfoValue {
    Bool(bool),
    Uint(u32),
    Ulong(u64),
    Size(usize),
    Sizes(Vec<usize>),
    Text(String),
    Platform,
    Device(Option<Device>),
    Devices(Vec<Device>),
    DeviceType(DeviceType),
    MemCacheType(MemCacheType),
    LocalMemType(LocalMemType),
    FpConfig(FpConfig),
    ExecCapabilities(ExecCapabilities),
    QueueProperties(QueueProperties),
    ContextProperties(Option<Vec<ContextProperty>>),
    PartitionProperties(Vec<()>),
    AffinityDomain,
    MemObjectType(MemObjectType),
    MemFlags(MemFlags),
    CommandType(CommandType),
    Status(Status),
    HostPointer(SharedMemoryPointer),
    Context(ContextId),
    Queue(Option<QueueId>),
    Buffer(Option<BufferId>),
    Binaries(Vec<Vec<u8>>),
    BuildStatus(BuildStatus),
    BinaryType(BinaryType),
    Program(ProgramId),
}

pub struct Platform;

impl Platform {
    pub fn devices(wanted: DeviceType) -> Result<Vec<Device>> {
        if wanted == DeviceType::NONE {
            return Err(Error::InvalidDeviceType);
        }

        if !Device::TYPE.matches(wanted) {
            return Err(Error::DeviceNotFound);
        }

        Ok(vec![Device])
    }

    pub fn info(param: PlatformInfo) -> InfoValue {
        match param {
            PlatformInfo::Profile => InfoValue::Text("EMBEDDED_PROFILE".to_string()),
            PlatformInfo::Version => InfoValue::Text("OpenCL 1.2 vodd".to_string()),
            PlatformInfo::Name => InfoValue::Text("vodd".to_string()),
            PlatformInfo::Vendor => InfoValue::Text("vodd".to_string()),
            PlatformInfo::Extensions => InfoValue::Text(EXTENSIONS.to_string()),
        }
    }
}

impl Device {
    pub const TYPE: DeviceType = DeviceType::GPU;

    pub const MEM_BASE_ADDR_ALIGN: usize = MEM_BASE_ADDR_ALIGN_BITS as usize / 8;

    pub fn info(self, param: DeviceInfo) -> InfoValue {
        // TODO: validate below
        match param {
            DeviceInfo::Type => InfoValue::DeviceType(Self::TYPE),
            DeviceInfo::VendorId => InfoValue::Uint(0),
            DeviceInfo::MaxComputeUnits => InfoValue::Uint(1),
            DeviceInfo::MaxWorkItemDimensions => InfoValue::Uint(3),
            DeviceInfo::MaxWorkGroupSize => InfoValue::Size(MAX_WORK_GROUP_SIZE),
            DeviceInfo::MaxWorkItemSizes => InfoValue::Sizes(vec![MAX_WORK_ITEM_SIZE as usize; 3]),
            DeviceInfo::PreferredVectorWidthChar => InfoValue::Uint(16),
            DeviceInfo::PreferredVectorWidthShort => InfoValue::Uint(8),
            DeviceInfo::PreferredVectorWidthInt => InfoValue::Uint(4),
            DeviceInfo::PreferredVectorWidthLong => InfoValue::Uint(2),
            DeviceInfo::PreferredVectorWidthFloat => InfoValue::Uint(4),
            DeviceInfo::PreferredVectorWidthDouble => InfoValue::Uint(0),
            DeviceInfo::PreferredVectorWidthHalf => InfoValue::Uint(0),
            DeviceInfo::NativeVectorWidthChar => InfoValue::Uint(16),
            DeviceInfo::NativeVectorWidthShort => InfoValue::Uint(8),
            DeviceInfo::NativeVectorWidthInt => InfoValue::Uint(4),
            DeviceInfo::NativeVectorWidthLong => InfoValue::Uint(2),
            DeviceInfo::NativeVectorWidthFloat => InfoValue::Uint(4),
            DeviceInfo::NativeVectorWidthDouble => InfoValue::Uint(0),
            DeviceInfo::NativeVectorWidthHalf => InfoValue::Uint(0),
            DeviceInfo::MaxClockFrequency => InfoValue::Uint(1000),
            DeviceInfo::AddressBits => InfoValue::Uint(64),
            DeviceInfo::MaxReadImageArgs => InfoValue::Uint(0),
            DeviceInfo::MaxWriteImageArgs => InfoValue::Uint(0),
            DeviceInfo::MaxMemAllocSize => InfoValue::Ulong(MAX_MEM_ALLOC_SIZE),
            DeviceInfo::Image2dMaxWidth => InfoValue::Size(0),
            DeviceInfo::Image2dMaxHeight => InfoValue::Size(0),
            DeviceInfo::Image3dMaxWidth => InfoValue::Size(0),
            DeviceInfo::Image3dMaxHeight => InfoValue::Size(0),
            DeviceInfo::Image3dMaxDepth => InfoValue::Size(0),
            DeviceInfo::ImageMaxBufferSize => InfoValue::Size(0),
            DeviceInfo::ImageMaxArraySize => InfoValue::Size(0),
            DeviceInfo::ImageSupport => InfoValue::Bool(false),
            DeviceInfo::MaxParameterSize => InfoValue::Size(1024),
            DeviceInfo::MaxSamplers => InfoValue::Uint(0),
            DeviceInfo::MemBaseAddrAlign => InfoValue::Uint(MEM_BASE_ADDR_ALIGN_BITS),
            DeviceInfo::MinDataTypeAlignSize => InfoValue::Uint(128),
            DeviceInfo::SingleFpConfig => InfoValue::FpConfig(FpConfig {
                inf_nan: true,
                round_to_nearest: true,
                ..FpConfig::NONE
            }),
            DeviceInfo::DoubleFpConfig => InfoValue::FpConfig(FpConfig::NONE),
            DeviceInfo::GlobalMemCacheType => InfoValue::MemCacheType(MemCacheType::ReadWrite),
            DeviceInfo::GlobalMemCachelineSize => InfoValue::Uint(64),
            DeviceInfo::GlobalMemCacheSize => InfoValue::Ulong(32 * 1024),
            DeviceInfo::GlobalMemSize => InfoValue::Ulong(GLOBAL_MEM_SIZE),
            DeviceInfo::MaxConstantBufferSize => InfoValue::Ulong(64 * 1024),
            DeviceInfo::MaxConstantArgs => InfoValue::Uint(8),
            DeviceInfo::LocalMemType => InfoValue::LocalMemType(LocalMemType::Local),
            DeviceInfo::LocalMemSize => InfoValue::Ulong(LOCAL_MEM_SIZE),
            DeviceInfo::ErrorCorrectionSupport => InfoValue::Bool(false),
            DeviceInfo::ProfilingTimerResolution => InfoValue::Size(1),
            DeviceInfo::EndianLittle => InfoValue::Bool(true),
            DeviceInfo::Available => InfoValue::Bool(true),
            DeviceInfo::CompilerAvailable => InfoValue::Bool(compiler::available()),
            DeviceInfo::LinkerAvailable => InfoValue::Bool(compiler::linkable()),
            DeviceInfo::ExecutionCapabilities => {
                InfoValue::ExecCapabilities(ExecCapabilities { kernel: true, native_kernel: false })
            }
            DeviceInfo::QueueProperties => InfoValue::QueueProperties(QueueProperties::SUPPORTED),
            DeviceInfo::HostUnifiedMemory => InfoValue::Bool(true),
            DeviceInfo::Platform => InfoValue::Platform,
            DeviceInfo::ParentDevice => InfoValue::Device(None),
            DeviceInfo::PartitionMaxSubDevices => InfoValue::Uint(0),
            DeviceInfo::PartitionProperties => InfoValue::PartitionProperties(vec![()]),
            DeviceInfo::PartitionAffinityDomain => InfoValue::AffinityDomain,
            DeviceInfo::PartitionType => InfoValue::PartitionProperties(Vec::new()),
            DeviceInfo::ReferenceCount => InfoValue::Uint(1),
            DeviceInfo::PreferredInteropUserSync => InfoValue::Bool(true),
            DeviceInfo::PrintfBufferSize => InfoValue::Size(1024 * 1024),
            DeviceInfo::Name => InfoValue::Text("vodd".to_string()),
            DeviceInfo::Vendor => InfoValue::Text("vodd".to_string()),
            DeviceInfo::DriverVersion => InfoValue::Text("0.0.1".to_string()),
            DeviceInfo::Profile => InfoValue::Text("EMBEDDED_PROFILE".to_string()),
            DeviceInfo::Version => InfoValue::Text("OpenCL 1.2 vodd".to_string()),
            DeviceInfo::OpenclCVersion => InfoValue::Text("OpenCL C 1.2 ".to_string()),
            DeviceInfo::Extensions => InfoValue::Text(EXTENSIONS.to_string()),
            DeviceInfo::BuiltInKernels => InfoValue::Text("".to_string()),
        }
    }
}

pub struct Context {
    reference_count: u32,
    devices: Vec<Device>,
    properties: Option<Vec<ContextProperty>>,
    notify: Option<Notify>,
}

impl Context {
    pub fn create(
        properties: Option<Vec<ContextProperty>>,
        devices: Vec<Device>,
        notify: Option<Notify>,
    ) -> Result<ContextId> {
        // TODO: validate properties passed to create?

        if devices.is_empty() {
            return Err(Error::InvalidValue);
        }

        if properties.as_deref().is_some_and(ContextProperty::repeated) {
            return Err(Error::InvalidProperty);
        }

        let id = ContextId(next_object_id());
        let mut deduped = devices;
        deduped.dedup();

        CONTEXTS.lock().expect("contexts").insert(
            id,
            Self { reference_count: 1, devices: deduped, properties, notify },
        );

        Ok(id)
    }

    pub fn retain(id: ContextId) -> Result<()> {
        CONTEXTS
            .lock()
            .expect("contexts")
            .get_mut(&id)
            .map(|context| context.reference_count += 1)
            .ok_or(Error::InvalidContext)
    }

    pub fn release(id: ContextId) -> Result<()> {
        let mut contexts = CONTEXTS.lock().expect("contexts");
        let context = contexts.get_mut(&id).ok_or(Error::InvalidContext)?;

        context.reference_count -= 1;
        if context.reference_count == 0 {
            contexts.remove(&id);
        }

        Ok(())
    }

    pub fn exists(id: ContextId) -> Result<()> {
        CONTEXTS
            .lock()
            .expect("contexts")
            .contains_key(&id)
            .then_some(())
            .ok_or(Error::InvalidContext)
    }

    pub fn info(id: ContextId, param: ContextInfo) -> Result<InfoValue> {
        let contexts = CONTEXTS.lock().expect("contexts");
        let context = contexts.get(&id).ok_or(Error::InvalidContext)?;

        Ok(match param {
            ContextInfo::ReferenceCount => InfoValue::Uint(context.reference_count),
            ContextInfo::NumDevices => InfoValue::Uint(context.devices.len() as u32),
            ContextInfo::Devices => InfoValue::Devices(context.devices.clone()),
            ContextInfo::Properties => InfoValue::ContextProperties(context.properties.clone()),
        })
    }

    fn report(id: ContextId, message: &str) {
        // TODO: why is this called report instead of notify?
        let notify = CONTEXTS
            .lock()
            .expect("contexts")
            .get(&id)
            .and_then(|context| context.notify.clone());

        if let Some(notify) = notify {
            notify(message);
        }
    }
}

#[derive(Clone, Copy)]
pub enum Target {
    Buffer(BufferId),
    Host(SharedMemoryPointer),
}

#[derive(Clone, Copy)]
pub struct Slab {
    pub target: Target,
    pub origin: [usize; 3],
    pub row_pitch: usize,
    pub slice_pitch: usize,
}

impl Slab {
    pub fn buffer(target: BufferId, offset: usize) -> Self {
        Self {
            target: Target::Buffer(target),
            origin: [offset, 0, 0],
            row_pitch: 0,
            slice_pitch: 0,
        }
    }

    pub fn host(target: SharedMemoryPointer) -> Self {
        Self {
            target: Target::Host(target),
            origin: [0, 0, 0],
            row_pitch: 0,
            slice_pitch: 0,
        }
    }

    pub fn rect(target: Target, origin: [usize; 3], row_pitch: usize, slice_pitch: usize) -> Self {
        Self { target, origin, row_pitch, slice_pitch }
    }

    fn id(&self) -> Option<BufferId> {
        match self.target {
            Target::Buffer(id) => Some(id),
            Target::Host(_) => None,
        }
    }

    fn base(&self, buffers: &mut BTreeMap<BufferId, Buffer>) -> Option<SharedMemoryPointer> {
        // TODO: this should be a result type that errors out if the ID does not exist?
        match self.target {
            Target::Buffer(id) => Some(buffers.get_mut(&id)?.base()),
            Target::Host(host) => Some(host),
        }
    }

    fn pitches(&self, region: &[usize; 3]) -> (usize, usize) {
        let row = if self.row_pitch == 0 {
            region[0]
        } else {
            self.row_pitch
        };

        let slice = if self.slice_pitch == 0 {
            region[1] * row
        } else {
            self.slice_pitch
        };

        (row, slice)
    }

    fn start(&self, region: &[usize; 3]) -> usize {
        let (row, slice) = self.pitches(region);

        self.origin[2] * slice + self.origin[1] * row + self.origin[0]
    }

    fn spans(&self, region: &[usize; 3]) -> usize {
        let (row, slice) = self.pitches(region);
        let last = region[2].saturating_sub(1) * slice + region[1].saturating_sub(1) * row;

        self.start(region) + last + region[0]
    }

    fn validate(&self, region: &[usize; 3]) -> Result<()> {
        let Some(id) = self.id() else {
            return Ok(());
        };

        let size = Buffer::size(id)?;
        if region.contains(&0) || self.spans(region) > size {
            return Err(Error::InvalidValue);
        }

        Ok(())
    }
}

// TODO: what about the map command?
pub enum Command {
    Copy {
        source: Slab,
        destination: Slab,
        region: [usize; 3],
    },
    Fill {
        destination: Slab,
        region: [usize; 3],
        pattern: Vec<u8>,
    },
    Unmap {
        buffer: BufferId,
        mapped: SharedMemoryPointer,
    },
    Ndrange {
        launch: Launch,
        grid: Grid,
    },
    Nothing,
}

impl Command {
    fn buffers(&self) -> Vec<BufferId> {
        match self {
            Command::Copy { source, destination, .. } => [source.id(), destination.id()]
                .into_iter()
                .flatten()
                .collect(),
            Command::Fill { destination, .. } => destination.id().into_iter().collect(),
            Command::Unmap { buffer, .. } => vec![*buffer],
            Command::Ndrange { launch, .. } => launch
                .arguments
                .iter()
                .filter_map(|argument| match argument {
                    KernelArgument::Memory(buffer) => *buffer,
                    _ => None,
                })
                .collect(),
            Command::Nothing => Vec::new(),
        }
    }

    fn execute(&self) -> Result<()> {
        if let Command::Ndrange { launch, grid } = self {
            return launch.run(grid);
        }

        let mut buffers = BUFFERS.lock().expect("buffers");

        match self {
            Command::Copy { source, destination, region } => {
                let Some(from) = source.base(&mut buffers) else {
                    return Ok(());
                };
                let Some(into) = destination.base(&mut buffers) else {
                    return Ok(());
                };

                copy_slabs(from, source, into, destination, region);
            }
            Command::Fill { destination, region, pattern } => {
                let Some(into) = destination.base(&mut buffers) else {
                    return Ok(());
                };

                fill_slab(into, destination, region, pattern);
            }
            Command::Unmap { buffer, mapped } => {
                if let Some(buffer) = buffers.get_mut(buffer) {
                    buffer.unmap(*mapped);
                }
            }
            Command::Ndrange { .. } | Command::Nothing => {}
        }

        Ok(())
    }
}

fn copy_slabs(
    from: SharedMemoryPointer,
    source: &Slab,
    into: SharedMemoryPointer,
    destination: &Slab,
    region: &[usize; 3],
) {
    let (source_row, source_slice) = source.pitches(region);
    let (destination_row, destination_slice) = destination.pitches(region);
    let source_start = source.start(region);
    let destination_start = destination.start(region);

    for slice in 0..region[2] {
        for row in 0..region[1] {
            unsafe {
                core::ptr::copy(
                    from.offset(source_start + slice * source_slice + row * source_row)
                        .as_ptr(),
                    into.offset(
                        destination_start + slice * destination_slice + row * destination_row,
                    )
                    .as_ptr(),
                    region[0],
                );
            }
        }
    }
}

fn fill_slab(into: SharedMemoryPointer, destination: &Slab, region: &[usize; 3], pattern: &[u8]) {
    let start = destination.start(region);

    for chunk in (0..region[0]).step_by(pattern.len()) {
        unsafe {
            core::ptr::copy_nonoverlapping(
                pattern.as_ptr(),
                into.offset(start + chunk).as_ptr(),
                pattern.len(),
            );
        }
    }
}

struct Enqueued {
    command: Command,
    event: EventId,
    wait: Vec<EventId>,
}

pub struct CommandQueue {
    reference_count: u32,
    context: ContextId,
    device: Device,
    properties: QueueProperties,
    commands: VecDeque<Enqueued>,
    running: bool,
}

impl CommandQueue {
    pub fn create(
        context: ContextId,
        device: Device,
        properties: QueueProperties,
    ) -> Result<QueueId> {
        let devices = match Context::info(context, ContextInfo::Devices)? {
            InfoValue::Devices(devices) => devices,
            _ => Vec::new(),
        };

        if !devices.contains(&device) {
            return Err(Error::InvalidDevice);
        }

        if !properties.within(QueueProperties::SUPPORTED) {
            return Err(Error::InvalidQueueProperties);
        }

        Context::retain(context)?;

        let id = QueueId(next_object_id());

        QUEUES.lock().expect("queues").insert(
            id,
            Self {
                reference_count: 1,
                context,
                device,
                properties,
                commands: VecDeque::new(),
                running: false,
            },
        );

        update_worker();

        Ok(id)
    }

    pub fn retain(id: QueueId) -> Result<()> {
        QUEUES
            .lock()
            .expect("queues")
            .get_mut(&id)
            .map(|queue| queue.reference_count += 1)
            .ok_or(Error::InvalidCommandQueue)
    }

    pub fn release(id: QueueId) -> Result<()> {
        let dropping = {
            let mut queues = QUEUES.lock().expect("queues");
            let queue = queues.get_mut(&id).ok_or(Error::InvalidCommandQueue)?;

            queue.reference_count -= 1;
            queue.reference_count == 0
        };

        if !dropping {
            return Ok(());
        }

        Self::finish(id)?;

        let context = QUEUES
            .lock()
            .expect("queues")
            .remove(&id)
            .map(|queue| queue.context);

        update_worker();

        if let Some(context) = context {
            Context::release(context)?;
        }

        Ok(())
    }

    pub fn info(id: QueueId, param: QueueInfo) -> Result<InfoValue> {
        let queues = QUEUES.lock().expect("queues");
        let queue = queues.get(&id).ok_or(Error::InvalidCommandQueue)?;

        Ok(match param {
            QueueInfo::Context => InfoValue::Context(queue.context),
            QueueInfo::Device => InfoValue::Device(Some(queue.device)),
            QueueInfo::ReferenceCount => InfoValue::Uint(queue.reference_count),
            QueueInfo::Properties => InfoValue::QueueProperties(queue.properties),
        })
    }

    fn profiles(id: QueueId) -> Result<bool> {
        QUEUES
            .lock()
            .expect("queues")
            .get(&id)
            .map(|queue| queue.properties.profiling)
            .ok_or(Error::InvalidCommandQueue)
    }

    pub fn flush(id: QueueId) -> Result<()> {
        // TODO: don't we have to implement this? why are we not implementing this?
        // TODO: should this call finish?

        QUEUES
            .lock()
            .expect("queues")
            .contains_key(&id)
            .then_some(())
            .ok_or(Error::InvalidCommandQueue)
    }

    pub fn finish(id: QueueId) -> Result<()> {
        loop {
            let generation = progress();

            let idle = {
                let queues = QUEUES.lock().expect("queues");
                let queue = queues.get(&id).ok_or(Error::InvalidCommandQueue)?;

                queue.commands.is_empty() && !queue.running
            };

            if idle {
                return Ok(());
            }

            await_progress(generation);
        }
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn read_buffer(
        id: QueueId,
        buffer: BufferId,
        offset: usize,
        size: usize,
        host: SharedMemoryPointer,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        let source = Slab::buffer(buffer, offset);

        Self::transfer(
            id,
            source,
            Slab::host(host),
            [size, 1, 1],
            CommandType::ReadBuffer,
            wait,
        )
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn write_buffer(
        id: QueueId,
        buffer: BufferId,
        offset: usize,
        size: usize,
        host: SharedMemoryPointer,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        let destination = Slab::buffer(buffer, offset);

        Self::transfer(
            id,
            Slab::host(host),
            destination,
            [size, 1, 1],
            CommandType::WriteBuffer,
            wait,
        )
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn copy_buffer(
        id: QueueId,
        source: BufferId,
        source_offset: usize,
        destination: BufferId,
        destination_offset: usize,
        size: usize,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        if source == destination
            && source_offset < destination_offset + size
            && destination_offset < source_offset + size
        {
            return Err(Error::MemCopyOverlap);
        }

        Self::transfer(
            id,
            Slab::buffer(source, source_offset),
            Slab::buffer(destination, destination_offset),
            [size, 1, 1],
            CommandType::CopyBuffer,
            wait,
        )
    }

    // TODO: should this be called enqueue_transfer
    pub fn transfer(
        id: QueueId,
        source: Slab,
        destination: Slab,
        region: [usize; 3],
        command_type: CommandType,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        source.validate(&region)?;
        destination.validate(&region)?;

        match (source.id(), destination.id()) {
            (Some(device), None) => Buffer::host_may_read(device)?,
            (None, Some(device)) => Buffer::host_may_write(device)?,
            _ => {}
        }

        if let Target::Host(host) = source.target
            && host.is_null()
        {
            return Err(Error::InvalidValue);
        }

        if let Target::Host(host) = destination.target
            && host.is_null()
        {
            return Err(Error::InvalidValue);
        }

        Self::enqueue(
            id,
            Command::Copy { source, destination, region },
            command_type,
            wait,
        )
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn fill_buffer(
        id: QueueId,
        buffer: BufferId,
        offset: usize,
        size: usize,
        pattern: Vec<u8>,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        let sized = [1, 2, 4, 8, 16, 32, 64, 128].contains(&pattern.len());

        if !sized || !offset.is_multiple_of(pattern.len()) || !size.is_multiple_of(pattern.len()) {
            return Err(Error::InvalidValue);
        }

        let destination = Slab::buffer(buffer, offset);
        let region = [size, 1, 1];
        destination.validate(&region)?;

        Self::enqueue(
            id,
            Command::Fill { destination, region, pattern },
            CommandType::FillBuffer,
            wait,
        )
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn map_buffer(
        id: QueueId,
        buffer: BufferId,
        offset: usize,
        size: usize,
        flags: MapFlags,
        wait: Vec<EventId>,
    ) -> Result<(SharedMemoryPointer, EventId)> {
        if flags.read {
            Buffer::host_may_read(buffer)?;
        }

        if flags.writes() {
            Buffer::host_may_write(buffer)?;
        }

        let mapped = Buffer::map(buffer, offset, size, flags)?;

        match Self::enqueue(id, Command::Nothing, CommandType::MapBuffer, wait) {
            Ok(event) => Ok((mapped, event)),
            Err(error) => {
                Buffer::undo_map(buffer, mapped);

                Err(error)
            }
        }
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn unmap(
        id: QueueId,
        buffer: BufferId,
        mapped: SharedMemoryPointer,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        Buffer::exists(buffer)?;

        if !Buffer::is_mapped(buffer, mapped) {
            return Err(Error::InvalidValue);
        }

        Buffer::undo_map(buffer, mapped);

        Self::enqueue(id, Command::Nothing, CommandType::UnmapMemObject, wait)
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn migrate(
        id: QueueId,
        buffers: &[BufferId],
        _flags: MigrateFlags,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        if buffers.is_empty() {
            return Err(Error::InvalidValue);
        }

        buffers
            .iter()
            .try_for_each(|buffer| Buffer::exists(*buffer))?;

        Self::enqueue(id, Command::Nothing, CommandType::MigrateMemObjects, wait)
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn ndrange(
        id: QueueId,
        kernel: KernelId,
        geometry: Geometry,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        let (launch, required, context) = Kernel::snapshot(kernel)?;
        let grid = geometry.resolve(required);

        Self::validate_grid(
            &grid,
            geometry.local.is_some().then_some(required).flatten(),
        )?;

        if launch.arena as u64 > LOCAL_MEM_SIZE {
            return Err(Error::InvalidWorkGroupSize);
        }

        if Self::context(id)? != context {
            return Err(Error::InvalidContext);
        }

        Self::enqueue(
            id,
            Command::Ndrange { launch, grid },
            CommandType::NdrangeKernel,
            wait,
        )
    }

    pub fn task(id: QueueId, kernel: KernelId, wait: Vec<EventId>) -> Result<EventId> {
        let geometry = Geometry {
            dimensions: 1,
            offset: [0, 0, 0],
            global: [1, 1, 1],
            local: Some([1, 1, 1]),
        };

        Self::ndrange(id, kernel, geometry, wait)
    }

    fn context(id: QueueId) -> Result<ContextId> {
        QUEUES
            .lock()
            .expect("queues")
            .get(&id)
            .map(|queue| queue.context)
            .ok_or(Error::InvalidCommandQueue)
    }

    fn validate_grid(geometry: &Grid, required: Option<[u32; 3]>) -> Result<()> {
        if !(1..=3).contains(&geometry.dimensions) {
            return Err(Error::InvalidWorkDimension);
        }

        if geometry.global.contains(&0) {
            return Err(Error::InvalidGlobalWorkSize);
        }

        if geometry
            .offset
            .iter()
            .zip(&geometry.global)
            .any(|(offset, global)| offset.checked_add(*global).is_none())
        {
            return Err(Error::InvalidGlobalOffset);
        }

        if geometry.local.contains(&0)
            || geometry
                .local
                .iter()
                .zip(&geometry.global)
                .any(|(local, global)| !global.is_multiple_of(*local))
        {
            return Err(Error::InvalidWorkGroupSize);
        }

        if geometry.work_group_size() > MAX_WORK_GROUP_SIZE {
            return Err(Error::InvalidWorkGroupSize);
        }

        if geometry
            .local
            .iter()
            .any(|local| *local > MAX_WORK_ITEM_SIZE)
        {
            return Err(Error::InvalidWorkItemSize);
        }

        if required.is_some_and(|required| {
            required
                .iter()
                .zip(&geometry.local)
                .any(|(wanted, local)| *wanted as u64 != *local)
        }) {
            return Err(Error::InvalidWorkGroupSize);
        }

        Ok(())
    }

    pub fn marker(id: QueueId, wait: Vec<EventId>) -> Result<EventId> {
        Self::enqueue(id, Command::Nothing, CommandType::Marker, wait)
    }

    // TODO: should this be prefixed with enqueue_?
    pub fn barrier(id: QueueId, wait: Vec<EventId>) -> Result<EventId> {
        Self::enqueue(id, Command::Nothing, CommandType::Barrier, wait)
    }

    fn enqueue(
        id: QueueId,
        command: Command,
        command_type: CommandType,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        let mut queues = QUEUES.lock().expect("queues");
        let queue = queues.get_mut(&id).ok_or(Error::InvalidCommandQueue)?;
        let context = queue.context;

        if !Event::share_context(&wait, context)
            || !Buffer::share_context(&command.buffers(), context)
        {
            return Err(Error::InvalidContext);
        }

        let event = Event::create(context, Some(id), command_type, Status::Queued);
        Event::retain(event)?;

        for waited in &wait {
            Event::retain(*waited)?;
        }

        for buffer in command.buffers() {
            Buffer::retain(buffer)?;
        }

        queue.commands.push_back(Enqueued { command, event, wait });
        drop(queues);

        Event::stamp(event, ProfilingInfo::Submit);

        signal_progress();

        Ok(event)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinaryType {
    None,
    CompiledObject,
    Library,
    Executable,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildStatus {
    None,
    Error,
    Success,
    InProgress,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgramInfo {
    ReferenceCount,
    Context,
    NumDevices,
    Devices,
    Source,
    BinarySizes,
    Binaries,
    NumKernels,
    KernelNames,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgramBuildInfo {
    Status,
    Options,
    Log,
    BinaryType,
}

type Translated = (Vec<u8>, String);
type Failed = (Error, String);

pub struct LinkFailure {
    pub program: Option<ProgramId>,
    pub error: Error,
}

pub struct Program {
    reference_count: u32,
    context: ContextId,
    devices: Vec<Device>,
    source: Option<String>,
    binary: Option<Vec<u8>>,
    module: Option<Arc<bitcode::Module>>,
    binary_type: BinaryType,
    status: BuildStatus,
    options: String,
    log: String,
    kernels: u32,
}

impl Program {
    pub fn create_with_source(context: ContextId, source: String) -> Result<ProgramId> {
        Context::exists(context)?;

        if source.is_empty() {
            return Err(Error::InvalidValue);
        }

        Self::insert(context, Some(source), None)
    }

    pub fn create_with_binary(context: ContextId, binary: Vec<u8>) -> Result<ProgramId> {
        Context::exists(context)?;

        let (binary_type, module) = Self::unwrap_binary(&binary)?;
        let parsed = parser::parse(&module).map_err(|_| Error::InvalidBinary)?;

        let id = Self::insert(context, None, Some(module))?;

        let mut programs = PROGRAMS.lock().expect("programs");
        if let Some(program) = programs.get_mut(&id) {
            program.binary_type = binary_type;
            program.status = BuildStatus::Success;
            program.module = Some(Arc::new(parsed));
        }

        Ok(id)
    }

    fn wrap_binary(binary: &[u8], binary_type: BinaryType) -> Vec<u8> {
        let tag: u32 = match binary_type {
            BinaryType::None => 0,
            BinaryType::CompiledObject => 1,
            BinaryType::Library => 2,
            BinaryType::Executable => 4,
        };

        BINARY_MAGIC
            .iter()
            .copied()
            .chain(tag.to_le_bytes())
            .chain(binary.iter().copied())
            .collect()
    }

    fn unwrap_binary(binary: &[u8]) -> Result<(BinaryType, Vec<u8>)> {
        let Some(tagged) = binary.strip_prefix(BINARY_MAGIC) else {
            return Ok((BinaryType::Executable, binary.to_vec()));
        };

        let (tag, module) = tagged.split_at_checked(4).ok_or(Error::InvalidBinary)?;
        let tag = u32::from_le_bytes(tag.try_into().map_err(|_| Error::InvalidBinary)?);

        let binary_type = match tag {
            1 => BinaryType::CompiledObject,
            2 => BinaryType::Library,
            4 => BinaryType::Executable,
            _ => return Err(Error::InvalidBinary),
        };

        Ok((binary_type, module.to_vec()))
    }

    pub fn retain(id: ProgramId) -> Result<()> {
        PROGRAMS
            .lock()
            .expect("programs")
            .get_mut(&id)
            .map(|program| program.reference_count += 1)
            .ok_or(Error::InvalidProgram)
    }

    pub fn release(id: ProgramId) -> Result<()> {
        let mut programs = PROGRAMS.lock().expect("programs");
        let program = programs.get_mut(&id).ok_or(Error::InvalidProgram)?;

        program.reference_count -= 1;
        if program.reference_count != 0 {
            return Ok(());
        }

        let context = programs.remove(&id).expect("program").context;
        drop(programs);

        Context::release(context)
    }

    pub fn exists(id: ProgramId) -> Result<()> {
        PROGRAMS
            .lock()
            .expect("programs")
            .contains_key(&id)
            .then_some(())
            .ok_or(Error::InvalidProgram)
    }

    pub fn info(id: ProgramId, param: ProgramInfo) -> Result<InfoValue> {
        let programs = PROGRAMS.lock().expect("programs");
        let program = programs.get(&id).ok_or(Error::InvalidProgram)?;

        Ok(match param {
            ProgramInfo::ReferenceCount => InfoValue::Uint(program.reference_count),
            ProgramInfo::Context => InfoValue::Context(program.context),
            ProgramInfo::NumDevices => InfoValue::Uint(program.devices.len() as u32),
            ProgramInfo::Devices => InfoValue::Devices(program.devices.clone()),
            ProgramInfo::Source => InfoValue::Text(program.source.clone().unwrap_or_default()),
            ProgramInfo::BinarySizes => InfoValue::Sizes(vec![program.tagged().len()]),
            ProgramInfo::Binaries => InfoValue::Binaries(vec![program.tagged()]),
            ProgramInfo::NumKernels => InfoValue::Size(program.entries()?.len()),
            ProgramInfo::KernelNames => InfoValue::Text(program.entries()?.join(";")),
        })
    }

    pub fn build_info(id: ProgramId, param: ProgramBuildInfo) -> Result<InfoValue> {
        let programs = PROGRAMS.lock().expect("programs");
        let program = programs.get(&id).ok_or(Error::InvalidProgram)?;

        Ok(match param {
            ProgramBuildInfo::Status => InfoValue::BuildStatus(program.status),
            ProgramBuildInfo::Options => InfoValue::Text(program.options.clone()),
            ProgramBuildInfo::Log => InfoValue::Text(program.log.clone()),
            ProgramBuildInfo::BinaryType => InfoValue::BinaryType(program.binary_type),
        })
    }

    pub fn build(id: ProgramId, options: String, notify: Option<ProgramNotify>) -> Result<()> {
        Self::translate(id, options, BinaryType::Executable)?;

        if let Some(notify) = notify {
            notify(id);
        }

        Ok(())
    }

    pub fn compile(
        id: ProgramId,
        options: String,
        headers: Vec<(String, ProgramId)>,
        notify: Option<ProgramNotify>,
    ) -> Result<()> {
        let included = Self::included(&headers)?;

        Self::translate_with(id, options, BinaryType::CompiledObject, included)?;

        if let Some(notify) = notify {
            notify(id);
        }

        Ok(())
    }

    fn objects(inputs: &[ProgramId]) -> Result<Vec<Vec<u8>>> {
        let programs = PROGRAMS.lock().expect("programs");

        inputs
            .iter()
            .map(|id| {
                let program = programs.get(id).ok_or(Error::InvalidProgram)?;

                if matches!(program.binary_type, BinaryType::None) {
                    return Err(Error::InvalidOperation);
                }

                program.binary.clone().ok_or(Error::InvalidOperation)
            })
            .collect()
    }

    pub fn link(
        context: ContextId,
        options: String,
        inputs: Vec<ProgramId>,
        notify: Option<ProgramNotify>,
    ) -> core::result::Result<ProgramId, LinkFailure> {
        let prepared = (|| {
            Context::exists(context)?;

            if inputs.is_empty() {
                return Err(Error::InvalidValue);
            }

            Self::objects(&inputs)
        })();

        let objects = match prepared {
            Ok(objects) => objects,
            Err(error) => {
                return Err(LinkFailure { program: None, error });
            }
        };

        let library = options.contains("-create-library");
        let id = match Self::insert(context, None, None) {
            Ok(id) => id,
            Err(error) => {
                return Err(LinkFailure { program: None, error });
            }
        };

        let binary_type = if library {
            BinaryType::Library
        } else {
            BinaryType::Executable
        };

        let linked = compiler::link(&objects, library);

        if let Err(error) = Self::adopt(id, options, binary_type, linked) {
            if let Some(notify) = notify {
                notify(id);
            }

            return Err(LinkFailure { program: Some(id), error });
        }

        if let Some(notify) = notify {
            notify(id);
        }

        Ok(id)
    }

    pub fn kernel_names(id: ProgramId) -> Result<Vec<String>> {
        PROGRAMS
            .lock()
            .expect("programs")
            .get(&id)
            .ok_or(Error::InvalidProgram)?
            .entries()
    }

    fn entry(
        id: ProgramId,
        name: &str,
    ) -> Result<(Arc<bitcode::Module>, bitcode::Id, Option<[u32; 3]>)> {
        let programs = PROGRAMS.lock().expect("programs");
        let program = programs.get(&id).ok_or(Error::InvalidProgram)?;
        let module = program
            .module
            .as_ref()
            .ok_or(Error::InvalidProgramExecutable)?;

        let entry = module
            .entry_points()
            .iter()
            .find(|entry| entry.name == name)
            .ok_or(Error::InvalidKernelName)?;

        Ok((
            Arc::clone(module),
            entry.function,
            entry.required_local_size,
        ))
    }

    fn context(id: ProgramId) -> Result<ContextId> {
        PROGRAMS
            .lock()
            .expect("programs")
            .get(&id)
            .map(|program| program.context)
            .ok_or(Error::InvalidProgram)
    }

    fn attach(id: ProgramId) -> Result<()> {
        PROGRAMS
            .lock()
            .expect("programs")
            .get_mut(&id)
            .map(|program| program.kernels += 1)
            .ok_or(Error::InvalidProgram)
    }

    fn detach(id: ProgramId) {
        if let Some(program) = PROGRAMS.lock().expect("programs").get_mut(&id) {
            program.kernels -= 1;
        }
    }

    fn insert(
        context: ContextId,
        source: Option<String>,
        binary: Option<Vec<u8>>,
    ) -> Result<ProgramId> {
        Context::retain(context)?;

        let id = ProgramId(next_object_id());
        let binary_type = if binary.is_some() {
            BinaryType::Executable
        } else {
            BinaryType::None
        };

        PROGRAMS.lock().expect("programs").insert(
            id,
            Self {
                reference_count: 1,
                context,
                devices: vec![Device],
                source,
                binary,
                module: None,
                binary_type,
                status: BuildStatus::None,
                options: String::new(),
                log: String::new(),
                kernels: 0,
            },
        );

        Ok(id)
    }

    fn adopt(
        id: ProgramId,
        options: String,
        binary_type: BinaryType,
        produced: Result<compiler::Output>,
    ) -> Result<()> {
        let mut programs = PROGRAMS.lock().expect("programs");
        let program = programs.get_mut(&id).ok_or(Error::InvalidProgram)?;

        program.options = options;

        let output = match produced {
            Ok(output) => output,
            Err(error) => {
                program.status = BuildStatus::Error;
                program.log = format!("{error:?}\n");

                return Err(error);
            }
        };

        program.log = output.log;

        let Some(binary) = output.binary else {
            program.status = BuildStatus::Error;
            program.binary_type = BinaryType::None;

            return Err(Error::LinkProgramFailure);
        };

        match parser::parse(&binary) {
            Ok(module) => {
                program.binary = Some(binary);
                program.module = Some(Arc::new(module));
                program.status = BuildStatus::Success;
                program.binary_type = binary_type;

                Ok(())
            }
            Err(error) => {
                program.status = BuildStatus::Error;
                program.binary_type = BinaryType::None;
                program.log = format!("{error:?}\n");

                Err(Error::LinkProgramFailure)
            }
        }
    }

    fn translate(id: ProgramId, options: String, binary_type: BinaryType) -> Result<()> {
        Self::translate_with(id, options, binary_type, Vec::new())
    }

    fn translate_with(
        id: ProgramId,
        options: String,
        binary_type: BinaryType,
        included: Vec<(String, String)>,
    ) -> Result<()> {
        let source = {
            let mut programs = PROGRAMS.lock().expect("programs");
            let program = programs.get_mut(&id).ok_or(Error::InvalidProgram)?;

            if program.kernels != 0 {
                return Err(Error::InvalidOperation);
            }

            program.options = options.clone();
            program.status = BuildStatus::InProgress;

            match (&program.source, &program.binary) {
                (Some(source), _) => Some(source.clone()),
                (None, Some(_)) => None,
                (None, None) => return Err(Error::InvalidOperation),
            }
        };

        let translated = match source {
            Some(source) => Self::from_source(&source, &included, &options),
            None => Ok(None),
        };

        let mut programs = PROGRAMS.lock().expect("programs");
        let program = programs.get_mut(&id).ok_or(Error::InvalidProgram)?;

        match translated {
            Ok(Some((binary, log))) => {
                program.binary = Some(binary);
                program.log = log;
            }
            Ok(None) => {}
            Err((error, log)) => {
                program.status = BuildStatus::Error;
                program.binary_type = BinaryType::None;
                program.log = log;

                return Err(error);
            }
        }

        let parsed = program
            .binary
            .as_ref()
            .map(|binary| parser::parse(binary))
            .transpose();

        match parsed {
            Ok(module) => {
                program.module = module.map(Arc::new);
                program.status = BuildStatus::Success;
                program.binary_type = binary_type;

                Ok(())
            }
            Err(error) => {
                program.status = BuildStatus::Error;
                program.binary_type = BinaryType::None;
                program.log = format!("{error:?}\n");

                Err(Error::BuildProgramFailure)
            }
        }
    }

    fn from_source(
        source: &str,
        headers: &[(String, String)],
        options: &str,
    ) -> core::result::Result<Option<Translated>, Failed> {
        let output = compiler::compile(source, headers, options)
            .map_err(|error| (error, "no SPIR-V capable clang was found\n".to_string()))?;

        match output.binary {
            Some(binary) => Ok(Some((binary, output.log))),
            None => Err((Error::BuildProgramFailure, output.log)),
        }
    }

    fn included(headers: &[(String, ProgramId)]) -> Result<Vec<(String, String)>> {
        let programs = PROGRAMS.lock().expect("programs");

        headers
            .iter()
            .map(|(name, id)| {
                let text = programs
                    .get(id)
                    .ok_or(Error::InvalidProgram)?
                    .source
                    .clone()
                    .ok_or(Error::InvalidOperation)?;

                Ok((name.clone(), text))
            })
            .collect()
    }

    fn tagged(&self) -> Vec<u8> {
        self.binary
            .as_ref()
            .map(|binary| Self::wrap_binary(binary, self.binary_type))
            .unwrap_or_default()
    }

    fn entries(&self) -> Result<Vec<String>> {
        let module = self
            .module
            .as_ref()
            .ok_or(Error::InvalidProgramExecutable)?;

        Ok(module
            .entry_points()
            .iter()
            .map(|entry| entry.name.clone())
            .collect())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArgumentKind {
    Global,
    Local,
    Constant,
    Value(usize),
}

#[derive(Clone, Debug)]
pub enum KernelArgument {
    Memory(Option<BufferId>),
    Local(usize),
    Value(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KernelInfo {
    FunctionName,
    NumArgs,
    ReferenceCount,
    Context,
    Program,
    Attributes,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KernelWorkGroupInfo {
    WorkGroupSize,
    CompileWorkGroupSize,
    LocalMemSize,
    PreferredWorkGroupSizeMultiple,
    PrivateMemSize,
}

pub struct Kernel {
    reference_count: u32,
    context: ContextId,
    program: ProgramId,
    name: String,
    module: Arc<bitcode::Module>,
    function: bitcode::Id,
    signature: Vec<ArgumentKind>,
    required_local_size: Option<[u32; 3]>,
    static_local: usize,
    arguments: Vec<Option<KernelArgument>>,
}

pub struct Launch {
    module: Arc<bitcode::Module>,
    function: bitcode::Id,
    arguments: Vec<KernelArgument>,
    local_offsets: Vec<usize>,
    arena: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub dimensions: u32,
    pub offset: [u64; 3],
    pub global: [u64; 3],
    pub local: Option<[u64; 3]>,
}

#[derive(Clone, Copy, Debug)]
pub struct Grid {
    dimensions: u32,
    offset: [u64; 3],
    global: [u64; 3],
    local: [u64; 3],
}

impl Grid {
    fn work_group_count(&self) -> [u64; 3] {
        [
            self.global[0] / self.local[0],
            self.global[1] / self.local[1],
            self.global[2] / self.local[2],
        ]
    }

    fn work_group_size(&self) -> usize {
        (self.local[0] * self.local[1] * self.local[2]) as usize
    }
}

impl Geometry {
    fn resolve(&self, required: Option<[u32; 3]>) -> Grid {
        let local = match (self.local, required) {
            (Some(local), _) => local,
            (None, Some(required)) => required.map(u64::from),
            (None, None) => divide(self.global, self.dimensions),
        };

        Grid {
            dimensions: self.dimensions,
            offset: self.offset,
            global: self.global,
            local,
        }
    }
}

fn divide(global: [u64; 3], dimensions: u32) -> [u64; 3] {
    let mut local = [1u64; 3];
    let mut budget = MAX_WORK_GROUP_SIZE as u64;

    for index in 0..dimensions as usize {
        let size = (1..=budget.min(global[index]))
            .rev()
            .find(|candidate| global[index].is_multiple_of(*candidate))
            .unwrap_or(1);

        local[index] = size;
        budget /= size;
    }

    local
}

impl Kernel {
    pub fn create(program: ProgramId, name: String) -> Result<KernelId> {
        let (module, function, required_local_size) = Program::entry(program, &name)?;
        let signature = Self::signature(&module, function)?;
        let context = Program::context(program)?;
        let static_local =
            interpreter::local_memory_size(&module).map_err(|_| Error::InvalidKernelDefinition)?;

        Program::retain(program)?;
        Program::attach(program)?;

        let id = KernelId(next_object_id());
        let arguments = vec![None; signature.len()];

        KERNELS.lock().expect("kernels").insert(
            id,
            Self {
                reference_count: 1,
                context,
                program,
                name,
                module,
                function,
                signature,
                required_local_size,
                static_local,
                arguments,
            },
        );

        Ok(id)
    }

    pub fn create_all(program: ProgramId) -> Result<Vec<KernelId>> {
        Program::kernel_names(program)?
            .into_iter()
            .map(|name| Self::create(program, name))
            .collect()
    }

    pub fn retain(id: KernelId) -> Result<()> {
        KERNELS
            .lock()
            .expect("kernels")
            .get_mut(&id)
            .map(|kernel| kernel.reference_count += 1)
            .ok_or(Error::InvalidKernel)
    }

    pub fn release(id: KernelId) -> Result<()> {
        let mut kernels = KERNELS.lock().expect("kernels");
        let kernel = kernels.get_mut(&id).ok_or(Error::InvalidKernel)?;

        kernel.reference_count -= 1;
        if kernel.reference_count != 0 {
            return Ok(());
        }

        let program = kernels.remove(&id).expect("kernel").program;
        drop(kernels);

        Program::detach(program);

        Program::release(program)
    }

    pub fn exists(id: KernelId) -> Result<()> {
        KERNELS
            .lock()
            .expect("kernels")
            .contains_key(&id)
            .then_some(())
            .ok_or(Error::InvalidKernel)
    }

    pub fn info(id: KernelId, param: KernelInfo) -> Result<InfoValue> {
        let kernels = KERNELS.lock().expect("kernels");
        let kernel = kernels.get(&id).ok_or(Error::InvalidKernel)?;

        Ok(match param {
            KernelInfo::FunctionName => InfoValue::Text(kernel.name.clone()),
            KernelInfo::NumArgs => InfoValue::Uint(kernel.signature.len() as u32),
            KernelInfo::ReferenceCount => InfoValue::Uint(kernel.reference_count),
            KernelInfo::Context => InfoValue::Context(kernel.context),
            KernelInfo::Program => InfoValue::Program(kernel.program),
            KernelInfo::Attributes => InfoValue::Text(String::new()),
        })
    }

    pub fn work_group_info(id: KernelId, param: KernelWorkGroupInfo) -> Result<InfoValue> {
        let kernels = KERNELS.lock().expect("kernels");
        let kernel = kernels.get(&id).ok_or(Error::InvalidKernel)?;

        Ok(match param {
            KernelWorkGroupInfo::WorkGroupSize => InfoValue::Size(MAX_WORK_GROUP_SIZE),
            KernelWorkGroupInfo::CompileWorkGroupSize => InfoValue::Sizes(
                kernel
                    .required_local_size
                    .unwrap_or([0, 0, 0])
                    .iter()
                    .map(|size| *size as usize)
                    .collect(),
            ),
            KernelWorkGroupInfo::LocalMemSize => InfoValue::Ulong(kernel.local_bytes() as u64),
            KernelWorkGroupInfo::PreferredWorkGroupSizeMultiple => InfoValue::Size(1),
            KernelWorkGroupInfo::PrivateMemSize => InfoValue::Ulong(0),
        })
    }

    pub fn argument_kind(id: KernelId, index: u32) -> Result<ArgumentKind> {
        KERNELS
            .lock()
            .expect("kernels")
            .get(&id)
            .ok_or(Error::InvalidKernel)?
            .signature
            .get(index as usize)
            .copied()
            .ok_or(Error::InvalidArgIndex)
    }

    pub fn set_argument(id: KernelId, index: u32, argument: KernelArgument) -> Result<()> {
        let mut kernels = KERNELS.lock().expect("kernels");
        let kernel = kernels.get_mut(&id).ok_or(Error::InvalidKernel)?;

        let kind = *kernel
            .signature
            .get(index as usize)
            .ok_or(Error::InvalidArgIndex)?;

        let context = kernel.context;
        let accepted = match (kind, &argument) {
            (ArgumentKind::Global | ArgumentKind::Constant, KernelArgument::Memory(_)) => true,
            (ArgumentKind::Local, KernelArgument::Local(size)) => *size != 0,
            (ArgumentKind::Value(expected), KernelArgument::Value(bytes)) => {
                bytes.len() == expected
            }
            _ => false,
        };

        if !accepted {
            return Err(match (kind, &argument) {
                (ArgumentKind::Value(_), _) => Error::InvalidArgSize,
                (ArgumentKind::Global | ArgumentKind::Constant, _) => Error::InvalidArgSize,
                (ArgumentKind::Local, KernelArgument::Local(_)) => Error::InvalidArgSize,
                _ => Error::InvalidArgValue,
            });
        }

        if let KernelArgument::Memory(Some(buffer)) = argument {
            Buffer::exists(buffer)?;

            if !Buffer::share_context(&[buffer], context) {
                return Err(Error::InvalidContext);
            }
        }

        kernel.arguments[index as usize] = Some(argument);

        Ok(())
    }

    fn signature(module: &bitcode::Module, function: bitcode::Id) -> Result<Vec<ArgumentKind>> {
        module
            .function(function)
            .map_err(|_| Error::InvalidKernelDefinition)?
            .parameters
            .iter()
            .map(
                |parameter| match module.storage_class(parameter.result_type) {
                    Ok(bitcode::StorageClass::CrossWorkgroup) => Ok(ArgumentKind::Global),
                    Ok(bitcode::StorageClass::Workgroup) => Ok(ArgumentKind::Local),
                    Ok(bitcode::StorageClass::UniformConstant) => Ok(ArgumentKind::Constant),
                    Ok(_) => Err(Error::InvalidKernelDefinition),
                    Err(_) => module
                        .layout(parameter.result_type)
                        .map(|layout| ArgumentKind::Value(layout.size))
                        .map_err(|_| Error::InvalidKernelDefinition),
                },
            )
            .collect()
    }

    fn local_bytes(&self) -> usize {
        self.arguments
            .iter()
            .flatten()
            .filter_map(|argument| match argument {
                KernelArgument::Local(size) => Some(align_up(*size)),
                _ => None,
            })
            .sum::<usize>()
            + self.static_local
    }

    fn snapshot(id: KernelId) -> Result<(Launch, Option<[u32; 3]>, ContextId)> {
        let kernels = KERNELS.lock().expect("kernels");
        let kernel = kernels.get(&id).ok_or(Error::InvalidKernel)?;

        let arguments = kernel
            .arguments
            .iter()
            .cloned()
            .collect::<Option<Vec<KernelArgument>>>()
            .ok_or(Error::InvalidKernelArgs)?;

        let mut arena = align_up(kernel.static_local);
        let local_offsets = arguments
            .iter()
            .map(|argument| match argument {
                KernelArgument::Local(size) => {
                    let offset = arena;
                    arena += align_up(*size);

                    offset
                }
                _ => 0,
            })
            .collect();

        Ok((
            Launch {
                module: Arc::clone(&kernel.module),
                function: kernel.function,
                arguments,
                local_offsets,
                arena,
            },
            kernel.required_local_size,
            kernel.context,
        ))
    }
}

struct Bound {
    base: u64,
    size: u64,
}

impl Launch {
    fn run(&self, geometry: &Grid) -> Result<()> {
        let (arguments, bounds) = self.bind()?;
        let group_counts = geometry.work_group_count();
        let mut arena = vec![0u8; self.arena];

        for z in 0..group_counts[2] {
            for y in 0..group_counts[1] {
                for x in 0..group_counts[0] {
                    arena.fill(0);
                    self.run_group(geometry, [x, y, z], &arguments, &bounds, &mut arena)?;
                }
            }
        }

        Ok(())
    }

    fn bind(&self) -> Result<(Vec<interpreter::Argument>, Vec<Bound>)> {
        let mut buffers = BUFFERS.lock().expect("buffers");
        let mut bounds = Vec::new();

        let arguments = self
            .arguments
            .iter()
            .zip(&self.local_offsets)
            .map(|(argument, offset)| {
                Ok(match argument {
                    KernelArgument::Memory(None) => interpreter::Argument::Buffer(0),
                    KernelArgument::Memory(Some(id)) => {
                        let buffer = buffers.get_mut(id).ok_or(Error::InvalidMemObject)?;
                        let base = buffer.base().as_ptr() as u64;

                        bounds.push(Bound { base, size: buffer.size as u64 });

                        interpreter::Argument::Buffer(base)
                    }
                    KernelArgument::Local(_) => interpreter::Argument::Buffer(*offset as u64),
                    KernelArgument::Value(bytes) => interpreter::Argument::Value(bytes.clone()),
                })
            })
            .collect::<Result<Vec<interpreter::Argument>>>()?;

        Ok((arguments, bounds))
    }

    fn run_group(
        &self,
        geometry: &Grid,
        group: [u64; 3],
        arguments: &[interpreter::Argument],
        bounds: &[Bound],
        arena: &mut [u8],
    ) -> Result<()> {
        let work_group_size = geometry.work_group_size();

        let mut items = (0..work_group_size)
            .map(|_| {
                interpreter::Interpreter::new(
                    Arc::clone(&self.module),
                    self.function,
                    arguments,
                    FUEL,
                )
                .map_err(trap)
            })
            .collect::<Result<Vec<interpreter::Interpreter>>>()?;

        let mut replies: Vec<Option<interpreter::Resume>> =
            vec![Some(interpreter::Resume::Start); work_group_size];
        let mut parked = vec![false; work_group_size];

        loop {
            let mut progressed = false;

            for lane in 0..work_group_size {
                let Some(mut reply) = replies[lane].take() else {
                    continue;
                };

                progressed = true;

                loop {
                    match items[lane].resume(reply).map_err(trap)? {
                        None => break,
                        Some(interpreter::YieldReason::ControlBarrier { .. }) => {
                            parked[lane] = true;

                            break;
                        }
                        Some(reason) => {
                            reply = service(reason, geometry, group, lane, bounds, arena)?;
                        }
                    }
                }
            }

            if progressed {
                continue;
            }

            if !parked.iter().any(|waiting| *waiting) {
                return Ok(());
            }

            for lane in 0..work_group_size {
                if parked[lane] {
                    parked[lane] = false;
                    replies[lane] = Some(interpreter::Resume::Ack);
                }
            }
        }
    }
}

fn trap(error: interpreter::Error) -> Error {
    match error {
        interpreter::Error::OutOfFuel
        | interpreter::Error::OutOfBounds
        | interpreter::Error::ReadOnlyRegion
        | interpreter::Error::UnsupportedRegion => Error::OutOfResources,
        _ => Error::InvalidProgramExecutable,
    }
}

fn builtin(kind: bitcode::Builtin, geometry: &Grid, group: [u64; 3], lane: usize) -> [u64; 3] {
    let local = [
        lane as u64 % geometry.local[0],
        (lane as u64 / geometry.local[0]) % geometry.local[1],
        lane as u64 / (geometry.local[0] * geometry.local[1]),
    ];

    match kind {
        bitcode::Builtin::LocalInvocationId => local,
        bitcode::Builtin::WorkgroupId => group,
        bitcode::Builtin::NumWorkgroups => geometry.work_group_count(),
        bitcode::Builtin::WorkgroupSize => geometry.local,
        bitcode::Builtin::GlobalOffset => geometry.offset,
        bitcode::Builtin::GlobalInvocationId => [
            geometry.offset[0] + group[0] * geometry.local[0] + local[0],
            geometry.offset[1] + group[1] * geometry.local[1] + local[1],
            geometry.offset[2] + group[2] * geometry.local[2] + local[2],
        ],
    }
}

fn within(bounds: &[Bound], address: u64, size: usize) -> Result<()> {
    bounds
        .iter()
        .any(|bound| address >= bound.base && address + size as u64 <= bound.base + bound.size)
        .then_some(())
        .ok_or(Error::OutOfResources)
}

fn signed(bits: u64, width: u32) -> i64 {
    let shift = 64 - width.min(64);

    ((bits << shift) as i64) >> shift
}

fn apply(
    operation: interpreter::Atomic,
    previous: u64,
    value: u64,
    comparator: u64,
    width: u32,
) -> u64 {
    match operation {
        interpreter::Atomic::Load => previous,
        interpreter::Atomic::Store | interpreter::Atomic::Exchange => value,
        interpreter::Atomic::CompareExchange => {
            if previous == comparator {
                value
            } else {
                previous
            }
        }
        interpreter::Atomic::Increment => previous.wrapping_add(1),
        interpreter::Atomic::Decrement => previous.wrapping_sub(1),
        interpreter::Atomic::Add => previous.wrapping_add(value),
        interpreter::Atomic::Sub => previous.wrapping_sub(value),
        interpreter::Atomic::UnsignedMin => previous.min(value),
        interpreter::Atomic::UnsignedMax => previous.max(value),
        interpreter::Atomic::SignedMin => {
            if signed(previous, width) <= signed(value, width) {
                previous
            } else {
                value
            }
        }
        interpreter::Atomic::SignedMax => {
            if signed(previous, width) >= signed(value, width) {
                previous
            } else {
                value
            }
        }
        interpreter::Atomic::And => previous & value,
        interpreter::Atomic::Or => previous | value,
        interpreter::Atomic::Xor => previous ^ value,
    }
}

fn local_slice(arena: &mut [u8], address: u64, size: usize) -> Result<&mut [u8]> {
    let start = address as usize;

    arena
        .get_mut(start..start + size)
        .ok_or(Error::OutOfResources)
}

fn service(
    reason: interpreter::YieldReason,
    geometry: &Grid,
    group: [u64; 3],
    lane: usize,
    bounds: &[Bound],
    arena: &mut [u8],
) -> Result<interpreter::Resume> {
    Ok(match reason {
        interpreter::YieldReason::Read { address, size } => {
            within(bounds, address, size)?;

            let mut bytes = vec![0u8; size];
            unsafe {
                core::ptr::copy_nonoverlapping(address as *const u8, bytes.as_mut_ptr(), size);
            }

            interpreter::Resume::Bytes(bytes)
        }
        interpreter::YieldReason::Write { address, bytes } => {
            within(bounds, address, bytes.len())?;

            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
            }

            interpreter::Resume::Ack
        }
        interpreter::YieldReason::ReadLocal { address, size } => {
            interpreter::Resume::Bytes(local_slice(arena, address, size)?.to_vec())
        }
        interpreter::YieldReason::WriteLocal { address, bytes } => {
            local_slice(arena, address, bytes.len())?.copy_from_slice(&bytes);

            interpreter::Resume::Ack
        }
        interpreter::YieldReason::Atomic {
            operation,
            address,
            width,
            local,
            value,
            comparator,
        } => {
            let size = width.div_ceil(8) as usize;
            let mut bytes = [0u8; 8];

            if local {
                bytes[..size].copy_from_slice(local_slice(arena, address, size)?);
            } else {
                within(bounds, address, size)?;
                unsafe {
                    core::ptr::copy_nonoverlapping(address as *const u8, bytes.as_mut_ptr(), size);
                }
            }

            let previous = u64::from_le_bytes(bytes);
            let updated = apply(operation, previous, value, comparator, width);
            let written = updated.to_le_bytes();

            if local {
                local_slice(arena, address, size)?.copy_from_slice(&written[..size]);
            } else {
                unsafe {
                    core::ptr::copy_nonoverlapping(written.as_ptr(), address as *mut u8, size);
                }
            }

            interpreter::Resume::Scalar(previous)
        }
        interpreter::YieldReason::Builtin(kind) => {
            interpreter::Resume::Builtin(builtin(kind, geometry, group, lane))
        }
        interpreter::YieldReason::MemoryBarrier { .. }
        | interpreter::YieldReason::ControlBarrier { .. } => interpreter::Resume::Ack,
    })
}

pub struct Buffer {
    reference_count: u32,
    context: ContextId,
    flags: MemFlags,
    size: usize,
    external: SharedMemoryPointer, // TODO: this is just host_pointer?
    storage: Option<Vec<u8>>,
    parent: Option<BufferId>,
    origin: usize,
    map_count: u32,
    maps: Vec<(usize, usize, MapFlags)>,
    destructors: Vec<DestructorNotify>,
}

impl Buffer {
    pub fn create(context: ContextId, flags: MemFlags, size: usize) -> Result<BufferId> {
        Context::exists(context)?;

        if size == 0 || size as u64 > MAX_MEM_ALLOC_SIZE {
            return Err(Error::InvalidBufferSize);
        }

        let (external, storage) = match flags.storage {
            Storage::Borrowed(pointer) => (pointer, None),
            Storage::Owned => (SharedMemoryPointer::NULL, Some(vec![0u8; size])),
            Storage::Copied(pointer) => {
                let mut storage = vec![0u8; size];

                unsafe {
                    core::ptr::copy_nonoverlapping(pointer.as_ptr(), storage.as_mut_ptr(), size);
                }

                (SharedMemoryPointer::NULL, Some(storage))
            }
        };

        Context::retain(context)?;

        let id = BufferId(next_object_id());

        BUFFERS.lock().expect("buffers").insert(
            id,
            Self {
                reference_count: 1,
                context,
                flags,
                size,
                external,
                storage,
                parent: None,
                origin: 0,
                map_count: 0,
                maps: Vec::new(),
                destructors: Vec::new(),
            },
        );

        Ok(id)
    }

    pub fn create_sub(
        parent: BufferId,
        flags: MemFlags,
        origin: usize,
        size: usize,
    ) -> Result<BufferId> {
        if !matches!(flags.storage, Storage::Owned) || flags.alloc_host {
            return Err(Error::InvalidValue);
        }

        let mut buffers = BUFFERS.lock().expect("buffers");
        let owner = buffers.get_mut(&parent).ok_or(Error::InvalidMemObject)?;

        if owner.parent.is_some() {
            return Err(Error::InvalidMemObject);
        }

        if size == 0 {
            return Err(Error::InvalidBufferSize);
        }

        if origin.checked_add(size).is_none_or(|end| end > owner.size) {
            return Err(Error::InvalidValue);
        }

        if !origin.is_multiple_of(Device::MEM_BASE_ADDR_ALIGN) {
            return Err(Error::MisalignedSubBufferOffset);
        }

        if !flags.narrows(owner.flags) {
            return Err(Error::InvalidValue);
        }

        let inherited = flags.inherit(owner.flags);
        let external = owner.base().offset(origin);
        let context = owner.context;

        owner.reference_count += 1;

        let id = BufferId(next_object_id());

        buffers.insert(
            id,
            Self {
                reference_count: 1,
                context,
                flags: inherited,
                size,
                external,
                storage: None,
                parent: Some(parent),
                origin,
                map_count: 0,
                maps: Vec::new(),
                destructors: Vec::new(),
            },
        );

        Ok(id)
    }

    pub fn retain(id: BufferId) -> Result<()> {
        BUFFERS
            .lock()
            .expect("buffers")
            .get_mut(&id)
            .map(|buffer| buffer.reference_count += 1)
            .ok_or(Error::InvalidMemObject)
    }

    pub fn release(id: BufferId) -> Result<()> {
        let dropped = {
            let mut buffers = BUFFERS.lock().expect("buffers");
            let buffer = buffers.get_mut(&id).ok_or(Error::InvalidMemObject)?;

            buffer.reference_count -= 1;
            if buffer.reference_count != 0 {
                return Ok(());
            }

            buffers.remove(&id).expect("buffer")
        };

        for notify in dropped.destructors.into_iter().rev() {
            notify(id);
        }

        match dropped.parent {
            Some(parent) => Self::release(parent),
            None => Context::release(dropped.context),
        }
    }

    pub fn exists(id: BufferId) -> Result<()> {
        BUFFERS
            .lock()
            .expect("buffers")
            .contains_key(&id)
            .then_some(())
            .ok_or(Error::InvalidMemObject)
    }

    pub fn info(id: BufferId, param: MemInfo) -> Result<InfoValue> {
        let buffers = BUFFERS.lock().expect("buffers");
        let buffer = buffers.get(&id).ok_or(Error::InvalidMemObject)?;

        Ok(match param {
            MemInfo::Type => InfoValue::MemObjectType(MemObjectType::Buffer),
            MemInfo::Flags => InfoValue::MemFlags(buffer.flags),
            MemInfo::Size => InfoValue::Size(buffer.size),
            MemInfo::HostPtr => InfoValue::HostPointer(buffer.host_pointer()),
            MemInfo::MapCount => InfoValue::Uint(buffer.map_count),
            MemInfo::ReferenceCount => InfoValue::Uint(buffer.reference_count),
            MemInfo::Context => InfoValue::Context(buffer.context),
            MemInfo::AssociatedMemObject => InfoValue::Buffer(buffer.parent),
            MemInfo::Offset => InfoValue::Size(buffer.origin),
        })
    }

    pub fn add_destructor(id: BufferId, notify: DestructorNotify) -> Result<()> {
        BUFFERS
            .lock()
            .expect("buffers")
            .get_mut(&id)
            .map(|buffer| buffer.destructors.push(notify))
            .ok_or(Error::InvalidMemObject)
    }

    fn size(id: BufferId) -> Result<usize> {
        BUFFERS
            .lock()
            .expect("buffers")
            .get(&id)
            .map(|buffer| buffer.size)
            .ok_or(Error::InvalidMemObject)
    }

    fn share_context(ids: &[BufferId], context: ContextId) -> bool {
        let buffers = BUFFERS.lock().expect("buffers");

        ids.iter().all(|id| {
            buffers
                .get(id)
                .is_none_or(|buffer| buffer.context == context)
        })
    }

    fn map(
        id: BufferId,
        offset: usize,
        size: usize,
        flags: MapFlags,
    ) -> Result<SharedMemoryPointer> {
        let mut buffers = BUFFERS.lock().expect("buffers");
        let buffer = buffers.get_mut(&id).ok_or(Error::InvalidMemObject)?;

        if size == 0 || offset.checked_add(size).is_none_or(|end| end > buffer.size) {
            return Err(Error::InvalidValue);
        }

        let clashes = buffer.maps.iter().any(|(at, extent, mapped)| {
            let overlaps = offset < at + extent && *at < offset + size;

            overlaps && flags.writes() && mapped.writes()
        });

        if clashes {
            return Err(Error::InvalidOperation);
        }

        buffer.maps.push((offset, size, flags));
        buffer.map_count += 1;

        Ok(buffer.base().offset(offset))
    }

    fn undo_map(id: BufferId, mapped: SharedMemoryPointer) {
        if let Some(buffer) = BUFFERS.lock().expect("buffers").get_mut(&id) {
            buffer.unmap(mapped);
        }
    }

    fn is_mapped(id: BufferId, mapped: SharedMemoryPointer) -> bool {
        let mut buffers = BUFFERS.lock().expect("buffers");
        let Some(buffer) = buffers.get_mut(&id) else {
            return false;
        };

        let offset = mapped.distance(buffer.base());

        buffer.maps.iter().any(|(at, _, _)| *at == offset)
    }

    fn base(&mut self) -> SharedMemoryPointer {
        self.storage
            .as_mut()
            .map_or(self.external, |storage| unsafe {
                SharedMemoryPointer::new(storage.as_mut_ptr())
            })
    }

    fn host_access(id: BufferId) -> Result<HostAccess> {
        BUFFERS
            .lock()
            .expect("buffers")
            .get(&id)
            .map(|buffer| buffer.flags.host_access)
            .ok_or(Error::InvalidMemObject)
    }

    fn host_may_read(id: BufferId) -> Result<()> {
        match Self::host_access(id)? {
            HostAccess::WriteOnly | HostAccess::NoAccess => Err(Error::InvalidOperation),
            HostAccess::Unspecified | HostAccess::ReadOnly => Ok(()),
        }
    }

    fn host_may_write(id: BufferId) -> Result<()> {
        match Self::host_access(id)? {
            HostAccess::ReadOnly | HostAccess::NoAccess => Err(Error::InvalidOperation),
            HostAccess::Unspecified | HostAccess::WriteOnly => Ok(()),
        }
    }

    fn host_pointer(&self) -> SharedMemoryPointer {
        // TODO: both are the same here?
        match self.parent {
            Some(_) if !self.external.is_null() => self.external,
            _ => self.flags.host_pointer(),
        }
    }

    fn unmap(&mut self, mapped: SharedMemoryPointer) {
        let offset = mapped.distance(self.base());

        if let Some(index) = self.maps.iter().position(|(at, _, _)| *at == offset) {
            self.maps.remove(index);
            self.map_count -= 1;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfilingInfo {
    Queued,
    Submit,
    Start,
    End,
}

pub struct Event {
    reference_count: u32,
    context: ContextId,
    queue: Option<QueueId>,
    command_type: CommandType,
    status: Status,
    callbacks: Vec<(Status, EventNotify)>,
    stamps: [Option<u64>; 4],
}

impl Event {
    pub fn create_user(context: ContextId) -> Result<EventId> {
        Context::exists(context)?;

        Ok(Self::create(
            context,
            None,
            CommandType::User,
            Status::Submitted,
        ))
    }

    fn create(
        context: ContextId,
        queue: Option<QueueId>,
        command_type: CommandType,
        status: Status,
    ) -> EventId {
        let id = EventId(next_object_id());

        EVENTS.lock().expect("events").insert(
            id,
            Self {
                reference_count: 1,
                context,
                queue,
                command_type,
                status,
                callbacks: Vec::new(),
                stamps: [now(), None, None, None],
            },
        );

        id
    }

    pub fn retain(id: EventId) -> Result<()> {
        EVENTS
            .lock()
            .expect("events")
            .get_mut(&id)
            .map(|event| event.reference_count += 1)
            .ok_or(Error::InvalidEvent)
    }

    pub fn release(id: EventId) -> Result<()> {
        let mut events = EVENTS.lock().expect("events");
        let event = events.get_mut(&id).ok_or(Error::InvalidEvent)?;

        event.reference_count -= 1;
        if event.reference_count == 0 {
            events.remove(&id);
        }

        Ok(())
    }

    pub fn exists(id: EventId) -> Result<()> {
        EVENTS
            .lock()
            .expect("events")
            .contains_key(&id)
            .then_some(())
            .ok_or(Error::InvalidEvent)
    }

    pub fn info(id: EventId, param: EventInfo) -> Result<InfoValue> {
        let events = EVENTS.lock().expect("events");
        let event = events.get(&id).ok_or(Error::InvalidEvent)?;

        Ok(match param {
            EventInfo::CommandQueue => InfoValue::Queue(event.queue),
            EventInfo::Context => InfoValue::Context(event.context),
            EventInfo::CommandType => InfoValue::CommandType(event.command_type),
            EventInfo::ExecutionStatus => InfoValue::Status(event.status),
            EventInfo::ReferenceCount => InfoValue::Uint(event.reference_count),
        })
    }

    pub fn profiling_info(id: EventId, param: ProfilingInfo) -> Result<InfoValue> {
        let events = EVENTS.lock().expect("events");
        let event = events.get(&id).ok_or(Error::InvalidEvent)?;

        let queue = event.queue.ok_or(Error::ProfilingInfoNotAvailable)?;
        if event.status != Status::Complete {
            return Err(Error::ProfilingInfoNotAvailable);
        }

        let stamp = event.stamps[param as usize];
        drop(events);

        if !CommandQueue::profiles(queue)? {
            return Err(Error::ProfilingInfoNotAvailable);
        }

        stamp
            .map(InfoValue::Ulong)
            .ok_or(Error::ProfilingInfoNotAvailable)
    }

    fn stamp(id: EventId, param: ProfilingInfo) {
        if let Some(event) = EVENTS.lock().expect("events").get_mut(&id) {
            event.stamps[param as usize] = now();
        }
    }

    pub fn set_user_status(id: EventId, status: Status) -> Result<()> {
        if !matches!(status, Status::Complete | Status::Terminated(_)) {
            return Err(Error::InvalidValue);
        }

        let settled = {
            let events = EVENTS.lock().expect("events");
            let event = events.get(&id).ok_or(Error::InvalidEvent)?;

            if event.command_type != CommandType::User {
                return Err(Error::InvalidEvent);
            }

            event.status != Status::Submitted
        };

        if settled {
            return Err(Error::InvalidOperation);
        }

        Self::set_status(id, status);

        Ok(())
    }

    pub fn add_callback(id: EventId, wanted: Status, notify: EventNotify) -> Result<()> {
        let mut events = EVENTS.lock().expect("events");
        let event = events.get_mut(&id).ok_or(Error::InvalidEvent)?;
        let status = event.status;

        if !status.reached(wanted) {
            event.callbacks.push((wanted, notify));

            return Ok(());
        }

        drop(events);
        notify(id, status);

        Ok(())
    }

    pub fn wait(ids: &[EventId]) -> Result<()> {
        let Some(first) = ids.first() else {
            return Ok(());
        };

        let context = Self::context(*first)?;
        if !Self::share_context(ids, context) {
            return Err(Error::InvalidContext);
        }

        loop {
            let generation = progress();

            let statuses = {
                let events = EVENTS.lock().expect("events");

                ids.iter()
                    .map(|id| events.get(id).map(|event| event.status))
                    .collect::<Option<Vec<Status>>>()
                    .ok_or(Error::InvalidEvent)?
            };

            let settled = statuses
                .iter()
                .all(|status| *status == Status::Complete || status.is_terminated());

            if settled {
                if statuses.iter().any(|status| status.is_terminated()) {
                    return Err(Error::ExecStatusErrorForEventsInWaitList);
                }

                return Ok(());
            }

            await_progress(generation);
        }
    }

    fn context(id: EventId) -> Result<ContextId> {
        EVENTS
            .lock()
            .expect("events")
            .get(&id)
            .map(|event| event.context)
            .ok_or(Error::InvalidEvent)
    }

    fn share_context(ids: &[EventId], context: ContextId) -> bool {
        let events = EVENTS.lock().expect("events");

        ids.iter()
            .all(|id| events.get(id).is_none_or(|event| event.context == context))
    }

    fn set_status(id: EventId, status: Status) {
        let fired = {
            let mut events = EVENTS.lock().expect("events");
            let Some(event) = events.get_mut(&id) else {
                return;
            };

            event.status = status;

            let (fired, pending): (Vec<_>, Vec<_>) = event
                .callbacks
                .drain(..)
                .partition(|(wanted, _)| status.reached(*wanted));

            event.callbacks = pending;

            fired
        };

        signal_progress();

        for (_, notify) in fired {
            notify(id, status);
        }
    }
}

// TODO: should this be called upsert_worker?
fn update_worker() {
    let mut worker = WORKER.lock().expect("worker");
    let idle = QUEUES.lock().expect("queues").is_empty();

    match (idle, worker.take()) {
        (false, None) => {
            SHUTDOWN.store(false, Ordering::Relaxed);
            *worker = Some(std::thread::spawn(run_worker));
        }
        (true, Some(running)) => {
            SHUTDOWN.store(true, Ordering::Relaxed);
            signal_progress();
            running.join().expect("worker");
        }
        // TODO: is this branch even necessary? or can this branch just be ()?
        (_, existing) => *worker = existing,
    }
}

fn run_worker() {
    loop {
        let generation = progress();

        if SHUTDOWN.load(Ordering::Relaxed) {
            return;
        }

        if !step() {
            await_progress(generation);
        }
    }
}

fn step() -> bool {
    let Some((queue, enqueued, terminated)) = take_ready() else {
        return false;
    };

    let status = match terminated {
        Some(status) => status,
        None => {
            Event::stamp(enqueued.event, ProfilingInfo::Start);
            Event::set_status(enqueued.event, Status::Running);

            match enqueued.command.execute() {
                Ok(()) => Status::Complete,
                Err(error) => Status::Failed(error),
            }
        }
    };

    for buffer in enqueued.command.buffers() {
        let _released = Buffer::release(buffer);
    }

    for waited in &enqueued.wait {
        let _released = Event::release(*waited);
    }

    Event::stamp(enqueued.event, ProfilingInfo::End);

    if status.is_terminated()
        && let Ok(context) = Event::context(enqueued.event)
    {
        Context::report(context, &format!("command terminated: {status:?}"));
    }

    Event::set_status(enqueued.event, status);
    let _released = Event::release(enqueued.event);

    if let Some(queue) = QUEUES.lock().expect("queues").get_mut(&queue) {
        queue.running = false;
    }

    signal_progress();

    true
}

fn take_ready() -> Option<(QueueId, Enqueued, Option<Status>)> {
    let mut queues = QUEUES.lock().expect("queues");
    let events = EVENTS.lock().expect("events");

    let ready = queues.iter_mut().find_map(|(id, queue)| {
        let head = queue.commands.front()?;
        let statuses: Vec<Status> = head
            .wait
            .iter()
            .filter_map(|waited| events.get(waited).map(|event| event.status))
            .collect();

        if let Some(failed) = statuses.iter().find(|status| status.is_terminated()) {
            return Some((*id, Some(*failed)));
        }

        statuses
            .iter()
            .all(|status| *status == Status::Complete)
            .then_some((*id, None))
    });

    let (id, terminated) = ready?;

    drop(events);

    let queue = queues.get_mut(&id).expect("queue");
    let head = queue.commands.pop_front().expect("command");
    queue.running = true;

    Some((id, head, terminated))
}
