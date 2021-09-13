/// Perform a privilege check, returning with an illegal operation interrupt and
/// a TryAgainError if it fails.
macro_rules! privileged {
    ($self:ident) => {{
        if !$self.kernel_mode {
            $self.interrupt_tx.send(INTERRUPT_ILLEGAL_OPERATION).unwrap();
            Err(CPUError::TryAgainError)
        } else {
            Ok(())
        }
    }}
}

/// Make flags from the result of an integer operation.
macro_rules! make_flags_int {
    ($ans:expr, $carry:expr, $overflow:expr) => {{
        let mut flags: u16 = 0;
        if $ans == 0 {
            flags |= FLAG_ZERO;
        } else if $ans < 0 {
            flags |= FLAG_NEGATIVE;
        }
        if $carry {
            flags |= FLAG_CARRY;
        }
        if $overflow {
            flags |= FLAG_OVERFLOW;
        }
        flags
    }}
}

/// Make flags from the result of a floating point operation.
macro_rules! make_flags_float {
    ($ans:expr) => {{
        if $ans == 0.0 {
            FLAG_ZERO
        } else if $ans < 0.0 {
            FLAG_NEGATIVE
        } else {
            0
        }
    }}
}

/// Create a binary operation that works as both signed and unsigned.
macro_rules! bin_op_multisigned {
    ($self:expr, $reg_ref:expr, $value:expr, $int_op:ident, $float_op:ident) => {{
        let flags: u16;
        match $self.read_from_register($reg_ref)? {
            TypedValue::Byte(x) => {
                let y = u8::try_from($value).unwrap();
                let u_ans = x.$int_op(y);
                let s_ans = (x as i8).$int_op(y as i8);
                debug_assert_eq!(u_ans.0, s_ans.0 as u8);
                $self.write_to_register($reg_ref, TypedValue::Byte(u_ans.0))?;
                flags = make_flags_int!(s_ans.0, u_ans.1, s_ans.1);
            },
            TypedValue::Half(x) => {
                let y = u16::try_from($value).unwrap();
                let u_ans = x.$int_op(y);
                let s_ans = (x as i16).$int_op(y as i16);
                debug_assert_eq!(u_ans.0, s_ans.0 as u16);
                $self.write_to_register($reg_ref, TypedValue::Half(u_ans.0))?;
                flags = make_flags_int!(s_ans.0, u_ans.1, s_ans.1);
            },
            TypedValue::Word(x) => {
                let y = u32::try_from($value).unwrap();
                let u_ans = x.$int_op(y);
                let s_ans = (x as i32).$int_op(y as i32);
                debug_assert_eq!(u_ans.0, s_ans.0 as u32);
                $self.write_to_register($reg_ref, TypedValue::Word(u_ans.0))?;
                flags = make_flags_int!(s_ans.0, u_ans.1, s_ans.1);
            },
            TypedValue::Float(x) => {
                let y = f32::try_from($value).unwrap();
                let ans = x.$float_op(y);
                $self.write_to_register($reg_ref, TypedValue::Float(ans))?;
                flags = make_flags_float!(ans);
            },
        }
        $self.flags = flags;
        Ok(())
    }}
}

/// Create an unsigned binary operation.
macro_rules! bin_op_unsigned {
    ($self:expr, $reg_ref:expr, $value:expr, $op:ident) => {{
        let flags: u16;
        match $self.read_from_register($reg_ref)? {
            TypedValue::Byte(x) => {
                let y = u8::try_from($value).unwrap();
                let ans = x.$op(y);
                $self.write_to_register($reg_ref, TypedValue::Byte(ans.0))?;
                flags = make_flags_int!(ans.0 as i8, ans.1, false);
            },
            TypedValue::Half(x) => {
                let y = u16::try_from($value).unwrap();
                let ans = x.$op(y);
                $self.write_to_register($reg_ref, TypedValue::Half(ans.0))?;
                flags = make_flags_int!(ans.0 as i16, ans.1, false);
            },
            TypedValue::Word(x) => {
                let y = u32::try_from($value).unwrap();
                let ans = x.$op(y);
                $self.write_to_register($reg_ref, TypedValue::Word(ans.0))?;
                flags = make_flags_int!(ans.0 as i32, ans.1, false);
            },
            TypedValue::Float(_) => {
                unreachable!()
            },
        }
        $self.flags = flags;
        Ok(())
    }}
}

/// Create a signed binary operation.
macro_rules! bin_op_signed {
    ($self:expr, $reg_ref:expr, $value:expr, $int_op:ident, $float_op:ident) => {{
        let flags: u16;
        match $self.read_from_register($reg_ref)? {
            TypedValue::Byte(x) => {
                let y = u8::try_from($value).unwrap();
                let ans = (x as i8).$int_op(y as i8);
                $self.write_to_register($reg_ref, TypedValue::Byte(ans.0 as u8))?;
                flags = make_flags_int!(ans.0, false, ans.1);
            },
            TypedValue::Half(x) => {
                let y = u16::try_from($value).unwrap();
                let ans = (x as i16).$int_op(y as i16);
                $self.write_to_register($reg_ref, TypedValue::Half(ans.0 as u16))?;
                flags = make_flags_int!(ans.0, false, ans.1);
            },
            TypedValue::Word(x) => {
                let y = u32::try_from($value).unwrap();
                let ans = (x as i32).$int_op(y as i32);
                $self.write_to_register($reg_ref, TypedValue::Word(ans.0 as u32))?;
                flags = make_flags_int!(ans.0, false, ans.1);
            },
            TypedValue::Float(x) => {
                let y = f32::try_from($value).unwrap();
                let ans = x.$float_op(y);
                $self.write_to_register($reg_ref, TypedValue::Float(ans))?;
                flags = make_flags_float!(ans);
            },
        }
        $self.flags = flags;
        Ok(())
    }}
}

/// Create a bitwise binary operation.
macro_rules! bin_op_bitwise {
    ($self:expr, $reg_ref:expr, $value:expr, $op:ident) => {{
        let flags: u16;
        match $self.read_from_register($reg_ref)? {
            TypedValue::Byte(x) => {
                let y = u8::try_from($value).unwrap();
                let ans = x.$op(y);
                $self.write_to_register($reg_ref, TypedValue::Byte(ans))?;
                flags = make_flags_int!(ans as i8, false, false);
            },
            TypedValue::Half(x) => {
                let y = u16::try_from($value).unwrap();
                let ans = x.$op(y);
                $self.write_to_register($reg_ref, TypedValue::Half(ans))?;
                flags = make_flags_int!(ans as i16, false, false);
            },
            TypedValue::Word(x) => {
                let y = u32::try_from($value).unwrap();
                let ans = x.$op(y);
                $self.write_to_register($reg_ref, TypedValue::Word(ans))?;
                flags = make_flags_int!(ans as i32, false, false);
            },
            TypedValue::Float(_) => {
                unreachable!()
            },
        }
        $self.flags = flags;
        Ok(())
    }}
}

/// Create a bit rotation operation.
macro_rules! bin_op_rotate {
    ($self:expr, $reg_ref:expr, $value:expr, $op:ident) => {{
        let flags: u16;
        match $self.read_from_register($reg_ref)? {
            TypedValue::Byte(x) => {
                let ans = x.$op($value);
                $self.write_to_register($reg_ref, TypedValue::Byte(ans))?;
                flags = make_flags_int!(ans as i8, false, false);
            },
            TypedValue::Half(x) => {
                let ans = x.$op($value);
                $self.write_to_register($reg_ref, TypedValue::Half(ans))?;
                flags = make_flags_int!(ans as i16, false, false);
            },
            TypedValue::Word(x) => {
                let ans = x.$op($value);
                $self.write_to_register($reg_ref, TypedValue::Word(ans))?;
                flags = make_flags_int!(ans as i32, false, false);
            },
            TypedValue::Float(_) => {
                unreachable!()
            },
        }
        $self.flags = flags;
        Ok(())
    }}
}

/// Create a bit rotation operation with carry.
macro_rules! bin_op_rotate_carry {
    ($self:expr, $reg_ref:expr, $value:expr, $op:ident) => {{
        let flags: u16;
        match $self.read_from_register($reg_ref)? {
            TypedValue::Byte(x) => {
                let mut ans = x;
                let mut carry = $self.flags & FLAG_CARRY > 0;
                for _ in 0..$value {
                    let result = ans.$op(carry);
                    ans = result.0;
                    carry = result.1;
                }
                $self.write_to_register($reg_ref, TypedValue::Byte(ans))?;
                flags = make_flags_int!(ans as i8, carry, false);
            },
            TypedValue::Half(x) => {
                let mut ans = x;
                let mut carry = $self.flags & FLAG_CARRY > 0;
                for _ in 0..$value {
                    let result = ans.$op(carry);
                    ans = result.0;
                    carry = result.1;
                }
                $self.write_to_register($reg_ref, TypedValue::Half(ans))?;
                flags = make_flags_int!(ans as i16, carry, false);
            },
            TypedValue::Word(x) => {
                let mut ans = x;
                let mut carry = $self.flags & FLAG_CARRY > 0;
                for _ in 0..$value {
                    let result = ans.$op(carry);
                    ans = result.0;
                    carry = result.1;
                }
                $self.write_to_register($reg_ref, TypedValue::Word(ans))?;
                flags = make_flags_int!(ans as i32, carry, false);
            },
            TypedValue::Float(_) => {
                unreachable!()
            },
        }
        $self.flags = flags;
        Ok(())
    }}
}

/// Create a conditional jump to literal opcode.
macro_rules! cond_jump_literal {
    ($self:ident, $condition:expr) => {{
        debug!("{} literal", $condition.0);
        let address = fetch!(Word);
        if $condition.1 {
            debug!("Jumping to {:#x}", address);
            $self.program_counter = address;
        }
    }}
}

/// Create a conditional jump to reference opcode.
macro_rules! cond_jump_reference {
    ($self:ident, $condition:expr) => {{
        debug!("{} ref", $condition.0);
        let reg_ref = fetch!(Byte);
        debug!("{} to address in {:#x}", $condition.0, reg_ref);
        let address = try_tv_into_v!($self.read_from_register(reg_ref)?);
        if $condition.1 {
            debug!("Jumping to {:#x}", address);
            $self.program_counter = address;
        }
    }}
}

// Conditional jump definitions.
macro_rules! jequal {
    ($self:ident) => { ("JEQUAL", $self.flags & FLAG_ZERO != 0) }
}

macro_rules! jnotequal {
    ($self:ident) => { ("JNOTEQUAL", $self.flags & FLAG_ZERO == 0) }
}

macro_rules! sjgreater {
    ($self:ident) => { ("SJGREATER", ($self.flags & FLAG_ZERO == 0)
        && ($self.flags & FLAG_NEGATIVE != 0) == ($self.flags & FLAG_OVERFLOW != 0)) }
}

macro_rules! sjgreatereq {
    ($self:ident) => { ("SJGREATEREQ", ($self.flags & FLAG_ZERO != 0)
        || ($self.flags & FLAG_NEGATIVE != 0) == ($self.flags & FLAG_OVERFLOW != 0)) }
}

macro_rules! ujgreater {
    ($self:ident) => { ("UJGREATER", ($self.flags & FLAG_CARRY == 0)
        && ($self.flags & FLAG_ZERO == 0)) }
}

macro_rules! ujgreatereq {
    ($self:ident) => { ("UJGREATEREQ", ($self.flags & FLAG_CARRY == 0)
        || ($self.flags & FLAG_ZERO != 0)) }
}

macro_rules! sjlesser {
    ($self:ident) => { ("SJLESSER",
        ($self.flags & FLAG_NEGATIVE != 0) != ($self.flags & FLAG_OVERFLOW != 0)) }
}

macro_rules! sjlessereq {
    ($self:ident) => { ("SJLESSEREQ", ($self.flags & FLAG_ZERO != 0)
        || ($self.flags & FLAG_NEGATIVE != 0) != ($self.flags & FLAG_OVERFLOW != 0)) }
}

macro_rules! ujlesser {
    ($self:ident) => { ("UJLESSER", ($self.flags & FLAG_CARRY != 0)) }
}

macro_rules! ujlessereq {
    ($self:ident) => { ("UJLESSEREQ", ($self.flags & FLAG_CARRY != 0)
        || ($self.flags & FLAG_ZERO != 0)) }
}
