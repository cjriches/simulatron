// These macros use the addressing mode shorthand from the assembly language
// documentation, which is case-sensitive. Thus, we have non-conventional
// macro names.
#![allow(non_snake_case)]

/// Shortcut for checking number of operands.
macro_rules! num_operands {
    ($num:expr, $ops:expr, $span:expr) => {{
        if $ops.len() != $num {
            return Err(SaltError {
                span: $span,
                message: format!("Expected {} operands, but found {}.",
                                    $num, $ops.len()).into(),
            });
        }
    }}
}

/// Shortcut for disallowing SymbolReference resolutions.
macro_rules! no_literals {
    ($span:expr) => {{
        return Err(SaltError {
            span: $span,
            message: "Cannot use a literal here.".into(),
        });
    }}
}

/// Shortcut for disallowing SymbolReference resolutions.
macro_rules! no_symbols {
    ($span:expr) => {{
        return Err(SaltError {
            span: $span,
            message: "Symbol references resolve to addresses, which can't be \
                      used here.".into(),
        });
    }}
}

/// Shortcut for an operand that must be any register reference.
macro_rules! reg_ref_any {
    ($self:ident, $resolved:expr) => {{
        match $resolved.0 {
            ResolvedOperand::Literal(_) => no_literals!($resolved.1),
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                $self.code.push(reg_ref);
                reg_type
            },
            ResolvedOperand::SymbolReference => no_symbols!($resolved.1),
        }
    }}
}

/// Shortcut for an operand that must be an address.
/// Only applicable to two-operand instructions.
macro_rules! address {
    ($self:ident, $resolved:expr, $opcodes:expr, $opcode_pos:expr) => {{
        match $resolved.0 {
            ResolvedOperand::Literal(literal) => {
                $self.code[$opcode_pos] = $opcodes.0;
                $self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: $resolved.1,
                        message: "Expected an address (word) \
                                  register reference.".into(),
                    });
                }
                $self.code[$opcode_pos] = $opcodes.1;
                $self.code.push(reg_ref);
            },
            ResolvedOperand::SymbolReference => {
                $self.code[$opcode_pos] = $opcodes.0;
            }
        }
    }}
}

/// An instruction with no operands.
macro_rules! i_none {
    ($self:ident, $opcode:expr, $operands:expr, $span:expr) => {{
        num_operands!(0, $operands, $span);
        $self.code.push($opcode);
        Ok(())
    }}
}

/// An instruction with a single ..w. operand.
macro_rules! i_w {
    ($self:ident, $opcodes:expr, $operands:expr, $span:expr) => {{
        num_operands!(1, $operands, $span);

        let (resolved, op_span) = $self.resolve_operand(&$operands[0])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code.push($opcodes.0);
                $self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "Expected a word register reference.".into(),
                    });
                }
                $self.code.push($opcodes.1);
                $self.code.push(reg_ref);
            }
            ResolvedOperand::SymbolReference => no_symbols!(op_span),
        }

        Ok(())
    }}
}

/// An instruction with operands BHWF ..a.
macro_rules! i_BHWF_a {
    ($self:ident, $opcodes:expr, $operands:expr, $span:expr) => {{
        num_operands!(2, $operands, $span);

        // Push placeholder opcode.
        let opcode_pos = $self.code.len();
        $self.code.push(0);

        // First operand: RegRefAny.
        let resolved = $self.resolve_operand(&$operands[0])?;
        reg_ref_any!($self, resolved);

        // Second operand: address.
        let resolved = $self.resolve_operand(&$operands[1])?;
        address!($self, resolved, $opcodes, opcode_pos);

        Ok(())
    }}
}

/// An instruction with operands ..a. BHWF
macro_rules! i_a_BHWF {
    ($self:ident, $opcodes:expr, $operands:expr, $span:expr) => {{
        num_operands!(2, $operands, $span);

        // Push placeholder opcode.
        let opcode_pos = $self.code.len();
        $self.code.push(0);

        // First operand: address.
        let resolved = $self.resolve_operand(&$operands[0])?;
        address!($self, resolved, $opcodes, opcode_pos);

        // Second operand: RegRefAny.
        let resolved = $self.resolve_operand(&$operands[1])?;
        reg_ref_any!($self, resolved);

        Ok(())
    }}
}

/// An instruction with operands BHWF bhwf
macro_rules! i_BHWF_bhwf {
    ($self:ident, $opcodes:expr, $operands:expr, $span:expr) => {{
        num_operands!(2, $operands, $span);

        // Push placeholder opcode.
        let opcode_pos = $self.code.len();
        $self.code.push(0);

        // First operand: destination register.
        let resolved = $self.resolve_operand(&$operands[0])?;
        let reg_type = reg_ref_any!($self, resolved);

        // Second operand: source value or register.
        let (resolved, op_span) = $self.resolve_operand(&$operands[1])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code[opcode_pos] = $opcodes.0;
                $self.push_value_as_reg_type(&literal, reg_type, op_span)?;
            },
            ResolvedOperand::RegRef(reg_ref, reg_type_2) => {
                if reg_type != reg_type_2 {
                    return Err(SaltError {
                        span: op_span,
                        message: "Cannot operate between differently-sized \
                                  registers.".into(),
                    });
                }
                $self.code[opcode_pos] = $opcodes.1;
                $self.code.push(reg_ref);
            },
            ResolvedOperand::SymbolReference => no_symbols!(op_span),
        }

        Ok(())
    }}
}

/// An instruction a single BHWF operand.
macro_rules! i_BHWF {
    ($self:ident, $opcode:expr, $operands:expr, $span:expr) => {{
        num_operands!(1, $operands, $span);
        $self.code.push($opcode);
        let resolved = $self.resolve_operand(&$operands[0])?;
        reg_ref_any!($self, resolved);
        Ok(())
    }}
}

/// An instruction with operands ..w. ..a. ..a.
macro_rules! i_w_a_a {
    ($self:ident, $opcodes:expr, $operands:expr, $span:expr) => {{
        num_operands!(3, $operands, $span);

        // Push placeholder opcode.
        let opcode_pos = $self.code.len();
        $self.code.push(0);
        let mut opcode_choice = 0;

        // First operand: word.
        let (resolved, op_span) = $self.resolve_operand(&$operands[0])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "Expected a word register reference.".into(),
                    });
                }
                opcode_choice += 4;
                $self.code.push(reg_ref);
            }
            ResolvedOperand::SymbolReference => no_symbols!(op_span),
        }

        // Second operand: address.
        let (resolved, op_span) = $self.resolve_operand(&$operands[1])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "Expected an address (word) \
                                  register reference.".into(),
                    });
                }
                opcode_choice += 2;
                $self.code.push(reg_ref);
            },
            ResolvedOperand::SymbolReference => {},
        }

        // Third operand: address.
        let (resolved, op_span) = $self.resolve_operand(&$operands[2])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "Expected an address (word) \
                                  register reference.".into(),
                    });
                }
                opcode_choice += 1;
                $self.code.push(reg_ref);
            },
            ResolvedOperand::SymbolReference => {},
        }

        let opcode = match opcode_choice {
            0 => $opcodes.0,
            1 => $opcodes.1,
            2 => $opcodes.2,
            3 => $opcodes.3,
            4 => $opcodes.4,
            5 => $opcodes.5,
            6 => $opcodes.6,
            7 => $opcodes.7,
            _ => unreachable!(),
        };
        $self.code[opcode_pos] = opcode;

        Ok(())
    }}
}

/// An instruction with operands ..w. ..a. ..b.
macro_rules! i_w_a_b {
    ($self:ident, $opcodes:expr, $operands:expr, $span:expr) => {{
        num_operands!(3, $operands, $span);

        // Push placeholder opcode.
        let opcode_pos = $self.code.len();
        $self.code.push(0);
        let mut opcode_choice = 0;

        // First operand: word.
        let (resolved, op_span) = $self.resolve_operand(&$operands[0])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "Expected a word register reference.".into(),
                    });
                }
                opcode_choice += 4;
                $self.code.push(reg_ref);
            }
            ResolvedOperand::SymbolReference => no_symbols!(op_span),
        }

        // Second operand: address.
        let (resolved, op_span) = $self.resolve_operand(&$operands[1])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code.append(&mut value_as_word(&literal).unwrap());
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefWord) {
                    return Err(SaltError {
                        span: op_span,
                        message: "Expected an address (word) \
                                  register reference.".into(),
                    });
                }
                opcode_choice += 2;
                $self.code.push(reg_ref);
            },
            ResolvedOperand::SymbolReference => {},
        }

        // Third operand: byte value.
        let (resolved, op_span) = $self.resolve_operand(&$operands[2])?;
        match resolved {
            ResolvedOperand::Literal(literal) => {
                $self.code.append(&mut value_as_byte(&literal)
                    .ok_or_else(|| SaltError {
                        span: op_span,
                        message: "Literal too large: expected single byte.".into(),
                })?);
            },
            ResolvedOperand::RegRef(reg_ref, reg_type) => {
                if !register_type_matches(reg_type, RegRefType::RegRefByte) {
                    return Err(SaltError {
                        span: op_span,
                        message: "Expected a byte register reference.".into(),
                    });
                }
                opcode_choice += 1;
                $self.code.push(reg_ref);
            }
            ResolvedOperand::SymbolReference => no_symbols!(op_span),
        }

        let opcode = match opcode_choice {
            0 => $opcodes.0,
            1 => $opcodes.1,
            2 => $opcodes.2,
            3 => $opcodes.3,
            4 => $opcodes.4,
            5 => $opcodes.5,
            6 => $opcodes.6,
            7 => $opcodes.7,
            _ => unreachable!(),
        };
        $self.code[opcode_pos] = opcode;

        Ok(())
    }}
}
