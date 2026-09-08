use crate::address;
use crate::bitcode;
use crate::detectors;
use crate::value;

pub use crate::bitcode::AtomicOp as Atomic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Bitcode(bitcode::Error),
    Value(value::Error),
    UnboundValue(bitcode::Id),
    ReadOnlyRegion,
    OutOfBounds,
    InvalidAddress,
    ArgumentMismatch,
    UnsupportedRegion,
    Unreachable,
    EndOfBlock,
    NoFrame,
    NoPredecessor,
    OutOfFuel,
    OutOfSlots,
    UnexpectedResume,
}

impl From<value::Error> for Error {
    fn from(error: value::Error) -> Error {
        Error::Value(error)
    }
}

impl From<bitcode::Error> for Error {
    fn from(error: bitcode::Error) -> Error {
        Error::Bitcode(error)
    }
}

impl From<address::Invalid> for Error {
    fn from(_: address::Invalid) -> Error {
        Error::InvalidAddress
    }
}

#[derive(Debug, Clone)]
pub enum Argument {
    Buffer(u64),
    Value(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YieldReason {
    Read {
        address: u64,
        size: usize,
    },
    ReadLocal {
        address: u64,
        size: usize,
    },
    Write {
        address: u64,
        bytes: Vec<u8>,
    },
    WriteLocal {
        address: u64,
        bytes: Vec<u8>,
    },
    Atomic {
        operation: Atomic,
        address: u64,
        width: u32,
        local: bool,
        value: u64,
        comparator: u64,
    },
    Builtin(bitcode::Builtin),
    MemoryBarrier {
        semantics: bitcode::MemorySemantics,
    },
    ControlBarrier {
        execution_scope: u64,
        semantics: bitcode::MemorySemantics,
    },
    Break,
    Diagnostic(detectors::DiagnosticList),
}

impl YieldReason {
    fn accepts(&self, reply: &Resume) -> bool {
        matches!(
            (self, reply),
            (
                YieldReason::Read { .. } | YieldReason::ReadLocal { .. },
                Resume::Bytes(_)
            ) | (
                YieldReason::Write { .. }
                    | YieldReason::WriteLocal { .. }
                    | YieldReason::MemoryBarrier { .. }
                    | YieldReason::ControlBarrier { .. }
                    | YieldReason::Break
                    | YieldReason::Diagnostic(_),
                Resume::Ack
            ) | (YieldReason::Atomic { .. }, Resume::Scalar(_))
                | (YieldReason::Builtin(_), Resume::Builtin(_))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resume {
    Start,
    Bytes(Vec<u8>),
    Scalar(u64),
    Builtin([u64; 3]),
    Ack,
}

#[derive(Debug, Clone)]
struct Frame {
    function: bitcode::Id,
    block: bitcode::Id,
    instruction: usize,
    previous_block: Option<bitcode::Id>,
    values: Vec<Option<value::Value>>,
    return_slot: Option<bitcode::Id>,
    storage_watermark: usize,
}

impl Frame {
    fn new(
        function: bitcode::Id,
        block: bitcode::Id,
        values: Vec<Option<value::Value>>,
        return_slot: Option<bitcode::Id>,
        storage_watermark: usize,
    ) -> Frame {
        Frame {
            function,
            block,
            instruction: 0,
            previous_block: None,
            values,
            return_slot,
            storage_watermark,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Interpreter {
    module: std::sync::Arc<bitcode::Module>,
    mutable_storage: address::Storage,
    constant_storage: address::Storage,
    private_addresses: Vec<Option<u64>>,
    local_addresses: std::sync::Arc<Vec<Option<u64>>>,
    frames: Vec<Frame>,
    storage: address::Storage,
    fuel: usize,
    debug: bool,
    checks: detectors::Checks,
    pending: Option<YieldReason>,
    stepped: bool,
    broke_at: Option<(bitcode::Id, u32)>,
    location: Option<bitcode::Location>,
}

pub fn builtin_to_addr(builtin: bitcode::Builtin) -> u64 {
    let slot = match builtin {
        bitcode::Builtin::NumWorkgroups => 100,
        bitcode::Builtin::WorkgroupSize => 200,
        bitcode::Builtin::WorkgroupId => 300,
        bitcode::Builtin::LocalInvocationId => 400,
        bitcode::Builtin::GlobalInvocationId => 500,
        bitcode::Builtin::GlobalOffset => 600,
    };

    address::encode(Some(address::Region::Builtin), slot, 0)
}

pub fn builtin_from_addr(address: u64) -> Result<bitcode::Builtin, Error> {
    let (region, slot, _) = address::decode(address);

    Ok(match (region, slot) {
        (Some(address::Region::Builtin), 100) => bitcode::Builtin::NumWorkgroups,
        (Some(address::Region::Builtin), 200) => bitcode::Builtin::WorkgroupSize,
        (Some(address::Region::Builtin), 300) => bitcode::Builtin::WorkgroupId,
        (Some(address::Region::Builtin), 400) => bitcode::Builtin::LocalInvocationId,
        (Some(address::Region::Builtin), 500) => bitcode::Builtin::GlobalInvocationId,
        (Some(address::Region::Builtin), 600) => bitcode::Builtin::GlobalOffset,
        (_, slot) => return Err(bitcode::Error::UnsupportedBuiltin(slot as u32).into()),
    })
}

fn private_layout(
    module: &bitcode::Module,
) -> Result<(Vec<Option<u64>>, address::Storage, address::Storage), Error> {
    let mut mutable_storage = address::Storage::new(address::Region::Mutable);
    let mut constant_storage = address::Storage::new(address::Region::Constant);
    let mut addresses = vec![None; module.bound()];

    for variable in module.variables() {
        let storage = match variable.storage {
            bitcode::StorageClass::Private => &mut mutable_storage,
            bitcode::StorageClass::UniformConstant => &mut constant_storage,
            _ => continue,
        };

        let pointee_type = module.pointee_type(variable.result_type)?;
        let layout = module.layout(pointee_type)?;
        let (size, alignment) = (layout.size, layout.alignment);
        let address = storage.allocate(size, alignment).ok_or(Error::OutOfSlots)?;

        addresses[variable.result as usize] = Some(address);

        if let Some(initializer) = variable.initializer {
            let value = value::constant(module, initializer)?;
            let destination = storage.write(address, size)?;

            value::encode(module, pointee_type, &value, destination)?;
        }
    }

    Ok((addresses, mutable_storage, constant_storage))
}

pub fn local_layout(
    module: &bitcode::Module,
) -> Result<(std::sync::Arc<Vec<Option<u64>>>, address::Storage), Error> {
    let mut addresses = vec![None; module.bound()];
    let mut storage = address::Storage::new(address::Region::Local);

    let stored = module
        .variables()
        .iter()
        .filter(|variable| variable.storage == bitcode::StorageClass::Workgroup);

    for variable in stored {
        let pointee_type = module.pointee_type(variable.result_type)?;
        let layout = module.layout(pointee_type)?;

        let address = storage
            .allocate(layout.size, layout.alignment)
            .ok_or(Error::OutOfSlots)?;

        addresses[variable.result as usize] = Some(address);
    }

    Ok((std::sync::Arc::new(addresses), storage))
}

fn address_of(addresses: &[Option<u64>], id: bitcode::Id) -> Result<u64, Error> {
    addresses
        .get(id as usize)
        .copied()
        .flatten()
        .ok_or(Error::UnboundValue(id))
}

fn insert(
    composite: value::Value,
    indices: &[u32],
    object: value::Value,
) -> Result<value::Value, Error> {
    let Some((index, rest)) = indices.split_first() else {
        return Ok(object);
    };

    match composite {
        value::Value::Composite(mut members) => {
            let slot = members.get_mut(*index as usize).ok_or(Error::OutOfBounds)?;
            *slot = insert(slot.clone(), rest, object)?;

            Ok(value::Value::Composite(members))
        }
        _ => Err(value::Error::NotAComposite.into()),
    }
}

impl Interpreter {
    pub fn new(
        module: std::sync::Arc<bitcode::Module>,
        function: bitcode::Id,
        arguments: &[Argument],
        local_addresses: std::sync::Arc<Vec<Option<u64>>>,
        fuel: usize,
        debug: bool,
        checks: detectors::Checks,
    ) -> Result<Interpreter, Error> {
        let (private_addresses, mutable_storage, constant_storage) = private_layout(&module)?;

        let entry = module.function(function)?;
        if entry.parameters.len() != arguments.len() {
            return Err(Error::ArgumentMismatch);
        }

        let mut values = vec![None; module.bound()];

        for (parameter, argument) in entry.parameters.iter().zip(arguments) {
            values[parameter.result as usize] = Some(match argument {
                Argument::Buffer(address) => value::Value::Pointer(value::Pointer {
                    address: *address,
                    pointee_type: module.pointee_type(parameter.result_type)?,
                }),
                Argument::Value(bytes) => value::decode(&module, parameter.result_type, bytes)?,
            });
        }

        let frame = Frame::new(function, entry.entry_label()?, values, None, 0);

        Ok(Interpreter {
            module,
            mutable_storage,
            constant_storage,
            private_addresses,
            local_addresses,
            frames: vec![frame],
            storage: address::Storage::new(address::Region::Invocation),
            fuel,
            debug,
            checks,
            pending: None,
            stepped: false,
            broke_at: None,
            location: None,
        })
    }

    pub fn location(&self) -> Option<bitcode::Location> {
        self.location
    }

    pub fn resume(&mut self, resume: Resume) -> Result<Option<YieldReason>, Error> {
        let mut resume = match (self.pending.take(), resume) {
            (None, Resume::Start) => None,
            (Some(YieldReason::Break), Resume::Ack) => {
                self.stepped = true;
                None
            }
            (Some(YieldReason::Diagnostic(_)), Resume::Ack) => None,
            (Some(reason), resume) if reason.accepts(&resume) => Some(resume),
            _ => return Err(Error::UnexpectedResume),
        };

        while !self.frames.is_empty() {
            if self.fuel == 0 {
                return Err(Error::OutOfFuel);
            }
            self.fuel -= 1;

            if let Some(reason) = self.resume_debug(resume.as_ref())? {
                return Ok(Some(reason));
            }

            if let Some(reason) = self.resume_inner(resume.take())? {
                return Ok(Some(reason));
            }
        }

        Ok(None)
    }

    fn resume_debug(&mut self, resume: Option<&Resume>) -> Result<Option<YieldReason>, Error> {
        let frame = self.frame()?;
        let function = self.module.function(frame.function)?;
        let block = function.block(frame.block)?;
        let line = block.line(frame.instruction);

        self.location = line;

        let arrived = line.map(|location| (location.file, location.line));

        if self.debug
            && !self.stepped
            && resume.is_none()
            && arrived.is_some()
            && arrived != self.broke_at
        {
            self.broke_at = arrived;
            return self.yield_(YieldReason::Break);
        }

        self.stepped = false;

        Ok(None)
    }

    fn resume_inner(&mut self, mut resume: Option<Resume>) -> Result<Option<YieldReason>, Error> {
        let frame = self.frame()?;
        let function = self.module.function(frame.function)?;
        let block = function.block(frame.block)?;
        let position = frame.instruction;
        let instruction = block
            .instructions
            .get(position)
            .cloned()
            .ok_or(Error::EndOfBlock)?;

        match &instruction {
            bitcode::Instruction::Nop => self.advance()?,

            bitcode::Instruction::Undef { result, result_type } => {
                let value = value::zeroed(&self.module, *result_type)?;

                self.bind(*result, value)?;
                self.advance()?;
            }

            bitcode::Instruction::Variable { result, result_type, initializer } => {
                let pointee_type = self.module.pointee_type(*result_type)?;
                let layout = self.module.layout(pointee_type)?;
                let (size, alignment) = (layout.size, layout.alignment);

                let address = self
                    .storage
                    .allocate(size, alignment)
                    .ok_or(Error::OutOfSlots)?;

                let pointer = value::Pointer { address, pointee_type };
                let diagnostics = match initializer {
                    Some(initializer) => {
                        let value = self.value(*initializer)?;

                        self.write_internal(pointer, size, &value)?
                    }
                    None => detectors::DiagnosticList::new(),
                };

                self.bind(*result, value::Value::Pointer(pointer))?;
                self.advance()?;

                if !diagnostics.is_empty() {
                    return self.yield_(YieldReason::Diagnostic(diagnostics));
                }
            }

            bitcode::Instruction::Load { result, pointer, alignment, .. } => {
                let target = self.value(*pointer)?.as_pointer()?;
                let size = self.module.layout(target.pointee_type)?.size;

                match resume.take() {
                    Some(Resume::Bytes(bytes)) => {
                        let value = value::decode(&self.module, target.pointee_type, &bytes)?;

                        self.bind(*result, value)?;
                        self.advance()?;

                        let mut diagnostics = detectors::DiagnosticList::new();
                        diagnostics.extend(self.alignment(target, size, *alignment, false)?);

                        if !diagnostics.is_empty() {
                            return self.yield_(YieldReason::Diagnostic(diagnostics));
                        }
                    }
                    Some(Resume::Builtin(raw)) => {
                        let value = value::from_raw(&self.module, target.pointee_type, raw)?;

                        self.bind(*result, value)?;
                        self.advance()?;
                    }
                    Some(_) => return Err(Error::UnexpectedResume),
                    None => {
                        let address = target.address;

                        match address::region(address) {
                            Some(address::Region::Builtin) => {
                                let builtin = builtin_from_addr(address)?;
                                return self.yield_(YieldReason::Builtin(builtin));
                            }
                            Some(address::Region::Global) | None => {
                                return self.yield_(YieldReason::Read { address, size });
                            }
                            Some(address::Region::Local) => {
                                return self.yield_(YieldReason::ReadLocal { address, size });
                            }
                            Some(address::Region::Mutable)
                            | Some(address::Region::Constant)
                            | Some(address::Region::Invocation) => {
                                let (value, mut diagnostics) = self.read_internal(target, size)?;

                                self.bind(*result, value)?;
                                self.advance()?;

                                diagnostics
                                    .extend(self.alignment(target, size, *alignment, false)?);

                                if !diagnostics.is_empty() {
                                    return self.yield_(YieldReason::Diagnostic(diagnostics));
                                }
                            }
                        }
                    }
                }
            }

            bitcode::Instruction::Store { pointer, object, alignment } => {
                let target = self.value(*pointer)?.as_pointer()?;
                let size = self.module.layout(target.pointee_type)?.size;

                match resume.take() {
                    Some(Resume::Ack) => {
                        self.advance()?;

                        let mut diagnostics = detectors::DiagnosticList::new();
                        diagnostics.extend(self.alignment(target, size, *alignment, true)?);

                        if !diagnostics.is_empty() {
                            return self.yield_(YieldReason::Diagnostic(diagnostics));
                        }
                    }
                    Some(_) => return Err(Error::UnexpectedResume),
                    None => {
                        let value = self.value(*object)?;

                        match address::region(target.address) {
                            Some(address::Region::Constant) | Some(address::Region::Builtin) => {
                                return Err(Error::ReadOnlyRegion);
                            }
                            Some(address::Region::Global) | Some(address::Region::Local) | None => {
                                let mut bytes = vec![0u8; size];

                                value::encode(
                                    &self.module,
                                    target.pointee_type,
                                    &value,
                                    &mut bytes,
                                )?;

                                let address = target.address;
                                let reason = match address::region(address) {
                                    Some(address::Region::Local) => {
                                        YieldReason::WriteLocal { address, bytes }
                                    }
                                    _ => YieldReason::Write { address, bytes },
                                };

                                return self.yield_(reason);
                            }
                            Some(address::Region::Mutable) | Some(address::Region::Invocation) => {
                                let mut diagnostics = self.write_internal(target, size, &value)?;

                                self.advance()?;

                                diagnostics.extend(self.alignment(target, size, *alignment, true)?);

                                if !diagnostics.is_empty() {
                                    return self.yield_(YieldReason::Diagnostic(diagnostics));
                                }
                            }
                        }
                    }
                }
            }

            bitcode::Instruction::AccessChain { result, result_type, base, indices, element } => {
                let mut pointer = self.value(*base)?.as_pointer()?;

                let walked = if *element {
                    let index = self.value(indices[0])?.as_bits()?;
                    let stride = self.module.layout(pointer.pointee_type)?.size as u64;
                    pointer.address = pointer.address.wrapping_add(index.wrapping_mul(stride));
                    1
                } else {
                    0
                };

                let mut diagnostics = detectors::DiagnosticList::new();

                for index_id in &indices[walked..] {
                    let index = self.value(*index_id)?.as_bits()? as usize;
                    diagnostics.extend(self.element_index(pointer.pointee_type, index as u64)?);

                    let offset = self.module.member_offset(pointer.pointee_type, index)?;
                    pointer.pointee_type = self.module.member_type(pointer.pointee_type, index)?;
                    pointer.address = pointer.address.wrapping_add(offset as u64);
                }

                pointer.pointee_type = self.module.pointee_type(*result_type)?;

                self.bind(*result, value::Value::Pointer(pointer))?;
                self.advance()?;

                if !diagnostics.is_empty() {
                    return self.yield_(YieldReason::Diagnostic(diagnostics));
                }
            }

            bitcode::Instruction::Unary { result, result_type, operation, operand } => {
                let operand = self.value(*operand)?;
                let result_value =
                    value::unary_op(&self.module, *result_type, *operation, &operand)?;

                self.bind(*result, result_value)?;
                self.advance()?;
            }

            bitcode::Instruction::Binary { result, result_type, operation, lhs, rhs } => {
                let left = self.value(*lhs)?;
                let right = self.value(*rhs)?;
                let result_value =
                    value::binary_op(&self.module, *result_type, *operation, &left, &right)?;

                self.bind(*result, result_value)?;
                self.advance()?;
            }

            bitcode::Instruction::Convert { result, result_type, operation, operand } => {
                let operand = self.value(*operand)?;
                let decorations = self.module.decorations(*result)?;
                let rounding = decorations.rounding_mode;
                let saturated = decorations.saturated_conversion;
                let result_value = value::convert_op(
                    &self.module,
                    *result_type,
                    *operation,
                    &operand,
                    rounding,
                    saturated,
                )?;

                self.bind(*result, result_value)?;
                self.advance()?;
            }

            bitcode::Instruction::Select { result, condition, true_value, false_value, .. } => {
                let taken = self.value(*condition)?.as_bool()?;
                let chosen = if taken {
                    self.value(*true_value)?
                } else {
                    self.value(*false_value)?
                };

                self.bind(*result, chosen)?;
                self.advance()?;
            }

            bitcode::Instruction::CopyObject { result, operand, .. } => {
                let operand = self.value(*operand)?;

                self.bind(*result, operand)?;
                self.advance()?;
            }

            bitcode::Instruction::CompositeConstruct { result, members, .. } => {
                let values = members
                    .iter()
                    .map(|member| self.value(*member))
                    .collect::<Result<Vec<_>, Error>>()?;

                self.bind(*result, value::Value::Composite(values))?;
                self.advance()?;
            }

            bitcode::Instruction::CompositeExtract { result, composite, indices, .. } => {
                let composite = self.value(*composite)?;
                let extracted =
                    indices
                        .iter()
                        .try_fold(composite, |current, index| match current {
                            value::Value::Composite(members) => members
                                .get(*index as usize)
                                .cloned()
                                .ok_or(Error::OutOfBounds),
                            _ => Err(value::Error::NotAComposite.into()),
                        })?;

                self.bind(*result, extracted)?;
                self.advance()?;
            }

            bitcode::Instruction::CompositeInsert {
                result, object, composite, indices, ..
            } => {
                let object = self.value(*object)?;
                let composite = self.value(*composite)?;
                let updated = insert(composite, indices, object)?;

                self.bind(*result, updated)?;
                self.advance()?;
            }

            bitcode::Instruction::VectorShuffle {
                result,
                result_type,
                first,
                second,
                components,
            } => {
                let component_type = self.module.component_type(*result_type)?;
                let mut lanes = match self.value(*first)? {
                    value::Value::Composite(members) => members,
                    scalar => vec![scalar],
                };

                match self.value(*second)? {
                    value::Value::Composite(members) => lanes.extend(members),
                    scalar => lanes.push(scalar),
                }

                let selected = components
                    .iter()
                    .map(|component| match lanes.get(*component as usize) {
                        Some(lane) => Ok(lane.clone()),
                        None if *component == u32::MAX => {
                            Ok(value::zeroed(&self.module, component_type)?)
                        }
                        None => Err(Error::OutOfBounds),
                    })
                    .collect::<Result<Vec<_>, Error>>()?;

                self.bind(*result, value::Value::Composite(selected))?;
                self.advance()?;
            }

            bitcode::Instruction::Dot { result, result_type, lhs, rhs } => {
                let left = self.value(*lhs)?;
                let right = self.value(*rhs)?;
                let product = value::dot(&self.module, *result_type, &left, &right)?;

                self.bind(*result, product)?;
                self.advance()?;
            }

            bitcode::Instruction::ExtInst { result, result_type, set, instruction, operands } => {
                if Some(*set) != self.module.opencl_std() {
                    return Err(bitcode::Error::UnsupportedExtInstSet(*set).into());
                }

                let operands = operands
                    .iter()
                    .map(|operand| self.value(*operand))
                    .collect::<Result<Vec<_>, Error>>()?;
                let computed =
                    value::ext_inst(&self.module, *result_type, *instruction, &operands)?;

                self.bind(*result, computed)?;
                self.advance()?;
            }

            bitcode::Instruction::VectorExtractDynamic { result, vector, index, .. } => {
                let source = self.value(*vector)?;
                let position = self.value(*index)?.as_bits()? as usize;
                let extracted = match source {
                    value::Value::Composite(members) => {
                        members.get(position).cloned().ok_or(Error::OutOfBounds)?
                    }
                    scalar => scalar,
                };

                self.bind(*result, extracted)?;
                self.advance()?;
            }

            bitcode::Instruction::VectorInsertDynamic {
                result, vector, component, index, ..
            } => {
                let source = self.value(*vector)?;
                let inserted = self.value(*component)?;
                let position = self.value(*index)?.as_bits()? as usize;
                let updated = match source {
                    value::Value::Composite(mut members) => {
                        if position >= members.len() {
                            return Err(Error::OutOfBounds);
                        }
                        members[position] = inserted;
                        value::Value::Composite(members)
                    }
                    _ => return Err(value::Error::NotAComposite.into()),
                };

                self.bind(*result, updated)?;
                self.advance()?;
            }

            bitcode::Instruction::VectorTimesScalar { result, result_type, vector, scalar } => {
                let source = self.value(*vector)?;
                let factor = self.value(*scalar)?;
                let component_type = self.module.component_type(*result_type)?;
                let members = match source {
                    value::Value::Composite(members) => members,
                    scalar => vec![scalar],
                };
                let scaled = members
                    .iter()
                    .map(|member| {
                        value::binary_op_scalar(
                            &self.module,
                            component_type,
                            bitcode::BinaryOp::FMul,
                            member,
                            &factor,
                        )
                    })
                    .collect::<Result<Vec<_>, value::Error>>()?;

                self.bind(*result, value::Value::Composite(scaled))?;
                self.advance()?;
            }

            bitcode::Instruction::Atomic {
                result,
                operation,
                pointer,
                value: operand,
                comparator,
            } => {
                let target = self.value(*pointer)?.as_pointer()?;
                let width = self.module.scalar_width(target.pointee_type)?;
                match resume.take() {
                    Some(Resume::Scalar(previous)) => {
                        if let Some(result) = result {
                            self.bind(*result, value::Value::from_bits(previous, width))?;
                        }

                        self.advance()?;

                        let mut diagnostics = detectors::DiagnosticList::new();
                        diagnostics.extend(self.atomic_width(target, width));

                        if !diagnostics.is_empty() {
                            return self.yield_(YieldReason::Diagnostic(diagnostics));
                        }
                    }
                    Some(_) => return Err(Error::UnexpectedResume),
                    None => {
                        let operand = self.optional_bits(*operand)?;
                        let comparator = self.optional_bits(*comparator)?;

                        return self.yield_(YieldReason::Atomic {
                            operation: *operation,
                            address: target.address,
                            width,
                            local: address::region(target.address) == Some(address::Region::Local),
                            value: operand,
                            comparator,
                        });
                    }
                }
            }

            bitcode::Instruction::Phi { result, pairs, .. } => {
                let previous = self.frame()?.previous_block.ok_or(Error::NoPredecessor)?;
                let source = pairs
                    .iter()
                    .find(|(_, label)| *label == previous)
                    .map(|(value, _)| *value)
                    .ok_or(Error::NoPredecessor)?;
                let incoming = self.value(source)?;

                self.bind(*result, incoming)?;
                self.advance()?;
            }

            bitcode::Instruction::FunctionCall { result, function, arguments, .. } => {
                let callee = self.module.function(*function)?;

                if callee.parameters.len() != arguments.len() {
                    return Err(Error::ArgumentMismatch);
                }

                let mut values = vec![None; self.module.bound()];
                for (parameter, argument) in callee.parameters.iter().zip(arguments) {
                    values[parameter.result as usize] = Some(self.value(*argument)?);
                }

                let block = self.module.function(*function)?.entry_label()?;
                let frame = Frame::new(
                    *function,
                    block,
                    values,
                    Some(*result),
                    self.storage.watermark(),
                );

                self.advance()?;
                self.frames.push(frame);
            }

            bitcode::Instruction::Branch { target } => self.jump(*target)?,

            bitcode::Instruction::BranchConditional { condition, true_target, false_target } => {
                let taken = self.value(*condition)?.as_bool()?;
                let target = if taken { *true_target } else { *false_target };

                self.jump(target)?;
            }

            bitcode::Instruction::Switch { selector, default_target, cases } => {
                let selected = self.value(*selector)?.as_bits()?;
                let target = cases
                    .iter()
                    .find(|(literal, _)| *literal == selected)
                    .map(|(_, label)| *label)
                    .unwrap_or(*default_target);

                self.jump(target)?;
            }

            bitcode::Instruction::Barrier { execution_scope, memory_semantics } => {
                match resume.take() {
                    Some(Resume::Ack) => self.advance()?,
                    Some(_) => return Err(Error::UnexpectedResume),
                    None => {
                        let scope = self.value(*execution_scope)?.as_bits()?;

                        return self.yield_(YieldReason::ControlBarrier {
                            execution_scope: scope,
                            semantics: *memory_semantics,
                        });
                    }
                }
            }

            bitcode::Instruction::MemoryBarrier { memory_semantics } => match resume.take() {
                Some(Resume::Ack) => self.advance()?,
                Some(_) => return Err(Error::UnexpectedResume),
                None => {
                    return self
                        .yield_(YieldReason::MemoryBarrier { semantics: *memory_semantics });
                }
            },

            bitcode::Instruction::Return => self.pop_frame(value::Value::Void)?,

            bitcode::Instruction::ReturnValue { value } => {
                let returned = self.value(*value)?;

                self.pop_frame(returned)?;
            }

            bitcode::Instruction::Unreachable => return Err(Error::Unreachable),
        }

        Ok(None)
    }

    fn yield_(&mut self, reason: YieldReason) -> Result<Option<YieldReason>, Error> {
        self.pending = Some(reason.clone());
        Ok(Some(reason))
    }

    fn alignment(
        &self,
        target: value::Pointer,
        size: usize,
        declared: Option<u32>,
        store: bool,
    ) -> Result<Option<detectors::Diagnostic>, Error> {
        Ok(detectors::alignment(
            self.checks,
            detectors::Access::new(target.address, size, store, false, self.location),
            declared,
            self.module.layout(target.pointee_type)?.alignment,
        ))
    }

    fn element_index(
        &self,
        aggregate: bitcode::Id,
        index: u64,
    ) -> Result<Option<detectors::Diagnostic>, Error> {
        Ok(detectors::element_index(
            self.checks,
            index,
            self.module.element_count(aggregate)?,
        ))
    }

    fn atomic_width(&self, target: value::Pointer, width: u32) -> Option<detectors::Diagnostic> {
        detectors::atomic_width(
            self.checks,
            detectors::Access::new(
                target.address,
                (width / 8) as usize,
                true,
                true,
                self.location,
            ),
            width,
        )
    }

    fn frame(&self) -> Result<&Frame, Error> {
        self.frames.last().ok_or(Error::NoFrame)
    }

    fn frame_mut(&mut self) -> Result<&mut Frame, Error> {
        self.frames.last_mut().ok_or(Error::NoFrame)
    }

    fn advance(&mut self) -> Result<(), Error> {
        self.frame_mut()?.instruction += 1;

        Ok(())
    }

    fn jump(&mut self, label: bitcode::Id) -> Result<(), Error> {
        let frame = self.frame()?;

        self.module.function(frame.function)?.block(label)?;

        let frame = self.frame_mut()?;
        frame.previous_block = Some(frame.block);
        frame.block = label;
        frame.instruction = 0;

        Ok(())
    }

    fn pop_frame(&mut self, returned: value::Value) -> Result<(), Error> {
        let finished = self.frames.pop().ok_or(Error::NoFrame)?;

        self.storage.truncate(finished.storage_watermark);

        if let Some(slot) = finished.return_slot
            && let Some(parent) = self.frames.last_mut()
        {
            parent.values[slot as usize] = Some(returned);
        }

        Ok(())
    }

    fn value(&self, id: bitcode::Id) -> Result<value::Value, Error> {
        if let Some(found) = self
            .frame()?
            .values
            .get(id as usize)
            .and_then(|slot| slot.clone())
        {
            return Ok(found);
        }

        match value::constant(&self.module, id) {
            Ok(value) => return Ok(value),
            Err(value::Error::Bitcode(bitcode::Error::NotAConstant(_))) => (),
            Err(error) => return Err(error.into()),
        }

        if let Some(variable) = self.module.variable(id)? {
            let pointee_type = self.module.pointee_type(variable.result_type)?;

            if let Some(builtin) = self.module.decorations(id)?.builtin {
                return Ok(value::Value::Pointer(value::Pointer {
                    address: builtin_to_addr(builtin),
                    pointee_type,
                }));
            }

            let address = match variable.storage {
                bitcode::StorageClass::UniformConstant | bitcode::StorageClass::Private => {
                    address_of(&self.private_addresses, id)?
                }
                bitcode::StorageClass::Workgroup => address_of(&self.local_addresses, id)?,
                storage => return Err(value::Error::UnsupportedStorageClass(storage).into()),
            };

            return Ok(value::Value::Pointer(value::Pointer {
                address,
                pointee_type,
            }));
        }

        Err(Error::UnboundValue(id))
    }

    fn optional_bits(&self, id: Option<bitcode::Id>) -> Result<u64, Error> {
        match id {
            Some(id) => Ok(self.value(id)?.as_bits()?),
            None => Ok(0),
        }
    }

    fn bind(&mut self, id: bitcode::Id, value: value::Value) -> Result<(), Error> {
        self.frame_mut()?.values[id as usize] = Some(value);

        Ok(())
    }

    fn read_internal(
        &mut self,
        pointer: value::Pointer,
        size: usize,
    ) -> Result<(value::Value, detectors::DiagnosticList), Error> {
        let Interpreter {
            module,
            storage,
            mutable_storage,
            constant_storage,
            checks,
            location,
            ..
        } = self;

        let source = match address::region(pointer.address) {
            Some(address::Region::Invocation) => storage,
            Some(address::Region::Mutable) => mutable_storage,
            Some(address::Region::Constant) => constant_storage,
            _ => return Err(Error::UnsupportedRegion),
        };

        let (value, diagnostics) = detectors::checked_read(
            source,
            *checks,
            detectors::Access::new(pointer.address, size, false, false, *location),
            |bytes| -> Result<value::Value, Error> {
                Ok(value::decode(module, pointer.pointee_type, bytes)?)
            },
            || -> Result<value::Value, Error> { Ok(value::zeroed(module, pointer.pointee_type)?) },
        )?;

        Ok((value, diagnostics))
    }

    fn write_internal(
        &mut self,
        pointer: value::Pointer,
        size: usize,
        value: &value::Value,
    ) -> Result<detectors::DiagnosticList, Error> {
        let Interpreter { module, storage, mutable_storage, checks, location, .. } = self;

        let target = match address::region(pointer.address) {
            Some(address::Region::Invocation) => storage,
            Some(address::Region::Mutable) => mutable_storage,
            Some(address::Region::Constant) | Some(address::Region::Builtin) => {
                return Err(Error::ReadOnlyRegion);
            }
            _ => return Err(Error::UnsupportedRegion),
        };

        let (_, diagnostics) = detectors::checked_write(
            target,
            *checks,
            detectors::Access::new(pointer.address, size, true, false, *location),
            |destination| -> Result<(), Error> {
                Ok(value::encode(
                    module,
                    pointer.pointee_type,
                    value,
                    destination,
                )?)
            },
            || Ok(()),
        )?;

        Ok(diagnostics)
    }
}
