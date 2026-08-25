use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

const MAX_WORK_GROUP_SIZE: usize = 256;
const MAX_MEM_ALLOC_SIZE: u64 = 1024 * 1024 * 1024;
const MEM_BASE_ADDR_ALIGN_BITS: u32 = 1024;

static NEXT_OBJECT_ID: AtomicU32 = AtomicU32::new(1);
static CONTEXTS: Mutex<BTreeMap<ContextId, Context>> = Mutex::new(BTreeMap::new());
static QUEUES: Mutex<BTreeMap<QueueId, CommandQueue>> = Mutex::new(BTreeMap::new());
static BUFFERS: Mutex<BTreeMap<BufferId, Buffer>> = Mutex::new(BTreeMap::new());
static EVENTS: Mutex<BTreeMap<EventId, Event>> = Mutex::new(BTreeMap::new());

static PROGRESS: Mutex<u64> = Mutex::new(0);
static PROGRESSED: Condvar = Condvar::new();
static WORKER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn next_object_id() -> u32 {
    NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed)
}

fn progress() -> u64 {
    *PROGRESS.lock().expect("progress")
}

fn signal_progress() {
    *PROGRESS.lock().expect("progress") += 1;
    PROGRESSED.notify_all();
}

fn await_progress(generation: u64) {
    let progress = PROGRESS.lock().expect("progress");
    let _settled = PROGRESSED
        .wait_while(progress, |current| *current == generation)
        .expect("progress");
}

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

pub type Notify = Arc<dyn Fn(&str) + Send + Sync>;
pub type EventNotify = Box<dyn FnOnce(EventId, Status) + Send>;
pub type DestructorNotify = Box<dyn FnOnce(BufferId) + Send>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostPointer(*mut u8);

unsafe impl Send for HostPointer {}

impl HostPointer {
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
    Borrowed(HostPointer),
    Copied(HostPointer),
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

    fn host_pointer(self) -> HostPointer {
        match self.storage {
            Storage::Borrowed(pointer) => pointer,
            Storage::Owned | Storage::Copied(_) => HostPointer::NULL,
        }
    }

    fn narrows(self, parent: Self) -> bool {
        let access = match (parent.access, self.access) {
            (_, Access::Unspecified) => true,
            (Access::ReadOnly, wanted) => wanted == Access::ReadOnly,
            (Access::WriteOnly, wanted) => wanted == Access::WriteOnly,
            _ => true,
        };

        let host = match (parent.host_access, self.host_access) {
            (_, HostAccess::Unspecified) => true,
            (HostAccess::NoAccess, _) => false,
            _ => true,
        };

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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Queued,
    Submitted,
    Running,
    Complete,
    Terminated(i32),
}

impl Status {
    fn rank(self) -> i32 {
        match self {
            Status::Queued => 3,
            Status::Submitted => 2,
            Status::Running => 1,
            Status::Complete => 0,
            Status::Terminated(code) => code,
        }
    }

    fn reached(self, wanted: Status) -> bool {
        matches!(self, Status::Terminated(_)) || self.rank() <= wanted.rank()
    }

    fn is_terminated(self) -> bool {
        matches!(self, Status::Terminated(_))
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
    Text(&'static str),
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
    HostPointer(HostPointer),
    Context(ContextId),
    Queue(Option<QueueId>),
    Buffer(Option<BufferId>),
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
            PlatformInfo::Profile => InfoValue::Text("EMBEDDED_PROFILE"),
            PlatformInfo::Version => InfoValue::Text("OpenCL 1.2 vodd"),
            PlatformInfo::Name => InfoValue::Text("vodd"),
            PlatformInfo::Vendor => InfoValue::Text("vodd"),
            PlatformInfo::Extensions => InfoValue::Text("cles_khr_int64"),
        }
    }
}

impl Device {
    pub const TYPE: DeviceType = DeviceType::GPU;

    pub const MEM_BASE_ADDR_ALIGN: usize = MEM_BASE_ADDR_ALIGN_BITS as usize / 8;

    pub fn info(self, param: DeviceInfo) -> InfoValue {
        match param {
            DeviceInfo::Type => InfoValue::DeviceType(Self::TYPE),
            DeviceInfo::VendorId => InfoValue::Uint(0),
            DeviceInfo::MaxComputeUnits => InfoValue::Uint(1),
            DeviceInfo::MaxWorkItemDimensions => InfoValue::Uint(3),
            DeviceInfo::MaxWorkGroupSize => InfoValue::Size(MAX_WORK_GROUP_SIZE),
            DeviceInfo::MaxWorkItemSizes => InfoValue::Sizes(vec![MAX_WORK_GROUP_SIZE; 3]),
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
            DeviceInfo::GlobalMemSize => InfoValue::Ulong(4 * 1024 * 1024 * 1024),
            DeviceInfo::MaxConstantBufferSize => InfoValue::Ulong(64 * 1024),
            DeviceInfo::MaxConstantArgs => InfoValue::Uint(8),
            DeviceInfo::LocalMemType => InfoValue::LocalMemType(LocalMemType::Local),
            DeviceInfo::LocalMemSize => InfoValue::Ulong(32 * 1024),
            DeviceInfo::ErrorCorrectionSupport => InfoValue::Bool(false),
            DeviceInfo::ProfilingTimerResolution => InfoValue::Size(1),
            DeviceInfo::EndianLittle => InfoValue::Bool(true),
            DeviceInfo::Available => InfoValue::Bool(true),
            DeviceInfo::CompilerAvailable => InfoValue::Bool(true),
            DeviceInfo::LinkerAvailable => InfoValue::Bool(true),
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
            DeviceInfo::Name => InfoValue::Text("vodd"),
            DeviceInfo::Vendor => InfoValue::Text("vodd"),
            DeviceInfo::DriverVersion => InfoValue::Text("0.0.1"),
            DeviceInfo::Profile => InfoValue::Text("EMBEDDED_PROFILE"),
            DeviceInfo::Version => InfoValue::Text("OpenCL 1.2 vodd"),
            DeviceInfo::OpenclCVersion => InfoValue::Text("OpenCL C 1.2 "),
            DeviceInfo::Extensions => InfoValue::Text("cles_khr_int64"),
            DeviceInfo::BuiltInKernels => InfoValue::Text(""),
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
        if devices.is_empty() {
            return Err(Error::InvalidValue);
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
    Host(HostPointer),
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

    pub fn host(target: HostPointer) -> Self {
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

    fn base(&self, buffers: &mut BTreeMap<BufferId, Buffer>) -> Option<HostPointer> {
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
        mapped: HostPointer,
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
            Command::Nothing => Vec::new(),
        }
    }

    fn execute(&self) {
        let mut buffers = BUFFERS.lock().expect("buffers");

        match self {
            Command::Copy { source, destination, region } => {
                let Some(from) = source.base(&mut buffers) else {
                    return;
                };
                let Some(into) = destination.base(&mut buffers) else {
                    return;
                };

                copy_slabs(from, source, into, destination, region);
            }
            Command::Fill { destination, region, pattern } => {
                let Some(into) = destination.base(&mut buffers) else {
                    return;
                };

                fill_slab(into, destination, region, pattern);
            }
            Command::Unmap { buffer, mapped } => {
                if let Some(buffer) = buffers.get_mut(buffer) {
                    buffer.unmap(*mapped);
                }
            }
            Command::Nothing => {}
        }
    }
}

fn copy_slabs(
    from: HostPointer,
    source: &Slab,
    into: HostPointer,
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

fn fill_slab(into: HostPointer, destination: &Slab, region: &[usize; 3], pattern: &[u8]) {
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

    pub fn flush(id: QueueId) -> Result<()> {
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

    pub fn read_buffer(
        id: QueueId,
        buffer: BufferId,
        offset: usize,
        size: usize,
        host: HostPointer,
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

    pub fn write_buffer(
        id: QueueId,
        buffer: BufferId,
        offset: usize,
        size: usize,
        host: HostPointer,
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

    pub fn map_buffer(
        id: QueueId,
        buffer: BufferId,
        offset: usize,
        size: usize,
        flags: MapFlags,
        wait: Vec<EventId>,
    ) -> Result<(HostPointer, EventId)> {
        let mapped = Buffer::map(buffer, offset, size, flags)?;

        match Self::enqueue(id, Command::Nothing, CommandType::MapBuffer, wait) {
            Ok(event) => Ok((mapped, event)),
            Err(error) => {
                Buffer::undo_map(buffer, mapped);

                Err(error)
            }
        }
    }

    pub fn unmap(
        id: QueueId,
        buffer: BufferId,
        mapped: HostPointer,
        wait: Vec<EventId>,
    ) -> Result<EventId> {
        Buffer::exists(buffer)?;

        if !Buffer::is_mapped(buffer, mapped) {
            return Err(Error::InvalidValue);
        }

        Self::enqueue(
            id,
            Command::Unmap { buffer, mapped },
            CommandType::UnmapMemObject,
            wait,
        )
    }

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

    pub fn marker(id: QueueId, wait: Vec<EventId>) -> Result<EventId> {
        Self::enqueue(id, Command::Nothing, CommandType::Marker, wait)
    }

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

        signal_progress();

        Ok(event)
    }
}

pub struct Buffer {
    reference_count: u32,
    context: ContextId,
    flags: MemFlags,
    size: usize,
    external: HostPointer,
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
            Storage::Owned => (HostPointer::NULL, Some(vec![0u8; size])),
            Storage::Copied(pointer) => {
                let mut storage = vec![0u8; size];

                unsafe {
                    core::ptr::copy_nonoverlapping(pointer.as_ptr(), storage.as_mut_ptr(), size);
                }

                (HostPointer::NULL, Some(storage))
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

    fn map(id: BufferId, offset: usize, size: usize, flags: MapFlags) -> Result<HostPointer> {
        let mut buffers = BUFFERS.lock().expect("buffers");
        let buffer = buffers.get_mut(&id).ok_or(Error::InvalidMemObject)?;

        if size == 0 || offset.checked_add(size).is_none_or(|end| end > buffer.size) {
            return Err(Error::InvalidValue);
        }

        let clashes = buffer.maps.iter().any(|(at, extent, mapped)| {
            let overlaps = offset < at + extent && *at < offset + size;

            overlaps && (flags.writes() || mapped.writes())
        });

        if clashes {
            return Err(Error::InvalidOperation);
        }

        buffer.maps.push((offset, size, flags));
        buffer.map_count += 1;

        Ok(buffer.base().offset(offset))
    }

    fn undo_map(id: BufferId, mapped: HostPointer) {
        if let Some(buffer) = BUFFERS.lock().expect("buffers").get_mut(&id) {
            buffer.unmap(mapped);
        }
    }

    fn is_mapped(id: BufferId, mapped: HostPointer) -> bool {
        let mut buffers = BUFFERS.lock().expect("buffers");
        let Some(buffer) = buffers.get_mut(&id) else {
            return false;
        };

        let offset = mapped.distance(buffer.base());

        buffer.maps.iter().any(|(at, _, _)| *at == offset)
    }

    fn base(&mut self) -> HostPointer {
        self.storage
            .as_mut()
            .map_or(self.external, |storage| unsafe {
                HostPointer::new(storage.as_mut_ptr())
            })
    }

    fn host_pointer(&self) -> HostPointer {
        match self.parent {
            Some(_) if !self.external.is_null() => self.external,
            _ => self.flags.host_pointer(),
        }
    }

    fn unmap(&mut self, mapped: HostPointer) {
        let offset = mapped.distance(self.base());

        if let Some(index) = self.maps.iter().position(|(at, _, _)| *at == offset) {
            self.maps.remove(index);
            self.map_count -= 1;
        }
    }
}

pub struct Event {
    reference_count: u32,
    context: ContextId,
    queue: Option<QueueId>,
    command_type: CommandType,
    status: Status,
    callbacks: Vec<(Status, EventNotify)>,
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

            if statuses.iter().any(|status| status.is_terminated()) {
                return Err(Error::ExecStatusErrorForEventsInWaitList);
            }

            if statuses.iter().all(|status| *status == Status::Complete) {
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
            Event::set_status(enqueued.event, Status::Running);
            enqueued.command.execute();

            Status::Complete
        }
    };

    for buffer in enqueued.command.buffers() {
        let _released = Buffer::release(buffer);
    }

    for waited in &enqueued.wait {
        let _released = Event::release(*waited);
    }

    if let Status::Terminated(code) = status
        && let Ok(context) = Event::context(enqueued.event)
    {
        Context::report(
            context,
            &format!("command terminated with status {code} by a failed event in its wait list"),
        );
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
