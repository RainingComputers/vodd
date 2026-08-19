use crate::bitcode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Bitcode(bitcode::Error),
    NotAScalar,
    NotABool,
    NotAPointer,
    NotAComposite,
    UnsupportedOperation,
    NotEnoughBytes,
    UnsupportedStorageClass(bitcode::StorageClass),
}

impl From<bitcode::Error> for Error {
    fn from(error: bitcode::Error) -> Error {
        Error::Bitcode(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Global,
    Local,
    Module,
    Invocation,
    Builtin(bitcode::Builtin),
}

impl Region {
    pub fn from_storage_class(storage: bitcode::StorageClass) -> Result<Region, Error> {
        Ok(match storage {
            bitcode::StorageClass::CrossWorkgroup => Region::Global,
            bitcode::StorageClass::Workgroup => Region::Local,
            bitcode::StorageClass::UniformConstant | bitcode::StorageClass::Private => {
                Region::Module
            }
            bitcode::StorageClass::Function => Region::Invocation,
            bitcode::StorageClass::Input | bitcode::StorageClass::Generic => {
                return Err(Error::UnsupportedStorageClass(storage));
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pointer {
    pub region: Region,
    pub address: u64,
    pub pointee_type: bitcode::Id,
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
    pub fn from_bits(bits: u64, width: u32) -> Value {
        Value::Scalar { bits: mask(bits, width), width }
    }

    pub(crate) fn as_bits(&self) -> Result<u64, Error> {
        match self {
            Value::Scalar { bits, .. } => Ok(*bits),
            Value::Bool(flag) => Ok(u64::from(*flag)),
            _ => Err(Error::NotAScalar),
        }
    }

    pub(crate) fn as_bool(&self) -> Result<bool, Error> {
        match self {
            Value::Bool(flag) => Ok(*flag),
            Value::Scalar { bits, .. } => Ok(*bits != 0),
            _ => Err(Error::NotABool),
        }
    }

    pub(crate) fn as_pointer(&self) -> Result<Pointer, Error> {
        match self {
            Value::Pointer(pointer) => Ok(*pointer),
            _ => Err(Error::NotAPointer),
        }
    }
}

pub(crate) fn encode(
    module: &bitcode::Module,
    type_id: bitcode::Id,
    value: &Value,
    destination: &mut [u8],
) -> Result<(), Error> {
    match value {
        Value::Void => Ok(()),
        Value::Bool(flag) => bits_to_le(destination, u64::from(*flag), 1),
        Value::Scalar { bits, width } => {
            bits_to_le(destination, *bits, (*width as usize).div_ceil(8))
        }
        Value::Pointer(pointer) => {
            bits_to_le(destination, pointer.address, module.layout(type_id)?.size)
        }
        Value::Composite(members) => {
            for (index, member) in members.iter().enumerate() {
                let member_type = module.member_type(type_id, index)?;
                let offset = module.member_offset(type_id, index)?;
                let end = offset + module.layout(member_type)?.size;

                if destination.len() < end {
                    return Err(Error::NotEnoughBytes);
                }

                encode(module, member_type, member, &mut destination[offset..end])?;
            }

            Ok(())
        }
    }
}

pub(crate) fn decode(
    module: &bitcode::Module,
    type_id: bitcode::Id,
    source: &[u8],
) -> Result<Value, Error> {
    Ok(match module.type_(type_id)? {
        bitcode::Type::Void => Value::Void,
        bitcode::Type::Bool => Value::Bool(*source.first().ok_or(Error::NotEnoughBytes)? != 0),
        bitcode::Type::Int { width } | bitcode::Type::Float { width } => {
            Value::from_bits(bits_from_le(source, (*width as usize).div_ceil(8))?, *width)
        }
        bitcode::Type::Pointer { storage, pointee_type } => Value::Pointer(Pointer {
            region: Region::from_storage_class(*storage)?,
            address: bits_from_le(source, module.layout(type_id)?.size)?,
            pointee_type: *pointee_type,
        }),
        bitcode::Type::Vector { count, .. } => {
            decode_inner(module, type_id, source, *count as usize)?
        }
        bitcode::Type::Array { count, .. } => {
            decode_inner(module, type_id, source, *count as usize)?
        }
        bitcode::Type::Struct { member_types } => {
            decode_inner(module, type_id, source, member_types.len())?
        }
        bitcode::Type::Function { .. } => Value::Void,
    })
}

fn decode_inner(
    module: &bitcode::Module,
    type_id: bitcode::Id,
    source: &[u8],
    count: usize,
) -> Result<Value, Error> {
    let values = (0..count)
        .map(|index| {
            let member_type = module.member_type(type_id, index)?;
            let offset = module.member_offset(type_id, index)?;
            let end = offset + module.layout(member_type)?.size;

            if source.len() < end {
                return Err(Error::NotEnoughBytes);
            }

            decode(module, member_type, &source[offset..end])
        })
        .collect::<Result<Vec<_>, Error>>()?;

    Ok(Value::Composite(values))
}

pub(crate) fn from_raw(
    module: &bitcode::Module,
    type_id: bitcode::Id,
    raw: [u64; 3],
) -> Result<Value, Error> {
    let count = module.component_count(type_id)?;
    let component_type = module.component_type(type_id)?;
    let width = module.scalar_width(component_type)?;

    if count == 1 {
        return Ok(Value::from_bits(raw[0], width));
    }

    Ok(Value::Composite(
        (0..count)
            .map(|index| Value::from_bits(raw.get(index).copied().unwrap_or(0), width))
            .collect(),
    ))
}

pub(crate) fn constant(module: &bitcode::Module, id: bitcode::Id) -> Result<Value, Error> {
    let declared = module.constant(id)?;

    match &declared.kind {
        bitcode::ConstantKind::True => Ok(Value::Bool(true)),
        bitcode::ConstantKind::False => Ok(Value::Bool(false)),
        bitcode::ConstantKind::Scalar { bits } => {
            let width = module.scalar_width(declared.result_type)?;
            Ok(Value::from_bits(*bits, width))
        }
        bitcode::ConstantKind::Composite { members } => Ok(Value::Composite(
            members
                .iter()
                .map(|member| constant(module, *member))
                .collect::<Result<Vec<_>, Error>>()?,
        )),
        bitcode::ConstantKind::Null => zeroed(module, declared.result_type),
    }
}

pub(crate) fn zeroed(module: &bitcode::Module, type_id: bitcode::Id) -> Result<Value, Error> {
    Ok(match module.type_(type_id)? {
        bitcode::Type::Void => Value::Void,
        bitcode::Type::Bool => Value::Bool(false),
        bitcode::Type::Int { width } | bitcode::Type::Float { width } => {
            Value::from_bits(0, *width)
        }
        bitcode::Type::Pointer { storage, pointee_type } => Value::Pointer(Pointer {
            region: Region::from_storage_class(*storage)?,
            address: 0,
            pointee_type: *pointee_type,
        }),
        bitcode::Type::Vector { component_type, count } => Value::Composite(
            (0..*count)
                .map(|_| zeroed(module, *component_type))
                .collect::<Result<Vec<_>, Error>>()?,
        ),
        bitcode::Type::Array { element_type, count } => Value::Composite(
            (0..*count)
                .map(|_| zeroed(module, *element_type))
                .collect::<Result<Vec<_>, Error>>()?,
        ),
        bitcode::Type::Struct { member_types } => Value::Composite(
            member_types
                .iter()
                .map(|member_type| zeroed(module, *member_type))
                .collect::<Result<Vec<_>, Error>>()?,
        ),
        bitcode::Type::Function { .. } => Value::Void,
    })
}

pub(crate) fn unary_op(
    module: &bitcode::Module,
    result_type: bitcode::Id,
    operation: bitcode::UnaryOp,
    value: &Value,
) -> Result<Value, Error> {
    let component_type = module.component_type(result_type)?;

    match value {
        Value::Composite(members) => Ok(Value::Composite(
            members
                .iter()
                .map(|member| unary_op_scalar(module, component_type, operation, member))
                .collect::<Result<Vec<_>, Error>>()?,
        )),
        scalar => unary_op_scalar(module, component_type, operation, scalar),
    }
}

fn unary_op_scalar(
    module: &bitcode::Module,
    component_type: bitcode::Id,
    operation: bitcode::UnaryOp,
    value: &Value,
) -> Result<Value, Error> {
    let width = module.scalar_width(component_type)?;
    let bits = value.as_bits()?;

    let result = match operation {
        bitcode::UnaryOp::SNegate => mask((-sign_extend(bits, width)) as u64, width),
        bitcode::UnaryOp::Not => mask(!bits, width),
        bitcode::UnaryOp::FNegate => {
            if width == 64 {
                (-f64::from_bits(bits)).to_bits()
            } else {
                (-f32::from_bits(bits as u32)).to_bits() as u64
            }
        }
    };

    Ok(Value::from_bits(result, width))
}

pub(crate) fn binary_op(
    module: &bitcode::Module,
    result_type: bitcode::Id,
    operation: bitcode::BinaryOp,
    left: &Value,
    right: &Value,
) -> Result<Value, Error> {
    let component_type = module.component_type(result_type)?;

    match (left, right) {
        (Value::Composite(left), Value::Composite(right)) => Ok(Value::Composite(
            left.iter()
                .zip(right)
                .map(|(left, right)| {
                    binary_op_scalar(module, component_type, operation, left, right)
                })
                .collect::<Result<Vec<_>, Error>>()?,
        )),
        (Value::Composite(left), scalar) => Ok(Value::Composite(
            left.iter()
                .map(|left| binary_op_scalar(module, component_type, operation, left, scalar))
                .collect::<Result<Vec<_>, Error>>()?,
        )),
        (left, right) => binary_op_scalar(module, component_type, operation, left, right),
    }
}

pub(crate) fn binary_op_scalar(
    module: &bitcode::Module,
    component_type: bitcode::Id,
    operation: bitcode::BinaryOp,
    left: &Value,
    right: &Value,
) -> Result<Value, Error> {
    let comparison = matches!(
        operation,
        bitcode::BinaryOp::ULessThan | bitcode::BinaryOp::SLessThan | bitcode::BinaryOp::INotEqual
    );
    let width = if comparison {
        operand_width(left)
    } else {
        module.scalar_width(component_type)?
    };
    let left = left.as_bits()?;
    let right = right.as_bits()?;

    if comparison {
        let result = match operation {
            bitcode::BinaryOp::ULessThan => mask(left, width) < mask(right, width),
            bitcode::BinaryOp::SLessThan => sign_extend(left, width) < sign_extend(right, width),
            _ => mask(left, width) != mask(right, width),
        };

        return Ok(Value::Bool(result));
    }

    let float = matches!(
        operation,
        bitcode::BinaryOp::FAdd
            | bitcode::BinaryOp::FSub
            | bitcode::BinaryOp::FMul
            | bitcode::BinaryOp::FDiv
            | bitcode::BinaryOp::FRem
            | bitcode::BinaryOp::FMod
    );

    let result = if float {
        if width == 64 {
            let left = f64::from_bits(left);
            let right = f64::from_bits(right);

            let result = match operation {
                bitcode::BinaryOp::FAdd => left + right,
                bitcode::BinaryOp::FSub => left - right,
                bitcode::BinaryOp::FMul => left * right,
                bitcode::BinaryOp::FDiv => left / right,
                bitcode::BinaryOp::FRem => left % right,
                _ => remainder_with_sign(left, right),
            };

            result.to_bits()
        } else {
            let left = f32::from_bits(left as u32);
            let right = f32::from_bits(right as u32);

            let result = match operation {
                bitcode::BinaryOp::FAdd => left + right,
                bitcode::BinaryOp::FSub => left - right,
                bitcode::BinaryOp::FMul => left * right,
                bitcode::BinaryOp::FDiv => left / right,
                bitcode::BinaryOp::FRem => left % right,
                _ => remainder_with_sign(left as f64, right as f64) as f32,
            };

            result.to_bits() as u64
        }
    } else {
        match operation {
            bitcode::BinaryOp::IAdd => mask(left.wrapping_add(right), width),
            bitcode::BinaryOp::ISub => mask(left.wrapping_sub(right), width),
            bitcode::BinaryOp::IMul => mask(left.wrapping_mul(right), width),
            bitcode::BinaryOp::UMod => {
                let divisor = mask(right, width);

                if divisor == 0 {
                    0
                } else {
                    mask(left, width) % divisor
                }
            }
            bitcode::BinaryOp::ShiftLeftLogical => {
                let amount = mask(right, width);

                if amount >= u64::from(width) {
                    0
                } else {
                    mask(left << amount, width)
                }
            }
            bitcode::BinaryOp::ShiftRightArithmetic => {
                let amount = mask(right, width);

                if amount >= u64::from(width) {
                    if sign_extend(left, width) < 0 {
                        mask(u64::MAX, width)
                    } else {
                        0
                    }
                } else {
                    mask((sign_extend(left, width) >> amount) as u64, width)
                }
            }
            _ => return Err(Error::UnsupportedOperation),
        }
    };

    Ok(Value::from_bits(result, width))
}

pub(crate) fn convert_op(
    module: &bitcode::Module,
    result_type: bitcode::Id,
    operation: bitcode::ConvertOp,
    value: &Value,
    rounding: Option<bitcode::RoundingMode>,
    saturated: bool,
) -> Result<Value, Error> {
    let component_type = module.component_type(result_type)?;

    match value {
        Value::Composite(members) => Ok(Value::Composite(
            members
                .iter()
                .map(|member| {
                    convert_op_scalar(
                        module,
                        component_type,
                        operation,
                        member,
                        rounding,
                        saturated,
                    )
                })
                .collect::<Result<Vec<_>, Error>>()?,
        )),
        scalar => convert_op_scalar(
            module,
            component_type,
            operation,
            scalar,
            rounding,
            saturated,
        ),
    }
}

fn convert_op_scalar(
    module: &bitcode::Module,
    component_type: bitcode::Id,
    operation: bitcode::ConvertOp,
    value: &Value,
    rounding: Option<bitcode::RoundingMode>,
    saturated: bool,
) -> Result<Value, Error> {
    let width = module.scalar_width(component_type)?;
    let source_bits = value.as_bits()?;
    let source_width = operand_width(value);

    let result = match operation {
        bitcode::ConvertOp::UConvert => mask(source_bits, width),
        bitcode::ConvertOp::SConvert => mask(sign_extend(source_bits, source_width) as u64, width),
        bitcode::ConvertOp::FToS | bitcode::ConvertOp::FToU => {
            let source = if source_width == 64 {
                f64::from_bits(source_bits)
            } else {
                f32::from_bits(source_bits as u32) as f64
            };

            let rounded = match rounding {
                Some(bitcode::RoundingMode::ToNearestEven) => round_to_nearest_even(source),
                Some(bitcode::RoundingMode::TowardPositive) => source.ceil(),
                Some(bitcode::RoundingMode::TowardNegative) => source.floor(),
                Some(bitcode::RoundingMode::TowardZero) | None => source.trunc(),
            };

            if operation == bitcode::ConvertOp::FToS {
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

    Ok(Value::from_bits(result, width))
}

fn operand_width(value: &Value) -> u32 {
    match value {
        Value::Scalar { width, .. } => *width,
        _ => 32,
    }
}

fn round_to_nearest_even(value: f64) -> f64 {
    let rounded = value.round();

    if (value - value.trunc()).abs() == 0.5 && rounded % 2.0 != 0.0 {
        rounded - value.signum()
    } else {
        rounded
    }
}

fn remainder_with_sign(left: f64, right: f64) -> f64 {
    let remainder = left % right;

    if remainder != 0.0 && (remainder < 0.0) != (right < 0.0) {
        remainder + right
    } else {
        remainder
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

fn mask(bits: u64, width: u32) -> u64 {
    if width >= 64 {
        bits
    } else {
        bits & ((1u64 << width) - 1)
    }
}

fn bits_to_le(destination: &mut [u8], bits: u64, size: usize) -> Result<(), Error> {
    if destination.len() < size {
        return Err(Error::NotEnoughBytes);
    }

    destination[..size].copy_from_slice(&bits.to_le_bytes()[..size]);

    Ok(())
}

fn bits_from_le(source: &[u8], size: usize) -> Result<u64, Error> {
    if source.len() < size {
        return Err(Error::NotEnoughBytes);
    }

    let mut buffer = [0u8; 8];
    buffer[..size].copy_from_slice(&source[..size]);

    Ok(u64::from_le_bytes(buffer))
}
