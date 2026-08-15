use crate::spirv;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Spirv(spirv::Error),
    UnboundValue(spirv::Id),
    NotAPointer(spirv::Id),
    ReadOnlyRegion,
    OutOfBounds,
    ArgumentMismatch,
    UnsupportedInstruction,
    NoPredecessor,
    BudgetExhausted,
    Host(HostError),
}

impl From<spirv::Error> for Error {
    fn from(error: spirv::Error) -> Error {
        Error::Spirv(error)
    }
}

impl From<HostError> for Error {
    fn from(error: HostError) -> Error {
        Error::Host(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    OutOfBounds,
    BadAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Atomic {
    Increment,
    Decrement,
}

pub trait Host {
    fn read(&mut self, address: u64, destination: &mut [u8]) -> Result<(), HostError>;

    fn write(&mut self, address: u64, source: &[u8]) -> Result<(), HostError>;

    fn read_local(&mut self, address: u64, destination: &mut [u8]) -> Result<(), HostError>;

    fn write_local(&mut self, address: u64, source: &[u8]) -> Result<(), HostError>;

    fn atomic(&mut self, operation: Atomic, address: u64, width: u32) -> Result<u64, HostError>;

    fn memory_barrier(&mut self, semantics: u32);

    fn builtin(&mut self, builtin: spirv::Builtin) -> [u64; 3];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Global,
    Local,
    Invocation,
    Module,
    Builtin(spirv::Builtin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pointer {
    pub region: Region,
    pub address: u64,
    pub pointee: spirv::Id,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Void,
    Bool(bool),
    Scalar { bits: u64, width: u32 },
    Composite(Vec<Value>),
    Pointer(Pointer),
}

impl Value {
    fn scalar_bits(&self) -> Result<u64, Error> {
        match self {
            Value::Scalar { bits, .. } => Ok(*bits),
            Value::Bool(flag) => Ok(u64::from(*flag)),
            _ => Err(Error::UnsupportedInstruction),
        }
    }

    fn as_bool(&self) -> Result<bool, Error> {
        match self {
            Value::Bool(flag) => Ok(*flag),
            Value::Scalar { bits, .. } => Ok(*bits != 0),
            _ => Err(Error::UnsupportedInstruction),
        }
    }

    fn as_pointer(&self) -> Result<Pointer, Error> {
        match self {
            Value::Pointer(pointer) => Ok(*pointer),
            _ => Err(Error::UnsupportedInstruction),
        }
    }
}

fn mask(bits: u64, width: u32) -> u64 {
    if width >= 64 {
        bits
    } else {
        bits & ((1u64 << width) - 1)
    }
}

fn sign_extend(bits: u64, width: u32) -> i64 {
    if width >= 64 {
        bits as i64
    } else {
        let shift = 64 - width;
        ((bits << shift) as i64) >> shift
    }
}

#[derive(Debug, Clone)]
pub enum Argument {
    Buffer(u64),
    Value(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Program {
    pub module: spirv::Module,
    module_storage: Vec<u8>,
    module_offsets: Vec<Option<usize>>,
}

impl Program {
    pub fn new(module: spirv::Module) -> Result<Program, Error> {
        let bound = module.bound;
        let mut program = Program {
            module,
            module_storage: Vec::new(),
            module_offsets: vec![None; bound],
        };
        let variables = program.module.variables.clone();
        for variable in variables {
            if variable.storage != spirv::StorageClass::UniformConstant
                && variable.storage != spirv::StorageClass::Private
            {
                continue;
            }
            let pointee = program.module.pointee_of(variable.result_type)?;
            let layout = program.module.layout_of(pointee)?.clone();
            let offset = program.module_storage.len();
            program.module_storage.resize(offset + layout.size, 0);
            if let Some(initializer) = variable.initializer {
                let value = program.constant_value(initializer)?;
                let mut bytes = vec![0u8; layout.size];
                program.encode(&value, pointee, &mut bytes)?;
                program.module_storage[offset..offset + layout.size].copy_from_slice(&bytes);
            }
            program.module_offsets[variable.result as usize] = Some(offset);
        }
        Ok(program)
    }

    pub fn constant_value(&self, id: spirv::Id) -> Result<Value, Error> {
        let constant = match self.module.constant_of(id)? {
            Some(found) => found,
            None => return Err(Error::UnboundValue(id)),
        };
        match &constant.kind {
            spirv::ConstantKind::True => Ok(Value::Bool(true)),
            spirv::ConstantKind::False => Ok(Value::Bool(false)),
            spirv::ConstantKind::Scalar { bits } => {
                let width = self.module.scalar_width(constant.result_type)?;
                Ok(Value::Scalar {
                    bits: mask(*bits, width),
                    width,
                })
            }
            spirv::ConstantKind::Composite { constituents } => {
                let mut members = Vec::with_capacity(constituents.len());
                for member in constituents {
                    members.push(self.constant_value(*member)?);
                }
                Ok(Value::Composite(members))
            }
            spirv::ConstantKind::Null => self.zeroed(constant.result_type),
        }
    }

    pub fn zeroed(&self, type_id: spirv::Id) -> Result<Value, Error> {
        Ok(match self.module.type_of(type_id)? {
            spirv::Type::Void => Value::Void,
            spirv::Type::Bool => Value::Bool(false),
            spirv::Type::Int { width } | spirv::Type::Float { width } => Value::Scalar {
                bits: 0,
                width: *width,
            },
            spirv::Type::Pointer { storage, pointee } => Value::Pointer(Pointer {
                region: region_of(*storage),
                address: 0,
                pointee: *pointee,
            }),
            spirv::Type::Vector { component, count } => {
                let component = *component;
                let count = *count as usize;
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(self.zeroed(component)?);
                }
                Value::Composite(members)
            }
            spirv::Type::Array { element, count } => {
                let element = *element;
                let count = *count as usize;
                let mut members = Vec::with_capacity(count);
                for _ in 0..count {
                    members.push(self.zeroed(element)?);
                }
                Value::Composite(members)
            }
            spirv::Type::Struct { members } => {
                let members = members.clone();
                let mut values = Vec::with_capacity(members.len());
                for member in members {
                    values.push(self.zeroed(member)?);
                }
                Value::Composite(values)
            }
            spirv::Type::Function { .. } => Value::Void,
        })
    }

    pub fn encode(
        &self,
        value: &Value,
        type_id: spirv::Id,
        destination: &mut [u8],
    ) -> Result<(), Error> {
        match value {
            Value::Void => Ok(()),
            Value::Bool(flag) => {
                if destination.is_empty() {
                    return Err(Error::OutOfBounds);
                }
                destination[0] = u8::from(*flag);
                Ok(())
            }
            Value::Scalar { bits, width } => {
                let size = (*width as usize).div_ceil(8);
                if destination.len() < size {
                    return Err(Error::OutOfBounds);
                }
                destination[..size].copy_from_slice(&bits.to_le_bytes()[..size]);
                Ok(())
            }
            Value::Pointer(pointer) => {
                let size = self.module.layout_of(type_id)?.size;
                if destination.len() < size {
                    return Err(Error::OutOfBounds);
                }
                destination[..size].copy_from_slice(&pointer.address.to_le_bytes()[..size]);
                Ok(())
            }
            Value::Composite(members) => {
                for (index, member) in members.iter().enumerate() {
                    let member_type = self.module.member_type(type_id, index)?;
                    let offset = self.module.member_offset(type_id, index)?;
                    let size = self.module.layout_of(member_type)?.size;
                    if destination.len() < offset + size {
                        return Err(Error::OutOfBounds);
                    }
                    self.encode(member, member_type, &mut destination[offset..offset + size])?;
                }
                Ok(())
            }
        }
    }

    pub fn decode(&self, type_id: spirv::Id, source: &[u8]) -> Result<Value, Error> {
        Ok(match self.module.type_of(type_id)? {
            spirv::Type::Void => Value::Void,
            spirv::Type::Bool => Value::Bool(source.first().copied().unwrap_or(0) != 0),
            spirv::Type::Int { width } | spirv::Type::Float { width } => {
                let width = *width;
                let size = (width as usize).div_ceil(8);
                if source.len() < size {
                    return Err(Error::OutOfBounds);
                }
                let mut buffer = [0u8; 8];
                buffer[..size].copy_from_slice(&source[..size]);
                Value::Scalar {
                    bits: u64::from_le_bytes(buffer),
                    width,
                }
            }
            spirv::Type::Pointer { storage, pointee } => {
                let region = region_of(*storage);
                let pointee = *pointee;
                let size = self.module.layout_of(type_id)?.size;
                if source.len() < size {
                    return Err(Error::OutOfBounds);
                }
                let mut buffer = [0u8; 8];
                buffer[..size].copy_from_slice(&source[..size]);
                Value::Pointer(Pointer {
                    region,
                    address: u64::from_le_bytes(buffer),
                    pointee,
                })
            }
            spirv::Type::Vector { count, .. } => {
                let count = *count as usize;
                let mut members = Vec::with_capacity(count);
                for index in 0..count {
                    let member_type = self.module.member_type(type_id, index)?;
                    let offset = self.module.member_offset(type_id, index)?;
                    let size = self.module.layout_of(member_type)?.size;
                    members.push(self.decode(member_type, &source[offset..offset + size])?);
                }
                Value::Composite(members)
            }
            spirv::Type::Array { count, .. } => {
                let count = *count as usize;
                let mut members = Vec::with_capacity(count);
                for index in 0..count {
                    let member_type = self.module.member_type(type_id, index)?;
                    let offset = self.module.member_offset(type_id, index)?;
                    let size = self.module.layout_of(member_type)?.size;
                    members.push(self.decode(member_type, &source[offset..offset + size])?);
                }
                Value::Composite(members)
            }
            spirv::Type::Struct { members } => {
                let count = members.len();
                let mut values = Vec::with_capacity(count);
                for index in 0..count {
                    let member_type = self.module.member_type(type_id, index)?;
                    let offset = self.module.member_offset(type_id, index)?;
                    let size = self.module.layout_of(member_type)?.size;
                    values.push(self.decode(member_type, &source[offset..offset + size])?);
                }
                Value::Composite(values)
            }
            spirv::Type::Function { .. } => Value::Void,
        })
    }
}

fn region_of(storage: spirv::StorageClass) -> Region {
    match storage {
        spirv::StorageClass::CrossWorkgroup => Region::Global,
        spirv::StorageClass::Workgroup => Region::Local,
        spirv::StorageClass::UniformConstant | spirv::StorageClass::Private => Region::Module,
        _ => Region::Invocation,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Done,
    Yielded { execution_scope: u64, semantics: u64 },
}

#[derive(Debug, Clone)]
struct Frame {
    function: usize,
    block: usize,
    instruction: usize,
    previous_block: Option<spirv::Id>,
    values: Vec<Option<Value>>,
    return_slot: Option<spirv::Id>,
    storage_watermark: usize,
}

#[derive(Debug, Clone)]
pub struct Invocation {
    frames: Vec<Frame>,
    storage: Vec<u8>,
    budget: usize,
}

impl Invocation {
    pub fn new(
        program: &Program,
        function_index: usize,
        arguments: &[Argument],
    ) -> Result<Invocation, Error> {
        let function = &program.module.functions[function_index];
        if function.parameters.len() != arguments.len() {
            return Err(Error::ArgumentMismatch);
        }
        let mut values = vec![None; program.module.bound];
        for (parameter, argument) in function.parameters.iter().zip(arguments) {
            let value = match argument {
                Argument::Buffer(address) => {
                    let storage = program.module.storage_of(parameter.result_type)?;
                    let pointee = program.module.pointee_of(parameter.result_type)?;
                    Value::Pointer(Pointer {
                        region: region_of(storage),
                        address: *address,
                        pointee,
                    })
                }
                Argument::Value(bytes) => program.decode(parameter.result_type, bytes)?,
            };
            values[parameter.result as usize] = Some(value);
        }
        Ok(Invocation {
            frames: vec![Frame {
                function: function_index,
                block: 0,
                instruction: 0,
                previous_block: None,
                values,
                return_slot: None,
                storage_watermark: 0,
            }],
            storage: Vec::new(),
            budget: 1 << 24,
        })
    }

    pub fn run(&mut self, program: &Program, host: &mut dyn Host) -> Result<(), Error> {
        loop {
            match self.resume(program, host)? {
                Step::Done => return Ok(()),
                Step::Yielded { .. } => return Ok(()),
            }
        }
    }

    pub fn resume(&mut self, program: &Program, host: &mut dyn Host) -> Result<Step, Error> {
        while let Some(depth) = self.frames.len().checked_sub(1) {
            if self.budget == 0 {
                return Err(Error::BudgetExhausted);
            }
            self.budget -= 1;

            let frame = &self.frames[depth];
            let function = &program.module.functions[frame.function];
            let block = match function.blocks.get(frame.block) {
                Some(found) => found,
                None => return Err(Error::UnsupportedInstruction),
            };
            let instruction = match block.instructions.get(frame.instruction) {
                Some(found) => found.clone(),
                None => return Err(Error::UnsupportedInstruction),
            };

            match self.execute(program, host, depth, &instruction)? {
                Flow::Next => {
                    self.frames[depth].instruction += 1;
                }
                Flow::Jump(label) => {
                    let function = &program.module.functions[self.frames[depth].function];
                    let target = function.block_index(label)?;
                    let current = function.blocks[self.frames[depth].block].label;
                    let frame = &mut self.frames[depth];
                    frame.previous_block = Some(current);
                    frame.block = target;
                    frame.instruction = 0;
                }
                Flow::Called(frame) => {
                    self.frames[depth].instruction += 1;
                    self.frames.push(frame);
                }
                Flow::Returned(value) => {
                    let finished = self.frames.pop().ok_or(Error::UnsupportedInstruction)?;
                    self.storage.truncate(finished.storage_watermark);
                    if let Some(slot) = finished.return_slot
                        && let Some(parent) = self.frames.last_mut()
                    {
                        parent.values[slot as usize] = Some(value);
                    }
                }
                Flow::Yielded {
                    execution_scope,
                    semantics,
                } => {
                    self.frames[depth].instruction += 1;
                    return Ok(Step::Yielded {
                        execution_scope,
                        semantics,
                    });
                }
            }
        }
        Ok(Step::Done)
    }

    fn value(&self, program: &Program, depth: usize, id: spirv::Id) -> Result<Value, Error> {
        if let Some(found) = self.frames[depth].values.get(id as usize).and_then(|slot| slot.clone())
        {
            return Ok(found);
        }
        if program.module.constant_of(id)?.is_some() {
            return program.constant_value(id);
        }
        if let Some(variable) = program.module.variable_of(id)? {
            let pointee = program.module.pointee_of(variable.result_type)?;
            let region = region_of(variable.storage);
            if let Some(builtin) = program.module.builtin_of(id)? {
                return Ok(Value::Pointer(Pointer {
                    region: Region::Builtin(builtin),
                    address: 0,
                    pointee,
                }));
            }
            let offset = program.module_offsets[id as usize].unwrap_or(0);
            return Ok(Value::Pointer(Pointer {
                region,
                address: offset as u64,
                pointee,
            }));
        }
        Err(Error::UnboundValue(id))
    }

    fn bind(&mut self, depth: usize, id: spirv::Id, value: Value) {
        self.frames[depth].values[id as usize] = Some(value);
    }

    fn load(
        &mut self,
        program: &Program,
        host: &mut dyn Host,
        pointer: Pointer,
    ) -> Result<Value, Error> {
        if let Region::Builtin(builtin) = pointer.region {
            let raw = host.builtin(builtin);
            let count = program.module.component_count(pointer.pointee)?;
            let component = program.module.component_type(pointer.pointee)?;
            let width = program.module.scalar_width(component)?;
            if count == 1 {
                return Ok(Value::Scalar {
                    bits: mask(raw[0], width),
                    width,
                });
            }
            let mut members = Vec::with_capacity(count);
            for index in 0..count {
                members.push(Value::Scalar {
                    bits: mask(raw.get(index).copied().unwrap_or(0), width),
                    width,
                });
            }
            return Ok(Value::Composite(members));
        }

        let size = program.module.layout_of(pointer.pointee)?.size;
        let mut bytes = vec![0u8; size];
        match pointer.region {
            Region::Global => host.read(pointer.address, &mut bytes)?,
            Region::Local => host.read_local(pointer.address, &mut bytes)?,
            Region::Invocation => {
                let start = pointer.address as usize;
                if start + size > self.storage.len() {
                    return Err(Error::OutOfBounds);
                }
                bytes.copy_from_slice(&self.storage[start..start + size]);
            }
            Region::Module => {
                let start = pointer.address as usize;
                if start + size > program.module_storage.len() {
                    return Err(Error::OutOfBounds);
                }
                bytes.copy_from_slice(&program.module_storage[start..start + size]);
            }
            Region::Builtin(_) => unreachable!(),
        }
        program.decode(pointer.pointee, &bytes)
    }

    fn store(
        &mut self,
        program: &Program,
        host: &mut dyn Host,
        pointer: Pointer,
        value: &Value,
    ) -> Result<(), Error> {
        let size = program.module.layout_of(pointer.pointee)?.size;
        let mut bytes = vec![0u8; size];
        program.encode(value, pointer.pointee, &mut bytes)?;
        match pointer.region {
            Region::Global => host.write(pointer.address, &bytes)?,
            Region::Local => host.write_local(pointer.address, &bytes)?,
            Region::Invocation => {
                let start = pointer.address as usize;
                if start + size > self.storage.len() {
                    return Err(Error::OutOfBounds);
                }
                self.storage[start..start + size].copy_from_slice(&bytes);
            }
            Region::Module | Region::Builtin(_) => return Err(Error::ReadOnlyRegion),
        }
        Ok(())
    }

    fn execute(
        &mut self,
        program: &Program,
        host: &mut dyn Host,
        depth: usize,
        instruction: &spirv::Instruction,
    ) -> Result<Flow, Error> {
        match instruction {
            spirv::Instruction::Nop => Ok(Flow::Next),

            spirv::Instruction::Undef {
                result,
                result_type,
            } => {
                let value = program.zeroed(*result_type)?;
                self.bind(depth, *result, value);
                Ok(Flow::Next)
            }

            spirv::Instruction::Variable {
                result,
                result_type,
                initializer,
            } => {
                let pointee = program.module.pointee_of(*result_type)?;
                let layout = program.module.layout_of(pointee)?.clone();
                let mut offset = self.storage.len();
                if layout.alignment > 0 {
                    let remainder = offset % layout.alignment;
                    if remainder != 0 {
                        offset += layout.alignment - remainder;
                    }
                }
                self.storage.resize(offset + layout.size, 0);
                let pointer = Pointer {
                    region: Region::Invocation,
                    address: offset as u64,
                    pointee,
                };
                if let Some(initializer) = initializer {
                    let value = self.value(program, depth, *initializer)?;
                    self.store(program, host, pointer, &value)?;
                }
                self.bind(depth, *result, Value::Pointer(pointer));
                Ok(Flow::Next)
            }

            spirv::Instruction::Load {
                result,
                pointer,
                ..
            } => {
                let target = self.value(program, depth, *pointer)?.as_pointer()?;
                let value = self.load(program, host, target)?;
                self.bind(depth, *result, value);
                Ok(Flow::Next)
            }

            spirv::Instruction::Store { pointer, object } => {
                let target = self.value(program, depth, *pointer)?.as_pointer()?;
                let value = self.value(program, depth, *object)?;
                self.store(program, host, target, &value)?;
                Ok(Flow::Next)
            }

            spirv::Instruction::AccessChain {
                result,
                result_type,
                base,
                indices,
                element,
            } => {
                let mut pointer = self.value(program, depth, *base)?.as_pointer()?;
                let mut walked = 0;
                if *element {
                    let index = self.value(program, depth, indices[0])?.scalar_bits()?;
                    let stride = program.module.layout_of(pointer.pointee)?.size as u64;
                    pointer.address = pointer.address.wrapping_add(index.wrapping_mul(stride));
                    walked = 1;
                }
                for index_id in &indices[walked..] {
                    let index = self.value(program, depth, *index_id)?.scalar_bits()? as usize;
                    let offset = program.module.member_offset(pointer.pointee, index)?;
                    pointer.pointee = program.module.member_type(pointer.pointee, index)?;
                    pointer.address = pointer.address.wrapping_add(offset as u64);
                }
                pointer.pointee = program.module.pointee_of(*result_type)?;
                self.bind(depth, *result, Value::Pointer(pointer));
                Ok(Flow::Next)
            }

            spirv::Instruction::Unary {
                result,
                result_type,
                operation,
                operand,
            } => {
                let value = self.value(program, depth, *operand)?;
                let computed = componentwise_unary(program, *result_type, *operation, &value)?;
                self.bind(depth, *result, computed);
                Ok(Flow::Next)
            }

            spirv::Instruction::Binary {
                result,
                result_type,
                operation,
                lhs,
                rhs,
            } => {
                let left = self.value(program, depth, *lhs)?;
                let right = self.value(program, depth, *rhs)?;
                let computed =
                    componentwise_binary(program, *result_type, *operation, &left, &right)?;
                self.bind(depth, *result, computed);
                Ok(Flow::Next)
            }

            spirv::Instruction::Convert {
                result,
                result_type,
                operation,
                operand,
            } => {
                let value = self.value(program, depth, *operand)?;
                let rounding = program.module.rounding_mode_of(*result)?;
                let saturated = program.module.is_saturated_conversion(*result)?;
                let computed = componentwise_convert(
                    program,
                    *result_type,
                    *operation,
                    &value,
                    rounding,
                    saturated,
                )?;
                self.bind(depth, *result, computed);
                Ok(Flow::Next)
            }

            spirv::Instruction::Select {
                result,
                condition,
                true_value,
                false_value,
                ..
            } => {
                let taken = self.value(program, depth, *condition)?.as_bool()?;
                let chosen = if taken {
                    self.value(program, depth, *true_value)?
                } else {
                    self.value(program, depth, *false_value)?
                };
                self.bind(depth, *result, chosen);
                Ok(Flow::Next)
            }

            spirv::Instruction::CopyObject {
                result, operand, ..
            } => {
                let value = self.value(program, depth, *operand)?;
                self.bind(depth, *result, value);
                Ok(Flow::Next)
            }

            spirv::Instruction::CompositeConstruct {
                result,
                constituents,
                ..
            } => {
                let mut members = Vec::with_capacity(constituents.len());
                for member in constituents {
                    members.push(self.value(program, depth, *member)?);
                }
                self.bind(depth, *result, Value::Composite(members));
                Ok(Flow::Next)
            }

            spirv::Instruction::CompositeExtract {
                result,
                composite,
                indices,
                ..
            } => {
                let mut value = self.value(program, depth, *composite)?;
                for index in indices {
                    value = match value {
                        Value::Composite(members) => members
                            .get(*index as usize)
                            .cloned()
                            .ok_or(Error::OutOfBounds)?,
                        _ => return Err(Error::UnsupportedInstruction),
                    };
                }
                self.bind(depth, *result, value);
                Ok(Flow::Next)
            }

            spirv::Instruction::VectorExtractDynamic {
                result,
                vector,
                index,
                ..
            } => {
                let source = self.value(program, depth, *vector)?;
                let position = self.value(program, depth, *index)?.scalar_bits()? as usize;
                let value = match source {
                    Value::Composite(members) => {
                        members.get(position).cloned().ok_or(Error::OutOfBounds)?
                    }
                    other => other,
                };
                self.bind(depth, *result, value);
                Ok(Flow::Next)
            }

            spirv::Instruction::VectorInsertDynamic {
                result,
                vector,
                component,
                index,
                ..
            } => {
                let source = self.value(program, depth, *vector)?;
                let inserted = self.value(program, depth, *component)?;
                let position = self.value(program, depth, *index)?.scalar_bits()? as usize;
                let value = match source {
                    Value::Composite(mut members) => {
                        if position >= members.len() {
                            return Err(Error::OutOfBounds);
                        }
                        members[position] = inserted;
                        Value::Composite(members)
                    }
                    _ => return Err(Error::UnsupportedInstruction),
                };
                self.bind(depth, *result, value);
                Ok(Flow::Next)
            }

            spirv::Instruction::VectorTimesScalar {
                result,
                result_type,
                vector,
                scalar,
            } => {
                let source = self.value(program, depth, *vector)?;
                let factor = self.value(program, depth, *scalar)?;
                let component = program.module.component_type(*result_type)?;
                let members = match source {
                    Value::Composite(members) => members,
                    other => vec![other],
                };
                let mut scaled = Vec::with_capacity(members.len());
                for member in members {
                    scaled.push(binary_scalar(
                        program,
                        component,
                        spirv::BinaryOp::FMul,
                        &member,
                        &factor,
                    )?);
                }
                self.bind(depth, *result, Value::Composite(scaled));
                Ok(Flow::Next)
            }

            spirv::Instruction::AtomicCounter {
                result,
                result_type,
                pointer,
                increment,
            } => {
                let target = self.value(program, depth, *pointer)?.as_pointer()?;
                let width = program.module.scalar_width(*result_type)?;
                let operation = if *increment {
                    Atomic::Increment
                } else {
                    Atomic::Decrement
                };
                let previous = host.atomic(operation, target.address, width)?;
                self.bind(
                    depth,
                    *result,
                    Value::Scalar {
                        bits: mask(previous, width),
                        width,
                    },
                );
                Ok(Flow::Next)
            }

            spirv::Instruction::Phi { result, pairs, .. } => {
                let previous = self.frames[depth]
                    .previous_block
                    .ok_or(Error::NoPredecessor)?;
                let mut chosen = None;
                for (value, label) in pairs {
                    if *label == previous {
                        chosen = Some(*value);
                        break;
                    }
                }
                let source = chosen.ok_or(Error::NoPredecessor)?;
                let value = self.value(program, depth, source)?;
                self.bind(depth, *result, value);
                Ok(Flow::Next)
            }

            spirv::Instruction::FunctionCall {
                result,
                function,
                arguments,
                ..
            } => {
                let index = program.module.function_index(*function)?;
                let callee = &program.module.functions[index];
                if callee.parameters.len() != arguments.len() {
                    return Err(Error::ArgumentMismatch);
                }
                let mut values = vec![None; program.module.bound];
                for (parameter, argument) in callee.parameters.iter().zip(arguments) {
                    values[parameter.result as usize] =
                        Some(self.value(program, depth, *argument)?);
                }
                Ok(Flow::Called(Frame {
                    function: index,
                    block: 0,
                    instruction: 0,
                    previous_block: None,
                    values,
                    return_slot: Some(*result),
                    storage_watermark: self.storage.len(),
                }))
            }

            spirv::Instruction::Branch { target } => Ok(Flow::Jump(*target)),

            spirv::Instruction::BranchConditional {
                condition,
                true_target,
                false_target,
            } => {
                let taken = self.value(program, depth, *condition)?.as_bool()?;
                Ok(Flow::Jump(if taken { *true_target } else { *false_target }))
            }

            spirv::Instruction::Switch {
                selector,
                default_target,
                cases,
            } => {
                let value = self.value(program, depth, *selector)?.scalar_bits()?;
                let mut target = *default_target;
                for (literal, label) in cases {
                    if *literal == value {
                        target = *label;
                        break;
                    }
                }
                Ok(Flow::Jump(target))
            }

            spirv::Instruction::Barrier {
                execution_scope,
                memory_semantics,
            } => {
                let scope = self.value(program, depth, *execution_scope)?.scalar_bits()?;
                let semantics = self.value(program, depth, *memory_semantics)?.scalar_bits()?;
                host.memory_barrier(semantics as u32);
                Ok(Flow::Yielded {
                    execution_scope: scope,
                    semantics,
                })
            }

            spirv::Instruction::Return => Ok(Flow::Returned(Value::Void)),

            spirv::Instruction::ReturnValue { value } => {
                let returned = self.value(program, depth, *value)?;
                Ok(Flow::Returned(returned))
            }

            spirv::Instruction::Unreachable => Err(Error::UnsupportedInstruction),
        }
    }
}

enum Flow {
    Next,
    Jump(spirv::Id),
    Called(Frame),
    Returned(Value),
    Yielded { execution_scope: u64, semantics: u64 },
}

fn componentwise_unary(
    program: &Program,
    result_type: spirv::Id,
    operation: spirv::UnaryOp,
    value: &Value,
) -> Result<Value, Error> {
    let component = program.module.component_type(result_type)?;
    match value {
        Value::Composite(members) => {
            let mut computed = Vec::with_capacity(members.len());
            for member in members {
                computed.push(unary_scalar(program, component, operation, member)?);
            }
            Ok(Value::Composite(computed))
        }
        other => unary_scalar(program, component, operation, other),
    }
}

fn unary_scalar(
    program: &Program,
    component: spirv::Id,
    operation: spirv::UnaryOp,
    value: &Value,
) -> Result<Value, Error> {
    let width = program.module.scalar_width(component)?;
    let bits = value.scalar_bits()?;
    let computed = match operation {
        spirv::UnaryOp::SNegate => mask((-sign_extend(bits, width)) as u64, width),
        spirv::UnaryOp::Not => mask(!bits, width),
        spirv::UnaryOp::FNegate => {
            if width == 64 {
                (-f64::from_bits(bits)).to_bits()
            } else {
                (-f32::from_bits(bits as u32)).to_bits() as u64
            }
        }
    };
    Ok(Value::Scalar {
        bits: computed,
        width,
    })
}

fn componentwise_binary(
    program: &Program,
    result_type: spirv::Id,
    operation: spirv::BinaryOp,
    left: &Value,
    right: &Value,
) -> Result<Value, Error> {
    let component = program.module.component_type(result_type)?;
    match (left, right) {
        (Value::Composite(first), Value::Composite(second)) => {
            let mut computed = Vec::with_capacity(first.len());
            for (one, other) in first.iter().zip(second) {
                computed.push(binary_scalar(program, component, operation, one, other)?);
            }
            Ok(Value::Composite(computed))
        }
        (Value::Composite(first), scalar) => {
            let mut computed = Vec::with_capacity(first.len());
            for one in first {
                computed.push(binary_scalar(program, component, operation, one, scalar)?);
            }
            Ok(Value::Composite(computed))
        }
        (one, other) => binary_scalar(program, component, operation, one, other),
    }
}

fn binary_scalar(
    program: &Program,
    component: spirv::Id,
    operation: spirv::BinaryOp,
    left: &Value,
    right: &Value,
) -> Result<Value, Error> {
    let comparison = matches!(
        operation,
        spirv::BinaryOp::ULessThan | spirv::BinaryOp::SLessThan | spirv::BinaryOp::INotEqual
    );
    let width = if comparison {
        operand_width(left)
    } else {
        program.module.scalar_width(component)?
    };
    let lhs = left.scalar_bits()?;
    let rhs = right.scalar_bits()?;

    if comparison {
        let outcome = match operation {
            spirv::BinaryOp::ULessThan => mask(lhs, width) < mask(rhs, width),
            spirv::BinaryOp::SLessThan => sign_extend(lhs, width) < sign_extend(rhs, width),
            _ => mask(lhs, width) != mask(rhs, width),
        };
        return Ok(Value::Bool(outcome));
    }

    let float = matches!(
        operation,
        spirv::BinaryOp::FAdd
            | spirv::BinaryOp::FSub
            | spirv::BinaryOp::FMul
            | spirv::BinaryOp::FDiv
            | spirv::BinaryOp::FRem
            | spirv::BinaryOp::FMod
    );

    let computed = if float {
        if width == 64 {
            let one = f64::from_bits(lhs);
            let other = f64::from_bits(rhs);
            let outcome = match operation {
                spirv::BinaryOp::FAdd => one + other,
                spirv::BinaryOp::FSub => one - other,
                spirv::BinaryOp::FMul => one * other,
                spirv::BinaryOp::FDiv => one / other,
                spirv::BinaryOp::FRem => one % other,
                _ => remainder_with_sign(one, other),
            };
            outcome.to_bits()
        } else {
            let one = f32::from_bits(lhs as u32);
            let other = f32::from_bits(rhs as u32);
            let outcome = match operation {
                spirv::BinaryOp::FAdd => one + other,
                spirv::BinaryOp::FSub => one - other,
                spirv::BinaryOp::FMul => one * other,
                spirv::BinaryOp::FDiv => one / other,
                spirv::BinaryOp::FRem => one % other,
                _ => remainder_with_sign(one as f64, other as f64) as f32,
            };
            outcome.to_bits() as u64
        }
    } else {
        match operation {
            spirv::BinaryOp::IAdd => mask(lhs.wrapping_add(rhs), width),
            spirv::BinaryOp::ISub => mask(lhs.wrapping_sub(rhs), width),
            spirv::BinaryOp::IMul => mask(lhs.wrapping_mul(rhs), width),
            spirv::BinaryOp::UMod => {
                let divisor = mask(rhs, width);
                if divisor == 0 {
                    0
                } else {
                    mask(lhs, width) % divisor
                }
            }
            spirv::BinaryOp::ShiftLeftLogical => {
                let amount = mask(rhs, width);
                if amount >= u64::from(width) {
                    0
                } else {
                    mask(lhs << amount, width)
                }
            }
            spirv::BinaryOp::ShiftRightArithmetic => {
                let amount = mask(rhs, width);
                if amount >= u64::from(width) {
                    let sign = sign_extend(lhs, width) < 0;
                    if sign { mask(u64::MAX, width) } else { 0 }
                } else {
                    mask((sign_extend(lhs, width) >> amount) as u64, width)
                }
            }
            _ => return Err(Error::UnsupportedInstruction),
        }
    };

    Ok(Value::Scalar {
        bits: computed,
        width,
    })
}

fn operand_width(value: &Value) -> u32 {
    match value {
        Value::Scalar { width, .. } => *width,
        _ => 32,
    }
}

fn remainder_with_sign(one: f64, other: f64) -> f64 {
    let remainder = one % other;
    if remainder != 0.0 && (remainder < 0.0) != (other < 0.0) {
        remainder + other
    } else {
        remainder
    }
}

fn componentwise_convert(
    program: &Program,
    result_type: spirv::Id,
    operation: spirv::ConvertOp,
    value: &Value,
    rounding: Option<spirv::RoundingMode>,
    saturated: bool,
) -> Result<Value, Error> {
    let component = program.module.component_type(result_type)?;
    match value {
        Value::Composite(members) => {
            let mut computed = Vec::with_capacity(members.len());
            for member in members {
                computed.push(convert_scalar(
                    program, component, operation, member, rounding, saturated,
                )?);
            }
            Ok(Value::Composite(computed))
        }
        other => convert_scalar(program, component, operation, other, rounding, saturated),
    }
}

fn convert_scalar(
    program: &Program,
    component: spirv::Id,
    operation: spirv::ConvertOp,
    value: &Value,
    rounding: Option<spirv::RoundingMode>,
    saturated: bool,
) -> Result<Value, Error> {
    let width = program.module.scalar_width(component)?;
    let bits = value.scalar_bits()?;
    let source_width = operand_width(value);

    let computed = match operation {
        spirv::ConvertOp::UConvert => mask(bits, width),
        spirv::ConvertOp::SConvert => mask(sign_extend(bits, source_width) as u64, width),
        spirv::ConvertOp::FToS | spirv::ConvertOp::FToU => {
            let source = if source_width == 64 {
                f64::from_bits(bits)
            } else {
                f32::from_bits(bits as u32) as f64
            };
            let rounded = match rounding {
                Some(spirv::RoundingMode::ToNearestEven) => round_to_nearest_even(source),
                Some(spirv::RoundingMode::TowardPositive) => source.ceil(),
                Some(spirv::RoundingMode::TowardNegative) => source.floor(),
                Some(spirv::RoundingMode::TowardZero) | None => source.trunc(),
            };
            if operation == spirv::ConvertOp::FToS {
                let low = -(2f64.powi(width as i32 - 1));
                let high = 2f64.powi(width as i32 - 1) - 1.0;
                let clamped = if saturated {
                    rounded.max(low).min(high)
                } else {
                    rounded
                };
                mask(clamped as i64 as u64, width)
            } else {
                let high = 2f64.powi(width as i32) - 1.0;
                let clamped = if saturated {
                    rounded.max(0.0).min(high)
                } else {
                    rounded
                };
                mask(clamped as u64, width)
            }
        }
    };

    Ok(Value::Scalar {
        bits: computed,
        width,
    })
}

fn round_to_nearest_even(value: f64) -> f64 {
    let rounded = value.round();
    if (value - value.trunc()).abs() == 0.5 && rounded % 2.0 != 0.0 {
        rounded - value.signum()
    } else {
        rounded
    }
}
