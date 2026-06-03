#[repr(u8)]
pub enum Flag {
    C = 1 << 0, // Carry
    Z = 1 << 1, // Zero
    I = 1 << 2, // Interrupt disable
    D = 1 << 3, // Decimal (unused on NES)
    B = 1 << 4, // Break
    U = 1 << 5, // Unused
    V = 1 << 6, // Overflow
    N = 1 << 7, // Negative
}

pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub stack_ptr: u8,
    pub pc: u16,
    pub status: u8,

    fetched: u8,
    addr_abs: u16,
    addr_rel: u16,
    opcode: u8,
    cycles: u8,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            stack_ptr: 0,
            pc: 0,
            status: Flag::U as u8,
            fetched: 0,
            addr_abs: 0,
            addr_rel: 0,
            opcode: 0,
            cycles: 0,
        }
    }

    pub fn get_flag(&self, f: Flag) -> bool {
        self.status & (f as u8) != 0
    }

    pub fn set_flag(&mut self, f: Flag, v: bool) {
        let mask = f as u8;
        if v {
            self.status |= mask;
        } else {
            self.status &= !mask;
        }
    }
}
