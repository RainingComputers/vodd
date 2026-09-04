use crate::bitcode;
use crate::value;

pub use crate::bitcode::AtomicOp as Atomic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Bitcode(bitcode::Error),
    Value(value::Error),
    UnboundValue(bitcode::Id),
    ReadOnlyRegion,
    OutOfBounds,
    ArgumentMismatch,
    UnsupportedRegion,
    Unreachable,
    EndOfBlock,
    NoFrame,
    NoPredecessor,
    OutOfFuel,
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
        semantics: u64,
    },
    ControlBarrier {
        execution_scope: u64,
        semantics: u64,
    },
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
                    | YieldReason::ControlBarrier { .. },
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
    module_storage: Vec<u8>,
    module_offsets: Vec<Option<usize>>,
    local_offsets: Vec<Option<usize>>,
    frames: Vec<Frame>,
    storage: Vec<u8>,
    fuel: usize,
    pending: Option<YieldReason>,
    location: Option<bitcode::Location>,
}

pub fn local_memory_size(module: &bitcode::Module) -> Result<usize, Error> {
    Ok(local_layout(module)?.1)
}

fn local_layout(module: &bitcode::Module) -> Result<(Vec<Option<usize>>, usize), Error> {
    let mut offsets = vec![None; module.bound()];
    let mut size = 0;

    let stored = module
        .variables()
        .iter()
        .filter(|variable| variable.storage == bitcode::StorageClass::Workgroup);

    for variable in stored {
        let pointee_type = module.pointee_type(variable.result_type)?;
        let layout = module.layout(pointee_type)?;
        let offset = bitcode::align_up(size, layout.alignment);

        offsets[variable.result as usize] = Some(offset);
        size = offset + layout.size;
    }

    Ok((offsets, size))
}

fn offset_of(offsets: &[Option<usize>], id: bitcode::Id) -> Result<u64, Error> {
    offsets
        .get(id as usize)
        .copied()
        .flatten()
        .map(|offset| offset as u64)
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
        fuel: usize,
    ) -> Result<Interpreter, Error> {
        let mut module_storage: Vec<u8> = Vec::new();
        let mut module_offsets = vec![None; module.bound()];

        let stored = module.variables().iter().filter(|variable| {
            matches!(
                variable.storage,
                bitcode::StorageClass::UniformConstant | bitcode::StorageClass::Private
            )
        });

        for variable in stored {
            let pointee_type = module.pointee_type(variable.result_type)?;
            let size = module.layout(pointee_type)?.size;
            let offset = module_storage.len();

            module_storage.resize(offset + size, 0);
            module_offsets[variable.result as usize] = Some(offset);

            if let Some(initializer) = variable.initializer {
                let value = value::constant(&module, initializer)?;
                value::encode(&module, pointee_type, &value, &mut module_storage[offset..])?;
            }
        }

        let entry = module.function(function)?;
        if entry.parameters.len() != arguments.len() {
            return Err(Error::ArgumentMismatch);
        }

        let mut values = vec![None; module.bound()];

        for (parameter, argument) in entry.parameters.iter().zip(arguments) {
            values[parameter.result as usize] = Some(match argument {
                Argument::Buffer(address) => {
                    let storage = module.storage_class(parameter.result_type)?;

                    value::Value::Pointer(value::Pointer {
                        region: value::Region::from_storage_class(storage)?,
                        address: *address,
                        pointee_type: module.pointee_type(parameter.result_type)?,
                    })
                }
                Argument::Value(bytes) => value::decode(&module, parameter.result_type, bytes)?,
            });
        }

        let frame = Frame::new(function, entry.entry_label()?, values, None, 0);
        let (local_offsets, _) = local_layout(&module)?;

        Ok(Interpreter {
            module,
            module_storage,
            module_offsets,
            local_offsets,
            frames: vec![frame],
            storage: Vec::new(),
            fuel,
            pending: None,
            location: None,
        })
    }

    pub fn location(&self) -> Option<bitcode::Location> {
        self.location
    }

    pub fn resume(&mut self, resume: Resume) -> Result<Option<YieldReason>, Error> {
        let mut resume = match (self.pending.take(), resume) {
            (None, Resume::Start) => None,
            (Some(reason), resume) if reason.accepts(&resume) => Some(resume),
            _ => return Err(Error::UnexpectedResume),
        };

        while !self.frames.is_empty() {
            if self.fuel == 0 {
                return Err(Error::OutOfFuel);
            }
            self.fuel -= 1;

            if let Some(reason) = self.resume_inner(resume.take())? {
                return Ok(Some(reason));
            }
        }

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

        self.location = block.line(position);

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
                let address = bitcode::align_up(self.storage.len(), alignment);
                let pointer = value::Pointer {
                    region: value::Region::Invocation,
                    address: address as u64,
                    pointee_type,
                };

                self.storage.resize(address + size, 0);

                if let Some(initializer) = initializer {
                    let value = self.value(*initializer)?;

                    self.write_internal(pointer, size, &value)?;
                }

                self.bind(*result, value::Value::Pointer(pointer))?;
                self.advance()?;
            }

            bitcode::Instruction::Load { result, pointer, .. } => {
                let target = self.value(*pointer)?.as_pointer()?;

                match resume.take() {
                    Some(Resume::Bytes(bytes)) => {
                        let value = value::decode(&self.module, target.pointee_type, &bytes)?;

                        self.bind(*result, value)?;
                        self.advance()?;
                    }
                    Some(Resume::Builtin(raw)) => {
                        let value = value::from_raw(&self.module, target.pointee_type, raw)?;

                        self.bind(*result, value)?;
                        self.advance()?;
                    }
                    Some(_) => return Err(Error::UnexpectedResume),
                    None => match target.region {
                        value::Region::Builtin(builtin) => {
                            return self.yield_(YieldReason::Builtin(builtin));
                        }
                        value::Region::Global => {
                            let size = self.module.layout(target.pointee_type)?.size;

                            return self
                                .yield_(YieldReason::Read { address: target.address, size });
                        }
                        value::Region::Local => {
                            let size = self.module.layout(target.pointee_type)?.size;

                            return self
                                .yield_(YieldReason::ReadLocal { address: target.address, size });
                        }
                        _ => {
                            let size = self.module.layout(target.pointee_type)?.size;
                            let value = self.read_internal(target, size)?;

                            self.bind(*result, value)?;
                            self.advance()?;
                        }
                    },
                }
            }

            bitcode::Instruction::Store { pointer, object } => match resume.take() {
                Some(Resume::Ack) => self.advance()?,
                Some(_) => return Err(Error::UnexpectedResume),
                None => {
                    let target = self.value(*pointer)?.as_pointer()?;
                    let value = self.value(*object)?;
                    let size = self.module.layout(target.pointee_type)?.size;

                    match target.region {
                        value::Region::Global | value::Region::Local => {
                            let mut bytes = vec![0u8; size];

                            value::encode(&self.module, target.pointee_type, &value, &mut bytes)?;

                            let address = target.address;
                            let reason = if target.region == value::Region::Global {
                                YieldReason::Write { address, bytes }
                            } else {
                                YieldReason::WriteLocal { address, bytes }
                            };

                            return self.yield_(reason);
                        }
                        _ => {
                            self.write_internal(target, size, &value)?;
                            self.advance()?;
                        }
                    }
                }
            },

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

                for index_id in &indices[walked..] {
                    let index = self.value(*index_id)?.as_bits()? as usize;
                    let offset = self.module.member_offset(pointer.pointee_type, index)?;
                    pointer.pointee_type = self.module.member_type(pointer.pointee_type, index)?;
                    pointer.address = pointer.address.wrapping_add(offset as u64);
                }

                pointer.pointee_type = self.module.pointee_type(*result_type)?;

                self.bind(*result, value::Value::Pointer(pointer))?;
                self.advance()?;
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
                    }
                    Some(_) => return Err(Error::UnexpectedResume),
                    None => {
                        let operand = self.optional_bits(*operand)?;
                        let comparator = self.optional_bits(*comparator)?;

                        return self.yield_(YieldReason::Atomic {
                            operation: *operation,
                            address: target.address,
                            width,
                            local: target.region == value::Region::Local,
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
                let frame = Frame::new(*function, block, values, Some(*result), self.storage.len());

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
                        let semantics = self.value(*memory_semantics)?.as_bits()?;

                        return self.yield_(YieldReason::ControlBarrier {
                            execution_scope: scope,
                            semantics,
                        });
                    }
                }
            }

            bitcode::Instruction::MemoryBarrier { memory_semantics } => match resume.take() {
                Some(Resume::Ack) => self.advance()?,
                Some(_) => return Err(Error::UnexpectedResume),
                None => {
                    let semantics = self.value(*memory_semantics)?.as_bits()?;

                    return self.yield_(YieldReason::MemoryBarrier { semantics });
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
                    region: value::Region::Builtin(builtin),
                    address: 0,
                    pointee_type,
                }));
            }

            let region = match variable.storage {
                bitcode::StorageClass::UniformConstant => value::Region::Module,
                storage => value::Region::from_storage_class(storage)?,
            };
            let address = match region {
                value::Region::Module => offset_of(&self.module_offsets, id)?,
                value::Region::Local => offset_of(&self.local_offsets, id)?,
                _ => 0,
            };

            return Ok(value::Value::Pointer(value::Pointer {
                region,
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

    fn read_internal(&self, pointer: value::Pointer, size: usize) -> Result<value::Value, Error> {
        let start = pointer.address as usize;
        let source = match pointer.region {
            value::Region::Invocation => &self.storage,
            value::Region::Module => &self.module_storage,
            _ => return Err(Error::UnsupportedRegion),
        };

        if start + size > source.len() {
            return Err(Error::OutOfBounds);
        }

        Ok(value::decode(
            &self.module,
            pointer.pointee_type,
            &source[start..start + size],
        )?)
    }

    fn write_internal(
        &mut self,
        pointer: value::Pointer,
        size: usize,
        value: &value::Value,
    ) -> Result<(), Error> {
        let start = pointer.address as usize;
        let destination = match pointer.region {
            value::Region::Invocation => &mut self.storage,
            value::Region::Module | value::Region::Builtin(_) => {
                return Err(Error::ReadOnlyRegion);
            }
            _ => return Err(Error::UnsupportedRegion),
        };

        if start + size > destination.len() {
            return Err(Error::OutOfBounds);
        }

        value::encode(
            &self.module,
            pointer.pointee_type,
            value,
            &mut destination[start..start + size],
        )?;

        Ok(())
    }
}
