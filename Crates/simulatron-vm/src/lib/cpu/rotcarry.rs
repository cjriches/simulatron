// Rotate left with carry.
pub trait Rcl {
    type Output;
    fn rcl(self, carry: bool) -> (Self::Output, bool);
}

impl Rcl for u8 {
    type Output = u8;
    fn rcl(self, carry: bool) -> (Self::Output, bool) {
        let new_carry = (self as i8) < 0;
        let new_self = (self << 1) | carry as u8;
        (new_self, new_carry)
    }
}

impl Rcl for u16 {
    type Output = u16;
    fn rcl(self, carry: bool) -> (Self::Output, bool) {
        let new_carry = (self as i16) < 0;
        let new_self = (self << 1) | carry as u16;
        (new_self, new_carry)
    }
}

impl Rcl for u32 {
    type Output = u32;
    fn rcl(self, carry: bool) -> (Self::Output, bool) {
        let new_carry = (self as i32) < 0;
        let new_self = (self << 1) | carry as u32;
        (new_self, new_carry)
    }
}

// Rotate right with carry.
pub trait Rcr {
    type Output;
    fn rcr(self, carry: bool) -> (Self::Output, bool);
}

impl Rcr for u8 {
    type Output = u8;
    fn rcr(self, carry: bool) -> (Self::Output, bool) {
        let new_carry = (self & 1) > 0;
        let new_self = (self >> 1) | if carry { 0x80 } else { 0 };
        (new_self, new_carry)
    }
}

impl Rcr for u16 {
    type Output = u16;
    fn rcr(self, carry: bool) -> (Self::Output, bool) {
        let new_carry = (self & 1) > 0;
        let new_self = (self >> 1) | if carry { 0x8000 } else { 0 };
        (new_self, new_carry)
    }
}

impl Rcr for u32 {
    type Output = u32;
    fn rcr(self, carry: bool) -> (Self::Output, bool) {
        let new_carry = (self & 1) > 0;
        let new_self = (self >> 1) | if carry { 0x80000000 } else { 0 };
        (new_self, new_carry)
    }
}
