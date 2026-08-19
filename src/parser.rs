use crate::bitcode;

const MAGIC: u32 = 0x0723_0203;

struct Reader<'w, I: Iterator<Item = u32>> {
    words: &'w mut I,
    remaining: usize,
}

impl<'w, I: Iterator<Item = u32>> Reader<'w, I> {
    fn from_iter(words: &'w mut I, word_count: usize) -> Reader<'w, I> {
        Reader { words, remaining: word_count - 1 }
    }

    fn try_word(&mut self) -> Option<u32> {
        if self.remaining == 0 {
            return None;
        }

        let value = self.words.next()?;
        self.remaining -= 1;

        Some(value)
    }

    fn word(&mut self) -> Result<u32, bitcode::Error> {
        self.try_word().ok_or(bitcode::Error::Truncated)
    }

    fn rest(&mut self) -> Vec<u32> {
        std::iter::from_fn(|| self.try_word()).collect()
    }

    fn string(&mut self) -> String {
        let mut bytes = Vec::new();

        while let Some(word) = self.try_word() {
            for shift in 0..4 {
                let byte = ((word >> (shift * 8)) & 0xFF) as u8;
                if byte == 0 {
                    return String::from_utf8_lossy(&bytes).into_owned();
                }

                bytes.push(byte);
            }
        }

        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn finish(mut self) -> Result<(), bitcode::Error> {
        while self.remaining > 0 {
            self.words
                .next()
                .ok_or(bitcode::Error::IncompleteInstruction)?;
            self.remaining -= 1;
        }

        Ok(())
    }
}

pub fn parse(bytes: &[u8]) -> Result<bitcode::Module, bitcode::Error> {
    if !bytes.len().is_multiple_of(4) {
        return Err(bitcode::Error::UnalignedLength);
    }

    parse_inner(
        bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])),
    )
}

fn parse_inner(words: impl IntoIterator<Item = u32>) -> Result<bitcode::Module, bitcode::Error> {
    let mut words = words.into_iter();

    let mut header = [0u32; 5];
    for slot in &mut header {
        *slot = words.next().ok_or(bitcode::Error::NotSpirv)?;
    }

    if header[0] != MAGIC {
        return Err(bitcode::Error::NotSpirv);
    }

    let bound = header[3] as usize;

    let mut module = bitcode::ModuleBuilder::from_bound(bound);

    let mut dec_groups: Vec<(bitcode::Id, Vec<bitcode::Id>)> = Vec::new();
    let mut pending_function: Option<bitcode::Function> = None;
    let mut pending_block: Option<bitcode::Block> = None;

    while let Some(header) = words.next() {
        let word_count = (header >> 16) as usize;
        let opcode = (header & 0xFFFF) as u16;

        if word_count == 0 {
            return Err(bitcode::Error::ZeroWordCount);
        }

        let mut reader = Reader::from_iter(&mut words, word_count);

        match opcode {
            14 => {
                let addressing = reader.word()?;
                module.set_addressing(match addressing {
                    1 => bitcode::Addressing::Physical32,
                    2 => bitcode::Addressing::Physical64,
                    _ => bitcode::Addressing::Logical,
                });
            }
            15 => {
                let _model = reader.word()?;
                let function = reader.word()?;
                let name = reader.string();
                module.push_entry_point(bitcode::EntryPoint { function, name });
            }
            71 => {
                let target = reader.word()?;
                let decoration = parse_decoration(&mut reader)?;
                module.set_decoration(target, decoration)?;
            }
            72 => {
                let target = reader.word()?;
                let _member = reader.word()?;
                let decoration = parse_decoration(&mut reader)?;
                module.set_decoration(target, decoration)?;
            }
            73 => {}
            74 => {
                let group = reader.word()?;
                dec_groups.push((group, reader.rest()));
            }
            19 => {
                let result = reader.word()?;
                module.set_type(result, bitcode::Type::Void)?;
            }
            20 => {
                let result = reader.word()?;
                module.set_type(result, bitcode::Type::Bool)?;
            }
            21 => {
                let result = reader.word()?;
                let width = reader.word()?;
                let _signedness = reader.word()?;
                module.set_type(result, bitcode::Type::Int { width })?;
            }
            22 => {
                let result = reader.word()?;
                let width = reader.word()?;
                module.set_type(result, bitcode::Type::Float { width })?;
            }
            23 => {
                let result = reader.word()?;
                let component_type = reader.word()?;
                let count = reader.word()?;
                module.set_type(result, bitcode::Type::Vector { component_type, count })?;
            }
            28 => {
                let result = reader.word()?;
                let element_type = reader.word()?;
                let length = reader.word()?;
                let count = match module.constant(length).map(|entry| &entry.kind) {
                    Ok(bitcode::ConstantKind::Scalar { bits }) => *bits,
                    _ => 0,
                };
                module.set_type(result, bitcode::Type::Array { element_type, count })?;
            }
            30 => {
                let result = reader.word()?;
                let member_types = reader.rest();
                module.set_type(result, bitcode::Type::Struct { member_types })?;
            }
            32 => {
                let result = reader.word()?;
                let storage = bitcode::StorageClass::from_word(reader.word()?)?;
                let pointee_type = reader.word()?;
                module.set_type(result, bitcode::Type::Pointer { storage, pointee_type })?;
            }
            33 => {
                let result = reader.word()?;
                let return_type = reader.word()?;
                let parameter_types = reader.rest();
                module.set_type(
                    result,
                    bitcode::Type::Function { return_type, parameter_types },
                )?;
            }
            41 | 48 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                module.set_constant(result, result_type, bitcode::ConstantKind::True)?;
            }
            42 | 49 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                module.set_constant(result, result_type, bitcode::ConstantKind::False)?;
            }
            43 | 50 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let mut bits = u64::from(reader.word()?);
                if let Some(high) = reader.try_word() {
                    bits |= u64::from(high) << 32;
                }
                module.set_constant(result, result_type, bitcode::ConstantKind::Scalar { bits })?;
            }
            44 | 51 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let constituents = reader.rest();
                module.set_constant(
                    result,
                    result_type,
                    bitcode::ConstantKind::Composite { members: constituents },
                )?;
            }
            46 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                module.set_constant(result, result_type, bitcode::ConstantKind::Null)?;
            }
            54 => {
                let return_type = reader.word()?;
                let result = reader.word()?;
                let _control = reader.word()?;
                let _function_type = reader.word()?;
                pending_function = Some(bitcode::Function {
                    result,
                    return_type,
                    parameters: Vec::new(),
                    blocks: Vec::new(),
                    block_of_label: vec![None; bound],
                });
            }
            55 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                if let Some(function) = pending_function.as_mut() {
                    function
                        .parameters
                        .push(bitcode::Parameter { result, result_type });
                }
            }
            56 => {
                if let Some(mut function) = pending_function.take() {
                    if let Some(block) = pending_block.take() {
                        function.block_of_label[block.label as usize] = Some(function.blocks.len());
                        function.blocks.push(block);
                    }
                    module.push_function(function)?;
                }
            }
            248 => {
                let label = reader.word()?;
                if let Some(function) = pending_function.as_mut()
                    && let Some(block) = pending_block.take()
                {
                    function.block_of_label[block.label as usize] = Some(function.blocks.len());
                    function.blocks.push(block);
                }
                pending_block = Some(bitcode::Block { label, instructions: Vec::new() });
            }
            59 => {
                let result_type = reader.word()?;
                let result = reader.word()?;
                let storage = bitcode::StorageClass::from_word(reader.word()?)?;
                let initializer = reader.try_word();

                if let Some(block) = pending_block.as_mut() {
                    block.instructions.push(bitcode::Instruction::Variable {
                        result,
                        result_type,
                        initializer,
                    });
                } else {
                    module.push_variable(bitcode::Variable {
                        result,
                        result_type,
                        storage,
                        initializer,
                    })?;
                }
            }
            _ => {
                if let Some(block) = pending_block.as_mut() {
                    block.instructions.push(parse_body(opcode, &mut reader)?);
                }
            }
        }

        reader.finish()?;
    }

    set_decoration_groups(&mut module, dec_groups)?;
    set_layouts(&mut module)?;

    Ok(module.finalize())
}

fn parse_decoration<I: Iterator<Item = u32>>(
    reader: &mut Reader<'_, I>,
) -> Result<bitcode::Decoration, bitcode::Error> {
    Ok(match reader.word()? {
        1 => bitcode::Decoration::SpecId(reader.word()?),
        10 => bitcode::Decoration::CPacked,
        11 => bitcode::Decoration::BuiltIn(bitcode::Builtin::from_word(reader.word()?)?),
        28 => bitcode::Decoration::SaturatedConversion,
        39 => bitcode::Decoration::FPRoundingMode(match reader.word()? {
            1 => bitcode::RoundingMode::TowardZero,
            2 => bitcode::RoundingMode::TowardPositive,
            3 => bitcode::RoundingMode::TowardNegative,
            _ => bitcode::RoundingMode::ToNearestEven,
        }),
        44 => bitcode::Decoration::Alignment(reader.word()?),
        _ => bitcode::Decoration::Ignored,
    })
}

fn parse_body<I: Iterator<Item = u32>>(
    opcode: u16,
    reader: &mut Reader<'_, I>,
) -> Result<bitcode::Instruction, bitcode::Error> {
    Ok(match opcode {
        1 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            bitcode::Instruction::Undef { result, result_type }
        }
        61 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let pointer = reader.word()?;
            bitcode::Instruction::Load { result, result_type, pointer }
        }
        62 => {
            let pointer = reader.word()?;
            let object = reader.word()?;
            bitcode::Instruction::Store { pointer, object }
        }
        65 | 66 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let base = reader.word()?;
            let indices = reader.rest();
            bitcode::Instruction::AccessChain { result, result_type, base, indices, element: false }
        }
        67 | 70 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let base = reader.word()?;
            let indices = reader.rest();
            bitcode::Instruction::AccessChain { result, result_type, base, indices, element: true }
        }
        126 | 127 | 200 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operand = reader.word()?;
            let operation = match opcode {
                126 => bitcode::UnaryOp::SNegate,
                127 => bitcode::UnaryOp::FNegate,
                _ => bitcode::UnaryOp::Not,
            };
            bitcode::Instruction::Unary { result, result_type, operation, operand }
        }
        128 | 129 | 130 | 131 | 132 | 133 | 136 | 137 | 140 | 141 | 171 | 176 | 177 | 195 | 196 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let lhs = reader.word()?;
            let rhs = reader.word()?;
            let operation = match opcode {
                128 => bitcode::BinaryOp::IAdd,
                129 => bitcode::BinaryOp::FAdd,
                130 => bitcode::BinaryOp::ISub,
                131 => bitcode::BinaryOp::FSub,
                132 => bitcode::BinaryOp::IMul,
                133 => bitcode::BinaryOp::FMul,
                136 => bitcode::BinaryOp::FDiv,
                137 => bitcode::BinaryOp::UMod,
                140 => bitcode::BinaryOp::FRem,
                141 => bitcode::BinaryOp::FMod,
                171 => bitcode::BinaryOp::INotEqual,
                176 => bitcode::BinaryOp::ULessThan,
                177 => bitcode::BinaryOp::SLessThan,
                195 => bitcode::BinaryOp::ShiftRightArithmetic,
                _ => bitcode::BinaryOp::ShiftLeftLogical,
            };
            bitcode::Instruction::Binary { result, result_type, operation, lhs, rhs }
        }
        109 | 110 | 113 | 114 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operand = reader.word()?;
            let operation = match opcode {
                109 => bitcode::ConvertOp::FToU,
                110 => bitcode::ConvertOp::FToS,
                113 => bitcode::ConvertOp::UConvert,
                _ => bitcode::ConvertOp::SConvert,
            };
            bitcode::Instruction::Convert { result, result_type, operation, operand }
        }
        169 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let condition = reader.word()?;
            let true_value = reader.word()?;
            let false_value = reader.word()?;
            bitcode::Instruction::Select { result, result_type, condition, true_value, false_value }
        }
        83 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let operand = reader.word()?;
            bitcode::Instruction::CopyObject { result, result_type, operand }
        }
        80 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let constituents = reader.rest();
            bitcode::Instruction::CompositeConstruct { result, result_type, members: constituents }
        }
        81 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let composite = reader.word()?;
            let indices = reader.rest();
            bitcode::Instruction::CompositeExtract { result, result_type, composite, indices }
        }
        77 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let index = reader.word()?;
            bitcode::Instruction::VectorExtractDynamic { result, result_type, vector, index }
        }
        78 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let component = reader.word()?;
            let index = reader.word()?;
            bitcode::Instruction::VectorInsertDynamic {
                result,
                result_type,
                vector,
                component,
                index,
            }
        }
        142 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let vector = reader.word()?;
            let scalar = reader.word()?;
            bitcode::Instruction::VectorTimesScalar { result, result_type, vector, scalar }
        }
        232 | 233 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let pointer = reader.word()?;
            let increment = opcode == 232;
            bitcode::Instruction::AtomicCounter { result, result_type, pointer, increment }
        }
        245 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let pairs = reader
                .rest()
                .chunks_exact(2)
                .map(|pair| (pair[0], pair[1]))
                .collect();
            bitcode::Instruction::Phi { result, result_type, pairs }
        }
        57 => {
            let result_type = reader.word()?;
            let result = reader.word()?;
            let function = reader.word()?;
            let arguments = reader.rest();
            bitcode::Instruction::FunctionCall { result, result_type, function, arguments }
        }
        249 => {
            let target = reader.word()?;
            bitcode::Instruction::Branch { target }
        }
        250 => {
            let condition = reader.word()?;
            let true_target = reader.word()?;
            let false_target = reader.word()?;
            bitcode::Instruction::BranchConditional { condition, true_target, false_target }
        }
        251 => {
            let selector = reader.word()?;
            let default_target = reader.word()?;
            let cases = reader
                .rest()
                .chunks_exact(2)
                .map(|pair| (u64::from(pair[0]), pair[1]))
                .collect();
            bitcode::Instruction::Switch { selector, default_target, cases }
        }
        224 => {
            let execution_scope = reader.word()?;
            let _memory_scope = reader.word()?;
            let memory_semantics = reader.word()?;
            bitcode::Instruction::Barrier { execution_scope, memory_semantics }
        }
        225 => {
            let _memory_scope = reader.word()?;
            let memory_semantics = reader.word()?;
            bitcode::Instruction::MemoryBarrier { memory_semantics }
        }
        253 => bitcode::Instruction::Return,
        254 => {
            let value = reader.word()?;
            bitcode::Instruction::ReturnValue { value }
        }
        255 => bitcode::Instruction::Unreachable,
        _ => bitcode::Instruction::Nop,
    })
}

fn set_decoration_groups(
    module: &mut bitcode::ModuleBuilder,
    groups: Vec<(bitcode::Id, Vec<bitcode::Id>)>,
) -> Result<(), bitcode::Error> {
    for (group, targets) in groups {
        let source = module.decorations(group)?.clone();
        for target in targets {
            module.merge_decoration(target, &source)?;
        }
    }

    Ok(())
}

fn set_layouts(module: &mut bitcode::ModuleBuilder) -> Result<(), bitcode::Error> {
    for type_id in 0..module.bound() {
        match calc_layout(module, type_id as bitcode::Id) {
            Ok(_) => (),
            Err(err) => match err {
                bitcode::Error::NotAType(_) => (),
                err => return Err(err),
            },
        }
    }

    Ok(())
}

fn calc_layout(
    module: &mut bitcode::ModuleBuilder,
    type_id: bitcode::Id,
) -> Result<&bitcode::Layout, bitcode::Error> {
    match module.layout(type_id) {
        Err(err) => match err {
            bitcode::Error::NotAType(_) => {
                let layout = calc_layout_inner(module, type_id)?;
                module.set_layout(type_id, layout)
            }
            err => Err(err),
        },
        Ok(_) => module.layout(type_id),
    }
}

fn calc_layout_inner(
    module: &mut bitcode::ModuleBuilder,
    type_id: bitcode::Id,
) -> Result<bitcode::Layout, bitcode::Error> {
    let declared = module.type_(type_id)?;

    Ok(match declared {
        bitcode::Type::Void => {
            bitcode::Layout { size: 0, alignment: 1, member_offsets: Vec::new() }
        }
        bitcode::Type::Function { .. } => {
            bitcode::Layout { size: 0, alignment: 1, member_offsets: Vec::new() }
        }
        bitcode::Type::Bool => {
            bitcode::Layout { size: 1, alignment: 1, member_offsets: Vec::new() }
        }
        bitcode::Type::Int { width } | bitcode::Type::Float { width } => {
            let size = (*width as usize).div_ceil(8);

            bitcode::Layout { size, alignment: size, member_offsets: Vec::new() }
        }
        bitcode::Type::Pointer { .. } => {
            let size = match module.addressing() {
                bitcode::Addressing::Physical32 => 4,
                _ => 8,
            };

            bitcode::Layout { size, alignment: size, member_offsets: Vec::new() }
        }
        bitcode::Type::Vector { component_type, count } => {
            let (component_type, count) = (*component_type, *count as usize);
            let lanes = if count == 3 { 4 } else { count };
            let element_type = calc_layout(module, component_type)?.size;

            bitcode::Layout {
                size: element_type * lanes,
                alignment: element_type * lanes,
                member_offsets: Vec::new(),
            }
        }
        bitcode::Type::Array { element_type, count } => {
            let (element_type, count) = (*element_type, *count as usize);
            let entry = calc_layout(module, element_type)?;

            bitcode::Layout {
                size: entry.size * count,
                alignment: entry.alignment,
                member_offsets: Vec::new(),
            }
        }
        bitcode::Type::Struct { member_types } => {
            let count = member_types.len();
            let packed = module.decorations(type_id)?.packed;

            let entries = (0..count)
                .map(|index| {
                    let member = module.member_type(type_id, index)?;
                    let entry = calc_layout(module, member)?;
                    Ok((entry.size, if packed { 1 } else { entry.alignment }))
                })
                .collect::<Result<Vec<_>, bitcode::Error>>()?;

            let alignment = entries
                .iter()
                .map(|(_, member_alignment)| *member_alignment)
                .max()
                .unwrap_or(1);

            let mut offset = 0;
            let member_offsets = entries
                .iter()
                .map(|(size, member_alignment)| {
                    let member_offset = bitcode::align_up(offset, *member_alignment);
                    offset = member_offset + size;
                    member_offset
                })
                .collect();

            bitcode::Layout {
                size: bitcode::align_up(offset, alignment),
                alignment,
                member_offsets,
            }
        }
    })
}
