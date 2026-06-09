use crate::bus::Bus;

const NMI_VECTOR: u16 = 0xFFFA;
const RESET_VECTOR: u16 = 0xFFFC;
const IRQ_VECTOR: u16 = 0xFFFE;

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

#[derive(Debug, Clone, Copy)]
pub struct Stack {
    ptr: u8,
}

impl Default for Stack {
    fn default() -> Self {
        Self { ptr: 0xfd }
    }
}

impl Stack {
    const BASE: u16 = 0x0100;

    fn reset(&mut self) {
        self.ptr = 0xfd;
    }

    pub fn push(&mut self, data: u8, bus: &mut Bus) {
        bus.write(Self::BASE + self.ptr as u16, data);
        self.ptr = self.ptr.wrapping_sub(1);
    }

    pub fn pop(&mut self, bus: &mut Bus) -> u8 {
        self.ptr = self.ptr.wrapping_add(1);
        bus.read(Self::BASE + self.ptr as u16)
    }
}

#[derive(Copy, Clone)]
pub struct Instruction {
    pub name: &'static str,
    pub operate: fn(&mut Cpu, &mut Bus) -> u8,
    pub addr_mode: fn(&mut Cpu, &mut Bus) -> u8,
    pub cycles: u8,
}

pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub pc: u16,
    pub status: u8,
    pub stack: Stack,

    fetched: u8,
    addr_abs: u16,
    addr_rel: u16,
    opcode: u8,
    cycles: u8,
    lookup: [Instruction; 256],
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            status: Flag::U as u8,
            fetched: 0,
            addr_abs: 0,
            addr_rel: 0,
            opcode: 0,
            cycles: 0,
            lookup: build_lookup(),
            pc: 0,
            stack: Default::default(),
        }
    }

    // The status register packs 8 independent flags into one byte. Rather than
    // scattering raw bit masks through every opcode, these two helpers let all
    // flag reads and writes go through a single named interface.
    pub fn get_flag(&self, f: Flag) -> bool {
        self.status & (f as u8) != 0
    }

    pub fn set_flag(&mut self, f: Flag, v: bool) -> &mut Self {
        let mask = f as u8;
        if v {
            self.status |= mask;
        } else {
            self.status &= !mask;
        }
        self
    }

    fn pc_read(&mut self, bus: &Bus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn pc_advance(&mut self) -> u16 {
        let prev = self.pc;
        self.pc = self.pc.wrapping_add(1);
        prev
    }

    // The 6502 has no defined power-on state. reset() is how the CPU gets its
    // bearings: it reads a start address from a known ROM location (the reset
    // vector at 0xFFFC) and jumps there. Every NES game begins here.
    pub fn reset(&mut self, bus: &mut Bus) {
        // 1. Read the 16-bit reset vector from 0xFFFC (lo) and 0xFFFD (hi), set pc to it
        // 2. Reset a, x, y to 0
        // 3. Set stack_ptr to 0xFD
        // 4. Set status to U flag only (all others clear)
        // 5. Clear addr_abs, addr_rel, fetched to 0
        // 6. Set cycles = 8 (reset takes time)

        self.pc = bus.read_u16(RESET_VECTOR);

        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.stack.reset();
        self.status = Flag::U as u8;
        self.addr_abs = 0;
        self.addr_rel = 0;
        self.fetched = 0;
        self.cycles = 8;
    }

    // Hardware devices (cartridges, mappers) signal the CPU by asserting the IRQ
    // line. The CPU saves its current position and status on the stack, then jumps
    // to a handler routine. Because it's maskable, games can suppress it during
    // critical sections by setting the I flag.
    pub fn irq(&mut self, bus: &mut Bus) {
        // Only execute if the I (interrupt disable) flag is clear
        //
        // 1. Push pc high byte to stack: write to 0x0100 + stack_ptr, then stack_ptr--
        // 2. Push pc low byte to stack: write to 0x0100 + stack_ptr, then stack_ptr--
        // 3. Set flags: B = 0, U = 1, I = 1
        // 4. Push status to stack: write to 0x0100 + stack_ptr, then stack_ptr--
        // 5. Read new pc from IRQ vector: lo from 0xFFFE, hi from 0xFFFF
        // 6. Set cycles = 7
        if self.get_flag(Flag::I) {
            return;
        }
        self.stack.push((self.pc >> 8) as u8, bus);
        self.stack.push((self.pc & 0xFF) as u8, bus);
        self.set_flag(Flag::B, false)
            .set_flag(Flag::U, true)
            .set_flag(Flag::I, true);

        self.stack.push(self.status, bus);
        self.pc = bus.read_u16(IRQ_VECTOR);
        self.cycles = 7;
    }

    // The PPU fires an NMI at the start of every vertical blank (~60 times/sec).
    // Because it cannot be masked by the I flag, it is the reliable heartbeat
    // games use to synchronize all CPU-side work — AI, input, sound — with the
    // screen. Miss it and the game tears or lags.
    pub fn nmi(&mut self, bus: &mut Bus) {
        // Same as irq() but: always fires (no I flag check), reads vector from 0xFFFA/0xFFFB,
        // and takes 8 cycles instead of 7
        //
        // 1. Push pc high byte to stack, stack_ptr--
        // 2. Push pc low byte to stack, stack_ptr--
        // 3. Set flags: B = 0, U = 1, I = 1
        // 4. Push status to stack, stack_ptr--
        // 5. Read new pc: lo from 0xFFFA, hi from 0xFFFB
        // 6. Set cycles = 8
        self.stack.push((self.pc >> 8) as u8, bus);
        self.stack.push((self.pc & 0xFF) as u8, bus);
        self.set_flag(Flag::B, false)
            .set_flag(Flag::U, true)
            .set_flag(Flag::I, true);

        self.stack.push(self.status, bus);
        self.pc = bus.read_u16(NMI_VECTOR);
        self.cycles = 8;
    }

    // The top-level driver of the CPU. The NES master clock calls this every
    // tick. Most instructions take 2–7 cycles; clock() executes the full
    // instruction in one shot but then idles for the remaining cycles so that
    // timing-sensitive devices (especially the PPU, which runs 3 cycles per CPU
    // cycle) stay in sync.
    pub fn clock(&mut self, bus: &mut Bus) {
        // If cycles == 0, the previous instruction is done — execute the next one:
        //   1. Set U flag
        //   2. Read opcode byte from pc, increment pc
        //   3. Set cycles = lookup[opcode].cycles
        //   4. Call the addressing mode fn, capture its return value (extra_cycle1)
        //   5. Call the opcode operate fn, capture its return value (extra_cycle2)
        //   6. cycles += extra_cycle1 & extra_cycle2  (& not +, both must agree)
        //   7. Set U flag again
        //
        // Always at the end (regardless of cycles): cycles--
        if self.cycles == 0 {
            self.set_flag(Flag::U, true);
            self.opcode = self.pc_read(bus);
            let instruction = self.lookup[self.opcode as usize];
            self.cycles = instruction.cycles;
            let addr_mode_cycles = (instruction.addr_mode)(self, bus);
            let operate_cycles = (instruction.operate)(self, bus);
            self.cycles += operate_cycles & addr_mode_cycles;
            self.set_flag(Flag::U, true);
        }
        self.cycles -= 1;
    }

    // Before an opcode does its arithmetic it needs one byte of input data.
    // fetch() provides that byte from wherever the addressing mode resolved:
    // either already in `fetched` (implied mode set it), or read from addr_abs.
    // Opcodes call this instead of reading memory directly so the implied-mode
    // special case stays in one place.
    fn fetch(&mut self, bus: &mut Bus) -> u8 {
        // If the current instruction's addr_mode is NOT imp, read from addr_abs into fetched.
        // If it IS imp, fetched was already set by imp() — don't read from memory.
        // Return fetched either way.
        //
        // Compare: self.lookup[self.opcode as usize].addr_mode != Cpu::imp
        if !std::ptr::fn_addr_eq(
            self.lookup[self.opcode as usize].addr_mode,
            Cpu::imp as fn(&mut Cpu, &mut Bus) -> u8,
        ) {
            self.fetched = bus.read(self.addr_abs)
        };
        self.fetched
    }

    fn set_n_z_flags(&mut self, last_result: u8) {
        self.set_flag(Flag::Z, if last_result == 0 { true } else { false });
        self.set_flag(Flag::N, if (last_result as i8) < 0 { true } else { false });
    }
}

impl Cpu {
    // Instructions like CLC or INX act on a register and need no memory address.
    // We still capture A into fetched so that accumulator-targeting instructions
    // (like ASL in accumulator mode) have a value to work with without special-casing.
    fn imp(&mut self, _bus: &mut Bus) -> u8 {
        // Capture accumulator into fetched (some implied instructions like PHA need it)
        // Return 0
        self.fetched = self.a;
        0
    }

    // The value to operate on is baked directly into the instruction — no memory
    // lookup needed. The cheapest and most common way to supply a constant.
    // e.g. LDA #$42 loads the literal value 0x42 into A.
    fn imm(&mut self, _bus: &mut Bus) -> u8 {
        // The operand is the next byte in the instruction stream.
        // Set addr_abs = pc, then pc++
        // Return 0
        self.addr_abs = self.pc_advance();
        0
    }

    // Zero page (0x0000–0x00FF) is the 6502's "fast RAM". A 1-byte address
    // instead of 2 means shorter, faster instructions. Games store their most
    // frequently accessed variables — counters, flags, pointers — here.
    fn zp0(&mut self, bus: &mut Bus) -> u8 {
        // Read one byte from pc as an 8-bit zero-page address, pc++
        // Mask to 0x00FF to stay in page zero
        // Set addr_abs, return 0
        self.addr_abs = self.pc_read(bus) as u16 & 0x00FF;
        0
    }

    // Zero page indexed: X acts as an array index into a small table in zero page.
    // The result wraps within zero page (never escapes 0x00FF), which is intentional
    // — it keeps the access fast and predictable.
    fn zpx(&mut self, bus: &mut Bus) -> u8 {
        // Same as zp0 but add x register to the address before masking
        // addr_abs = (read(pc) + x) & 0x00FF, pc++
        // Return 0
        self.addr_abs = (self.pc_read(bus) as u16 + self.x as u16) as u16 & 0x00FF;
        0
    }

    // Same idea as zpx but using Y. Mainly used with LDX/STX, which don't
    // support zpx — the instruction set isn't fully orthogonal.
    fn zpy(&mut self, bus: &mut Bus) -> u8 {
        // Same as zpx but add y register
        // addr_abs = (read(pc) + y) & 0x00FF, pc++
        // Return 0
        self.addr_abs = (self.pc_read(bus) as u16 + self.y as u16) as u16 & 0x00FF;
        0
    }

    // Used exclusively by branch instructions (BEQ, BNE, BCC, etc.). Instead of
    // a full 16-bit target address, a signed 8-bit offset keeps branch instructions
    // compact (2 bytes). The range of ±128 bytes is enough for nearly all loops
    // and conditionals.
    fn rel(&mut self, bus: &mut Bus) -> u8 {
        // Read one byte from pc as a signed relative offset, pc++
        // Store in addr_rel
        // If bit 7 is set (negative number), sign-extend: addr_rel |= 0xFF00
        // Return 0
        self.addr_rel = self.pc_read(bus) as i8 as u16;
        0
    }

    // The general-purpose addressing mode: reach any byte in the full 64KB space.
    // Most ROM reads, MMIO device accesses (PPU registers at 0x2000, APU at 0x4000),
    // and cartridge data use absolute addressing.
    fn abs(&mut self, bus: &mut Bus) -> u8 {
        // Read a full 16-bit address, little-endian:
        //   lo = read(pc), pc++
        //   hi = read(pc), pc++
        //   addr_abs = (hi << 8) | lo
        // Return 0
        let low = self.pc_read(bus) as u16;
        let high = self.pc_read(bus) as u16;
        self.addr_abs = (high << 8) | low;
        0
    }

    // The workhorse for iterating through arrays anywhere in memory: X is the
    // loop counter, the base address is the array start. Crossing a page boundary
    // (high byte of address changes) costs an extra cycle because the real hardware
    // needs an extra internal cycle to carry the addition.
    fn abx(&mut self, bus: &mut Bus) -> u8 {
        // Same as abs, then add x to addr_abs
        // If the addition changes the high byte (page boundary crossed), return 1
        // Otherwise return 0
        let low = self.pc_read(bus) as u16;
        let high = self.pc_read(bus) as u16;
        self.addr_abs = (high << 8) | low;

        let base_page = self.addr_abs & 0xFF00;
        self.addr_abs = self.addr_abs.wrapping_add(self.x as u16);

        if (self.addr_abs & 0xFF00) != base_page {
            1
        } else {
            0
        }
    }

    // Same as abx but uses Y. Commonly paired with izy: use a zero-page pointer
    // to find the base of an array, then aby/Y to walk through it.
    fn aby(&mut self, bus: &mut Bus) -> u8 {
        // Same as abx but add y instead of x
        let low = self.pc_read(bus) as u16;
        let high = self.pc_read(bus) as u16;
        self.addr_abs = (high << 8) | low;

        let base_page = self.addr_abs & 0xFF00;
        self.addr_abs = self.addr_abs.wrapping_add(self.y as u16);

        if (self.addr_abs & 0xFF00) != base_page {
            1
        } else {
            0
        }
    }

    // The 6502's only true pointer dereference — used almost exclusively by JMP
    // to implement jump tables and dynamic dispatch. Contains a famous hardware
    // bug: a pointer ending in 0xFF wraps its high-byte read within the same page
    // instead of crossing to the next. Some NES games depend on this bug.
    fn ind(&mut self, bus: &mut Bus) -> u8 {
        // Read a 16-bit pointer address from pc (little-endian), pc += 2
        // Then read the actual 16-bit address from that pointer
        //
        // Hardware bug: if the pointer's low byte is 0xFF, the high byte of the
        // target address wraps within the same page instead of crossing to the next:
        //   if ptr_lo == 0xFF:
        //     hi = read(ptr & 0xFF00)   <- wraps to start of same page
        //   else:
        //     hi = read(ptr + 1)        <- normal
        // Return 0

        let ptr_lo = self.pc_read(bus) as u16;
        let ptr_hi = self.pc_read(bus) as u16;
        let ptr = (ptr_hi << 8) | ptr_lo;

        let lo = bus.read(ptr) as u16;
        let hi = if ptr_lo == 0xFF {
            bus.read(ptr & 0xFF00) as u16 // hardware bug: wraps within same page
        } else {
            bus.read(ptr + 1) as u16
        };

        self.addr_abs = (hi << 8) | lo;
        0
    }

    // X selects a pointer from a table of 16-bit pointers stored in zero page,
    // then dereferences it. Useful when you have multiple data structures and need
    // to pick one by index — e.g. a table of enemy attribute pointers, select by X.
    fn izx(&mut self, bus: &mut Bus) -> u8 {
        // Read one byte from pc as a zero-page base address, pc++
        // Add x to get a zero-page pointer address (mask to 0x00FF)
        // Read the 16-bit actual address from that pointer (lo, hi — both masked to 0x00FF)
        // addr_abs = (hi << 8) | lo
        // Return 0
        let base = ((self.pc_read(bus) as u16) + self.x as u16) & 0x00FF;
        let low = bus.read(base) as u16;
        let high = bus.read((base + 1) & 0x00FF) as u16;
        self.addr_abs = (high << 8) | low;

        0
    }

    // Follows a fixed zero-page pointer to a base address, then uses Y as an
    // array offset. The natural pattern for "I have a pointer to a buffer, walk
    // through it with Y" — the most common indirect mode in NES games.
    fn izy(&mut self, bus: &mut Bus) -> u8 {
        // Read one byte from pc as a zero-page pointer address, pc++
        // Read 16-bit base address from zero page (lo at addr, hi at addr+1, both masked)
        // Add y to get the final addr_abs
        // If y addition crosses a page boundary, return 1; otherwise return 0
        let base = self.pc_read(bus) as u16;
        let low = bus.read(base) as u16;
        let high = bus.read((base + 1) & 0x00FF) as u16;
        let addr = (high << 8) | low;
        self.addr_abs = addr.wrapping_add(self.y as u16);
        if self.addr_abs & 0xFF00 != addr & 0xFF00 {
            1
        } else {
            0
        }
    }
}

// Opcodes — all stubbed, implemented in later steps
impl Cpu {
    fn xxx(&mut self, _bus: &mut Bus) -> u8 {
        0
    } // illegal/unknown opcode
    fn nop(&mut self, _bus: &mut Bus) -> u8 {
        0
    }
    fn adc(&mut self, bus: &mut Bus) -> u8 {
        // tmp = (A as u16) + (fetch() as u16) + (C as u16)
        // C = tmp > 0xFF
        // Z = (tmp & 0xFF) == 0
        // N = tmp & 0x80
        // V = (~(A ^ fetch()) & (A ^ tmp)) & 0x80  — set when both inputs have same sign but result has different sign
        // A = tmp & 0xFF
        // return 1
        let result = (self.a as u16) + (self.fetch(bus) as u16) + self.get_flag(Flag::C) as u16;
        self.set_flag(Flag::C, result > 0xFF)
            .set_flag(Flag::Z, result & 0xFF == 0)
            .set_flag(Flag::N, result & 0x80 != 0);
        let v = ((!(self.a as u16 ^ self.fetched as u16) & ((self.a as u16) ^ result)) & 0x80) > 0;
        self.set_flag(Flag::V, v);

        self.a = (result & 0xFF) as u8;
        1
    }
    fn and(&mut self, bus: &mut Bus) -> u8 {
        // A = A & fetch(); set N, Z; return 1
        self.a = self.a & self.fetch(bus);
        self.set_n_z_flags(self.a);
        1
    }
    fn asl(&mut self, bus: &mut Bus) -> u8 {
        // val = fetch()
        // C = val & 0x80  (old bit 7 shifts into carry)
        // val = val << 1
        // if imp mode: A = val; else write(addr_abs, val)
        // set N, Z; return 0
        let value = self.fetch(bus);
        let new_value = value << 1;
        self.set_flag(Flag::C, value & 0x80 > 0);
        if std::ptr::fn_addr_eq(
            self.lookup[self.opcode as usize].addr_mode,
            Cpu::imp as fn(&mut Cpu, &mut Bus) -> u8,
        ) {
            self.a = new_value;
        } else {
            bus.write(self.addr_abs, new_value);
        }
        self.set_n_z_flags(new_value);
        0
    }

    fn branch(&mut self, cond: bool) {
        if cond {
            self.cycles += 1;
            self.addr_abs = self.pc.wrapping_add(self.addr_rel);

            if self.addr_abs & 0xff00 != (self.pc & 0xff00) {
                self.cycles += 1;
            }

            self.pc = self.addr_abs;
        }
    }

    fn bcc(&mut self, _bus: &mut Bus) -> u8 {
        // branch if C == 0
        // if condition: cycles += 1; addr_abs = pc.wrapping_add(addr_rel);
        //   if (addr_abs & 0xFF00) != (pc & 0xFF00): cycles += 1  (page cross)
        //   pc = addr_abs
        // cycles are added directly here, not through the & mechanism; return 0
        self.branch(!self.get_flag(Flag::C));
        0
    }
    fn bcs(&mut self, _bus: &mut Bus) -> u8 {
        // branch if C == 1; same pattern as bcc
        self.branch(self.get_flag(Flag::C));
        0
    }
    fn beq(&mut self, _bus: &mut Bus) -> u8 {
        // branch if Z == 1; same pattern as bcc
        self.branch(self.get_flag(Flag::Z));
        0
    }
    fn bit(&mut self, bus: &mut Bus) -> u8 {
        // val = fetch()
        // Z = (A & val) == 0
        // N = val & 0x80   (bit 7 of memory value, NOT of A & val)
        // V = val & 0x40   (bit 6 of memory value)
        // return 0
        let value = self.fetch(bus);
        self.set_flag(Flag::Z, self.a & value == 0);
        self.set_flag(Flag::N, value & 0x80 > 0);
        self.set_flag(Flag::V, value & 0x40 > 0);
        0
    }
    fn bmi(&mut self, _bus: &mut Bus) -> u8 {
        // branch if N == 1; same pattern as bcc
        self.branch(self.get_flag(Flag::N));
        0
    }
    fn bne(&mut self, _bus: &mut Bus) -> u8 {
        // branch if Z == 0; same pattern as bcc
        self.branch(!self.get_flag(Flag::Z));
        0
    }
    fn bpl(&mut self, _bus: &mut Bus) -> u8 {
        // branch if N == 0; same pattern as bcc
        self.branch(!self.get_flag(Flag::N));
        0
    }
    fn brk(&mut self, bus: &mut Bus) -> u8 {
        // pc++
        // push hi(pc); push lo(pc)
        // set B = 1, U = 1
        // push status
        // set I = 1
        // pc = bus.read_u16(IRQ_VECTOR)
        // return 0
        self.pc = self.pc.wrapping_add(1);
        self.stack.push((self.pc >> 8) as u8, bus);
        self.stack.push(self.pc as u8, bus);
        self.set_flag(Flag::B, true).set_flag(Flag::U, true);
        self.stack.push(self.status, bus);
        self.set_flag(Flag::I, true);
        self.pc = bus.read_u16(IRQ_VECTOR);
        0
    }
    fn bvc(&mut self, _bus: &mut Bus) -> u8 {
        // branch if V == 0; same pattern as bcc
        self.branch(!self.get_flag(Flag::V));
        0
    }
    fn bvs(&mut self, _bus: &mut Bus) -> u8 {
        // branch if V == 1; same pattern as bcc
        self.branch(self.get_flag(Flag::V));
        0
    }
    fn clc(&mut self, _bus: &mut Bus) -> u8 {
        self.set_flag(Flag::C, false);
        0
    }
    fn cld(&mut self, _bus: &mut Bus) -> u8 {
        self.set_flag(Flag::D, false);
        0
    }
    fn cli(&mut self, _bus: &mut Bus) -> u8 {
        self.set_flag(Flag::I, false);
        0
    }
    fn clv(&mut self, _bus: &mut Bus) -> u8 {
        self.set_flag(Flag::V, false);
        0
    }
    fn cmp(&mut self, bus: &mut Bus) -> u8 {
        // tmp = (A as u16).wrapping_sub(fetch() as u16)
        // C = A >= fetched  (no borrow means carry set)
        // Z = (tmp & 0x00FF) == 0
        // N = tmp & 0x0080
        // return 1
        let tmp = (self.a as u16).wrapping_sub(self.fetch(bus) as u16);
        self.set_flag(Flag::C, self.a >= self.fetched)
            .set_flag(Flag::Z, tmp & 0x00ff == 0)
            .set_flag(Flag::N, tmp & 0x0080 != 0);
        1
    }
    fn cpx(&mut self, bus: &mut Bus) -> u8 {
        // same as cmp but compare X instead of A; return 0
        let tmp = (self.x as u16).wrapping_sub(self.fetch(bus) as u16);
        self.set_flag(Flag::C, self.x >= self.fetched)
            .set_flag(Flag::Z, tmp & 0x00ff == 0)
            .set_flag(Flag::N, tmp & 0x0080 != 0);
        0
    }
    fn cpy(&mut self, bus: &mut Bus) -> u8 {
        // same as cmp but compare Y instead of A; return 0
        let tmp = (self.y as u16).wrapping_sub(self.fetch(bus) as u16);
        self.set_flag(Flag::C, self.y >= self.fetched)
            .set_flag(Flag::Z, tmp & 0x00ff == 0)
            .set_flag(Flag::N, tmp & 0x0080 != 0);
        0
    }
    fn dec(&mut self, bus: &mut Bus) -> u8 {
        // val = read(addr_abs).wrapping_sub(1); write(addr_abs, val); set N, Z; return 0
        let value = bus.read(self.addr_abs).wrapping_sub(1);
        bus.write(self.addr_abs, value);
        self.set_n_z_flags(value);
        0
    }
    fn dex(&mut self, _bus: &mut Bus) -> u8 {
        // X = X.wrapping_sub(1); set N, Z; return 0
        self.x = self.x.wrapping_sub(1);
        self.set_n_z_flags(self.x);
        0
    }
    fn dey(&mut self, _bus: &mut Bus) -> u8 {
        // Y = Y.wrapping_sub(1); set N, Z; return 0
        self.y = self.y.wrapping_sub(1);
        self.set_n_z_flags(self.y);
        0
    }
    fn eor(&mut self, bus: &mut Bus) -> u8 {
        // A = A ^ fetch(); set N, Z; return 1
        self.a = self.a ^ self.fetch(bus);
        self.set_n_z_flags(self.a);
        1
    }
    fn inc(&mut self, bus: &mut Bus) -> u8 {
        // val = read(addr_abs).wrapping_add(1); write(addr_abs, val); set N, Z; return 0
        let value = bus.read(self.addr_abs).wrapping_add(1);
        bus.write(self.addr_abs, value);
        self.set_n_z_flags(value);
        0
    }
    fn inx(&mut self, _bus: &mut Bus) -> u8 {
        // X = X.wrapping_add(1); set N, Z; return 0
        self.x = self.x.wrapping_add(1);
        self.set_n_z_flags(self.x);
        0
    }
    fn iny(&mut self, _bus: &mut Bus) -> u8 {
        // Y = Y.wrapping_add(1); set N, Z; return 0
        self.y = self.y.wrapping_add(1);
        self.set_n_z_flags(self.y);
        0
    }
    fn jmp(&mut self, _bus: &mut Bus) -> u8 {
        // pc = addr_abs  (addressing mode already resolved the target — abs or ind)
        // return 0
        self.pc = self.addr_abs;
        0
    }
    fn jsr(&mut self, bus: &mut Bus) -> u8 {
        // push hi(pc - 1); push lo(pc - 1)   ← last byte of the JSR instruction
        // pc = addr_abs
        // return 0
        //
        // Note: by the time jsr() runs, pc already points one past the JSR instruction
        // (clock() advanced it past the opcode and abs() advanced it past the two address bytes).
        // The convention is to push pc - 1 so RTS can restore and then add 1 to land correctly.
        let pc = self.pc.wrapping_sub(1);
        self.stack.push((pc >> 8) as u8, bus);
        self.stack.push(pc as u8, bus);
        self.pc = self.addr_abs;
        0
    }
    fn lda(&mut self, bus: &mut Bus) -> u8 {
        // A = fetch(); set N, Z; return 1
        self.a = self.fetch(bus);
        self.set_flag(Flag::Z, if self.a == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.a as i8) < 0 { true } else { false });
        1
    }
    fn ldx(&mut self, bus: &mut Bus) -> u8 {
        // X = fetch(); set N, Z; return 1
        self.x = self.fetch(bus);
        self.set_flag(Flag::Z, if self.x == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.x as i8) < 0 { true } else { false });
        1
    }
    fn ldy(&mut self, bus: &mut Bus) -> u8 {
        // Y = fetch(); set N, Z; return 1
        self.y = self.fetch(bus);
        self.set_flag(Flag::Z, if self.y == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.y as i8) < 0 { true } else { false });
        1
    }
    fn lsr(&mut self, bus: &mut Bus) -> u8 {
        // val = fetch()
        // C = val & 0x01  (old bit 0 shifts into carry)
        // val = val >> 1   (N is always 0 after LSR — bit 7 becomes 0)
        // if imp mode: A = val; else write(addr_abs, val)
        // set N (always 0), Z; return 0
        let value = self.fetch(bus);
        let new_value = value >> 1;
        self.set_flag(Flag::C, value & 0x01 > 0);
        if std::ptr::fn_addr_eq(
            self.lookup[self.opcode as usize].addr_mode,
            Cpu::imp as fn(&mut Cpu, &mut Bus) -> u8,
        ) {
            self.a = new_value;
        } else {
            bus.write(self.addr_abs, new_value);
        }
        self.set_n_z_flags(new_value);
        0
    }
    fn ora(&mut self, bus: &mut Bus) -> u8 {
        // A = A | fetch(); set N, Z; return 1
        self.a = self.a | self.fetch(bus);
        self.set_n_z_flags(self.a);
        1
    }
    fn pha(&mut self, bus: &mut Bus) -> u8 {
        // stack.push(A); no flags; return 0
        self.stack.push(self.a, bus);
        0
    }
    fn php(&mut self, bus: &mut Bus) -> u8 {
        // stack.push(status | B | U); no flags changed on cpu.status; return 0
        self.stack
            .push(self.status | Flag::B as u8 | Flag::U as u8, bus);
        0
    }
    fn pla(&mut self, bus: &mut Bus) -> u8 {
        // A = stack.pop(); set N, Z; return 0
        self.a = self.stack.pop(bus);
        self.set_flag(Flag::Z, if self.a == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.a as i8) < 0 { true } else { false });
        0
    }
    fn plp(&mut self, bus: &mut Bus) -> u8 {
        // status = stack.pop(); then clear B, set U; return 0
        self.status = self.stack.pop(bus);
        self.set_flag(Flag::U, true);
        self.set_flag(Flag::B, false);
        0
    }
    fn rol(&mut self, bus: &mut Bus) -> u8 {
        // val = fetch()
        // new_val = (val << 1) | C   (old carry rotates into bit 0)
        // C = val & 0x80              (old bit 7 rotates into carry)
        // if imp mode: A = new_val; else write(addr_abs, new_val)
        // set N, Z; return 0
        let value = self.fetch(bus);
        let new_value = (value << 1) | self.get_flag(Flag::C) as u8;
        self.set_flag(Flag::C, value & 0x80 > 0);
        if std::ptr::fn_addr_eq(
            self.lookup[self.opcode as usize].addr_mode,
            Cpu::imp as fn(&mut Cpu, &mut Bus) -> u8,
        ) {
            self.a = new_value;
        } else {
            bus.write(self.addr_abs, new_value);
        }
        self.set_n_z_flags(new_value);
        0
    }
    fn ror(&mut self, bus: &mut Bus) -> u8 {
        // val = fetch()
        // new_val = (val >> 1) | (C << 7)  (old carry rotates into bit 7)
        // C = val & 0x01                    (old bit 0 rotates into carry)
        // if imp mode: A = new_val; else write(addr_abs, new_val)
        // set N, Z; return 0
        let value = self.fetch(bus);
        let new_value = (value >> 1) | ((self.get_flag(Flag::C) as u8) << 7);
        self.set_flag(Flag::C, value & 0x01 > 0);
        if std::ptr::fn_addr_eq(
            self.lookup[self.opcode as usize].addr_mode,
            Cpu::imp as fn(&mut Cpu, &mut Bus) -> u8,
        ) {
            self.a = new_value;
        } else {
            bus.write(self.addr_abs, new_value);
        }
        self.set_n_z_flags(new_value);
        0
    }
    fn rti(&mut self, bus: &mut Bus) -> u8 {
        // status = stack.pop(); clear B, set U
        // lo = stack.pop(); hi = stack.pop()
        // pc = (hi << 8) | lo            ← no +1, interrupt saved the exact return address
        // return 0
        self.status = self.stack.pop(bus);
        self.set_flag(Flag::B, false);
        self.set_flag(Flag::U, true);
        let lo = self.stack.pop(bus) as u16;
        let hi = (self.stack.pop(bus) as u16) << 8;
        self.pc = hi | lo;
        0
    }
    fn rts(&mut self, bus: &mut Bus) -> u8 {
        // lo = stack.pop(); hi = stack.pop()
        // pc = ((hi << 8) | lo) + 1      ← +1 because JSR pushed pc - 1
        // return 0
        let lo = self.stack.pop(bus);
        let hi = self.stack.pop(bus);
        self.pc = ((hi as u16) << 8 | lo as u16) + 1;
        0
    }
    fn sbc(&mut self, bus: &mut Bus) -> u8 {
        // SBC is ADC with the operand bit-flipped:
        // tmp = (A as u16) + (fetch() ^ 0xFF) as u16 + (C as u16)
        // same flag logic as ADC (C, Z, N, V), same V formula
        // A = tmp & 0xFF
        // return 1
        let result =
            (self.a as u16) + (self.fetch(bus) ^ 0xff) as u16 + self.get_flag(Flag::C) as u16;

        self.set_flag(Flag::C, result > 0xFF)
            .set_flag(Flag::Z, result & 0xFF == 0)
            .set_flag(Flag::N, result & 0x80 != 0);
        let v = ((!(self.a as u16 ^ (self.fetched ^ 0xff) as u16) & ((self.a as u16) ^ result))
            & 0x80)
            > 0;
        self.set_flag(Flag::V, v);
        self.a = (result & 0xff) as u8;
        1
    }
    fn sec(&mut self, _bus: &mut Bus) -> u8 {
        self.set_flag(Flag::C, true);
        0
    }
    fn sed(&mut self, _bus: &mut Bus) -> u8 {
        self.set_flag(Flag::D, true);
        0
    }
    fn sei(&mut self, _bus: &mut Bus) -> u8 {
        // I = 1
        self.set_flag(Flag::I, true);
        0
    }
    fn sta(&mut self, bus: &mut Bus) -> u8 {
        // write(addr_abs, A); no flags; return 0
        bus.write(self.addr_abs, self.a);
        0
    }
    fn stx(&mut self, bus: &mut Bus) -> u8 {
        // write(addr_abs, X); no flags; return 0
        bus.write(self.addr_abs, self.x);
        0
    }
    fn sty(&mut self, bus: &mut Bus) -> u8 {
        // write(addr_abs, Y); no flags; return 0
        bus.write(self.addr_abs, self.y);
        0
    }
    fn tax(&mut self, _bus: &mut Bus) -> u8 {
        // X = A; set N, Z; return 0
        self.x = self.a;
        self.set_flag(Flag::Z, if self.x == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.x as i8) < 0 { true } else { false });
        0
    }
    fn tay(&mut self, _bus: &mut Bus) -> u8 {
        // Y = A; set N, Z; return 0
        self.y = self.a;
        self.set_flag(Flag::Z, if self.y == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.y as i8) < 0 { true } else { false });
        0
    }
    fn tsx(&mut self, _bus: &mut Bus) -> u8 {
        // X = stack.ptr; set N, Z; return 0
        self.x = self.stack.ptr;
        self.set_flag(Flag::Z, if self.x == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.x as i8) < 0 { true } else { false });
        0
    }
    fn txa(&mut self, _bus: &mut Bus) -> u8 {
        // A = X; set N, Z; return 0
        self.a = self.x;
        self.set_flag(Flag::Z, if self.a == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.a as i8) < 0 { true } else { false });
        0
    }
    fn txs(&mut self, _bus: &mut Bus) -> u8 {
        // stack.ptr = X; no flags; return 0
        self.stack.ptr = self.x;
        0
    }
    fn tya(&mut self, _bus: &mut Bus) -> u8 {
        // A = Y; set N, Z; return 0
        self.a = self.y;
        self.set_flag(Flag::Z, if self.a == 0 { true } else { false });
        self.set_flag(Flag::N, if (self.a as i8) < 0 { true } else { false });
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;

    fn make() -> (Cpu, Bus) {
        (Cpu::new(), Bus::new())
    }

    fn setup(pc: u16, mem: &[(u16, u8)]) -> (Cpu, Bus) {
        let (mut cpu, mut bus) = make();
        cpu.pc = pc;
        for &(addr, val) in mem {
            bus.write(addr, val);
        }
        (cpu, bus)
    }

    // --- reset ---

    #[test]
    fn reset_loads_pc_from_reset_vector() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        assert_eq!(cpu.pc, 0x8000);
    }

    #[test]
    fn reset_clears_registers() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0xFF;
        cpu.x = 0xFF;
        cpu.y = 0xFF;
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        assert_eq!(cpu.a, 0);
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.y, 0);
    }

    #[test]
    fn reset_sets_stack_ptr_to_fd() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        assert_eq!(cpu.stack.ptr, 0xFD);
    }

    #[test]
    fn reset_sets_u_flag_only() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        assert_eq!(cpu.status, Flag::U as u8);
    }

    #[test]
    fn reset_takes_8_cycles() {
        let (mut cpu, mut bus) = make();
        cpu.reset(&mut bus);
        assert_eq!(cpu.cycles, 8);
    }

    // --- irq ---

    #[test]
    fn irq_does_nothing_when_interrupt_disabled() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        cpu.set_flag(Flag::I, true);
        let pc_before = cpu.pc;
        let sp_before = cpu.stack.ptr;
        cpu.irq(&mut bus);
        assert_eq!(cpu.pc, pc_before);
        assert_eq!(cpu.stack.ptr, sp_before);
    }

    #[test]
    fn irq_jumps_to_irq_vector() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        cpu.set_flag(Flag::I, false);
        bus.write(0xFFFE, 0x00);
        bus.write(0xFFFF, 0x90);
        cpu.irq(&mut bus);
        assert_eq!(cpu.pc, 0x9000);
    }

    #[test]
    fn irq_pushes_three_bytes_to_stack() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        cpu.set_flag(Flag::I, false);
        let sp_before = cpu.stack.ptr;
        cpu.irq(&mut bus);
        assert_eq!(cpu.stack.ptr, sp_before.wrapping_sub(3));
    }

    #[test]
    fn irq_takes_7_cycles() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        cpu.set_flag(Flag::I, false);
        cpu.irq(&mut bus);
        assert_eq!(cpu.cycles, 7);
    }

    // --- nmi ---

    #[test]
    fn nmi_fires_even_when_interrupt_disabled() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        cpu.set_flag(Flag::I, true);
        bus.write(0xFFFA, 0x00);
        bus.write(0xFFFB, 0xA0);
        cpu.nmi(&mut bus);
        assert_eq!(cpu.pc, 0xA000);
    }

    #[test]
    fn nmi_pushes_three_bytes_to_stack() {
        let (mut cpu, mut bus) = make();
        bus.write(0xFFFC, 0x00);
        bus.write(0xFFFD, 0x80);
        cpu.reset(&mut bus);
        let sp_before = cpu.stack.ptr;
        cpu.nmi(&mut bus);
        assert_eq!(cpu.stack.ptr, sp_before.wrapping_sub(3));
    }

    #[test]
    fn nmi_takes_8_cycles() {
        let (mut cpu, mut bus) = make();
        cpu.nmi(&mut bus);
        assert_eq!(cpu.cycles, 8);
    }

    // --- addressing modes ---

    #[test]
    fn imp_stores_accumulator_in_fetched() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x42;
        cpu.imp(&mut bus);
        assert_eq!(cpu.fetched, 0x42);
    }

    #[test]
    fn imm_sets_addr_abs_to_pc_and_advances() {
        let (mut cpu, mut bus) = setup(0x0200, &[]);
        cpu.imm(&mut bus);
        assert_eq!(cpu.addr_abs, 0x0200);
        assert_eq!(cpu.pc, 0x0201);
    }

    #[test]
    fn zp0_reads_one_byte_address() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0x42)]);
        cpu.zp0(&mut bus);
        assert_eq!(cpu.addr_abs, 0x0042);
        assert_eq!(cpu.pc, 0x0201);
    }

    #[test]
    fn zpx_adds_x_and_wraps_in_zero_page() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xF0)]);
        cpu.x = 0x20;
        cpu.zpx(&mut bus);
        assert_eq!(cpu.addr_abs, 0x0010); // 0xF0 + 0x20 = 0x110, masked to 0x0010
    }

    #[test]
    fn zpy_adds_y_and_wraps_in_zero_page() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xF0)]);
        cpu.y = 0x10;
        cpu.zpy(&mut bus);
        assert_eq!(cpu.addr_abs, 0x0000); // 0xF0 + 0x10 = 0x100, masked to 0x00
    }

    #[test]
    fn rel_positive_offset_unchanged() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0x10)]);
        cpu.rel(&mut bus);
        assert_eq!(cpu.addr_rel, 0x0010);
    }

    #[test]
    fn rel_negative_offset_sign_extends() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0x80)]); // -128 as i8
        cpu.rel(&mut bus);
        assert_eq!(cpu.addr_rel, 0xFF80);
    }

    #[test]
    fn abs_reads_little_endian_16bit_address() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0x34), (0x0201, 0x12)]);
        cpu.abs(&mut bus);
        assert_eq!(cpu.addr_abs, 0x1234);
        assert_eq!(cpu.pc, 0x0202);
    }

    #[test]
    fn abx_no_page_cross_returns_0() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0x00), (0x0201, 0x20)]);
        cpu.x = 0x01;
        let extra = cpu.abx(&mut bus);
        assert_eq!(cpu.addr_abs, 0x2001);
        assert_eq!(extra, 0);
    }

    #[test]
    fn abx_page_cross_returns_1() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFF), (0x0201, 0x20)]);
        cpu.x = 0x01;
        let extra = cpu.abx(&mut bus);
        assert_eq!(cpu.addr_abs, 0x2100);
        assert_eq!(extra, 1);
    }

    #[test]
    fn aby_no_page_cross_returns_0() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0x00), (0x0201, 0x20)]);
        cpu.y = 0x10;
        let extra = cpu.aby(&mut bus);
        assert_eq!(cpu.addr_abs, 0x2010);
        assert_eq!(extra, 0);
    }

    #[test]
    fn aby_page_cross_returns_1() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFE), (0x0201, 0x20)]);
        cpu.y = 0x02;
        let extra = cpu.aby(&mut bus);
        assert_eq!(cpu.addr_abs, 0x2100);
        assert_eq!(extra, 1);
    }

    #[test]
    fn ind_reads_address_from_pointer() {
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0x00),
                (0x0201, 0x30), // pointer stored at 0x3000
                (0x3000, 0x78),
                (0x3001, 0x56), // actual address: 0x5678
            ],
        );
        cpu.ind(&mut bus);
        assert_eq!(cpu.addr_abs, 0x5678);
    }

    #[test]
    fn ind_page_boundary_bug_wraps_hi_byte() {
        // pointer lo is 0xFF → hi byte wraps to start of same page, not next page
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0xFF),
                (0x0201, 0x30), // pointer at 0x30FF
                (0x30FF, 0x80), // lo byte of target
                (0x3000, 0x50), // hi byte comes from 0x3000 (bug), not 0x3100
            ],
        );
        cpu.ind(&mut bus);
        assert_eq!(cpu.addr_abs, 0x5080);
    }

    #[test]
    fn izx_indexes_into_zero_page_by_x() {
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0x20), // base zero-page offset
                (0x0024, 0xCD), // lo at (0x20 + x=4) = 0x24
                (0x0025, 0xAB), // hi at 0x25
            ],
        );
        cpu.x = 0x04;
        cpu.izx(&mut bus);
        assert_eq!(cpu.addr_abs, 0xABCD);
    }

    #[test]
    fn izy_no_page_cross_returns_0() {
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0x40), // zero-page pointer address
                (0x0040, 0x00), // lo byte → base = 0x3000
                (0x0041, 0x30),
            ],
        );
        cpu.y = 0x10;
        let extra = cpu.izy(&mut bus);
        assert_eq!(cpu.addr_abs, 0x3010);
        assert_eq!(extra, 0);
    }

    #[test]
    fn izy_page_cross_returns_1() {
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0x40),
                (0x0040, 0xFF), // lo byte → base = 0x30FF
                (0x0041, 0x30),
            ],
        );
        cpu.y = 0x01;
        let extra = cpu.izy(&mut bus);
        assert_eq!(cpu.addr_abs, 0x3100);
        assert_eq!(extra, 1);
    }

    // --- zp0 edge cases ---

    #[test]
    fn zp0_max_address_stays_in_zero_page() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFF)]);
        cpu.zp0(&mut bus);
        assert_eq!(cpu.addr_abs, 0x00FF);
    }

    // --- zpx edge cases ---

    #[test]
    fn zpx_result_at_top_of_zero_page_no_wrap() {
        // 0xFE + 0x01 = 0xFF — max zero-page address, no wrap
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFE)]);
        cpu.x = 0x01;
        cpu.zpx(&mut bus);
        assert_eq!(cpu.addr_abs, 0x00FF);
    }

    #[test]
    fn zpx_wraps_from_ff_to_zero() {
        // 0xFF + 0x01 = 0x100, masked to 0x00
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFF)]);
        cpu.x = 0x01;
        cpu.zpx(&mut bus);
        assert_eq!(cpu.addr_abs, 0x0000);
    }

    // --- zpy edge cases ---

    #[test]
    fn zpy_wraps_from_ff_to_zero() {
        // 0xFF + 0x01 = 0x100, masked to 0x00
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFF)]);
        cpu.y = 0x01;
        cpu.zpy(&mut bus);
        assert_eq!(cpu.addr_abs, 0x0000);
    }

    // --- rel edge cases ---

    #[test]
    fn rel_max_positive_offset() {
        // 0x7F = +127, maximum positive branch range
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0x7F)]);
        cpu.rel(&mut bus);
        assert_eq!(cpu.addr_rel, 0x007F);
    }

    #[test]
    fn rel_minus_one() {
        // 0xFF = -1 as i8, sign-extends to 0xFFFF
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFF)]);
        cpu.rel(&mut bus);
        assert_eq!(cpu.addr_rel, 0xFFFF);
    }

    // --- abs edge cases ---

    #[test]
    fn abs_top_of_address_space() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFF), (0x0201, 0xFF)]);
        cpu.abs(&mut bus);
        assert_eq!(cpu.addr_abs, 0xFFFF);
    }

    // --- abx edge cases ---

    #[test]
    fn abx_last_byte_of_page_no_cross() {
        // 0x20FE + 1 = 0x20FF — still on same page, no extra cycle
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFE), (0x0201, 0x20)]);
        cpu.x = 0x01;
        let extra = cpu.abx(&mut bus);
        assert_eq!(cpu.addr_abs, 0x20FF);
        assert_eq!(extra, 0);
    }

    // --- aby edge cases ---

    #[test]
    fn aby_last_byte_of_page_no_cross() {
        // 0x20FE + 1 = 0x20FF — still on same page, no extra cycle
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xFE), (0x0201, 0x20)]);
        cpu.y = 0x01;
        let extra = cpu.aby(&mut bus);
        assert_eq!(cpu.addr_abs, 0x20FF);
        assert_eq!(extra, 0);
    }

    // --- ind edge cases ---

    #[test]
    fn ind_page_boundary_bug_in_page_zero() {
        // pointer at 0x00FF: hi byte wraps to 0x0000, not 0x0100
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0xFF),
                (0x0201, 0x00), // pointer = 0x00FF
                (0x00FF, 0x34), // lo byte of target
                (0x0000, 0x12), // hi byte (bug: 0x0000, not 0x0100)
            ],
        );
        cpu.ind(&mut bus);
        assert_eq!(cpu.addr_abs, 0x1234);
    }

    // --- izx edge cases ---

    #[test]
    fn izx_pointer_wraps_at_ff() {
        // base = (0xFB + x=4) & 0xFF = 0xFF; hi must come from 0x0000, not 0x0100
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0xFB),
                (0x00FF, 0xCD), // lo byte at 0xFF
                (0x0000, 0xAB), // hi byte wraps to 0x0000
            ],
        );
        cpu.x = 0x04;
        cpu.izx(&mut bus);
        assert_eq!(cpu.addr_abs, 0xABCD);
    }

    // --- izy edge cases ---

    #[test]
    fn izy_pointer_address_at_ff() {
        // pointer lives at 0xFF; hi byte must come from 0x0000, not 0x0100
        let (mut cpu, mut bus) = setup(
            0x0200,
            &[
                (0x0200, 0xFF), // zero-page pointer address
                (0x00FF, 0x00), // lo byte of base → base = 0x3000
                (0x0000, 0x30), // hi byte wraps to 0x0000
            ],
        );
        cpu.y = 0x05;
        let extra = cpu.izy(&mut bus);
        assert_eq!(cpu.addr_abs, 0x3005);
        assert_eq!(extra, 0);
    }

    // --- flag ops ---

    #[test]
    fn clc_clears_carry() {
        let (mut cpu, mut bus) = make();
        cpu.set_flag(Flag::C, true);
        cpu.clc(&mut bus);
        assert!(!cpu.get_flag(Flag::C));
    }

    #[test]
    fn sec_sets_carry() {
        let (mut cpu, mut bus) = make();
        cpu.set_flag(Flag::C, false);
        cpu.sec(&mut bus);
        assert!(cpu.get_flag(Flag::C));
    }

    #[test]
    fn cld_clears_decimal() {
        let (mut cpu, mut bus) = make();
        cpu.set_flag(Flag::D, true);
        cpu.cld(&mut bus);
        assert!(!cpu.get_flag(Flag::D));
    }

    #[test]
    fn sed_sets_decimal() {
        let (mut cpu, mut bus) = make();
        cpu.set_flag(Flag::D, false);
        cpu.sed(&mut bus);
        assert!(cpu.get_flag(Flag::D));
    }

    #[test]
    fn cli_clears_interrupt_disable() {
        let (mut cpu, mut bus) = make();
        cpu.set_flag(Flag::I, true);
        cpu.cli(&mut bus);
        assert!(!cpu.get_flag(Flag::I));
    }

    #[test]
    fn sei_sets_interrupt_disable() {
        let (mut cpu, mut bus) = make();
        cpu.set_flag(Flag::I, false);
        cpu.sei(&mut bus);
        assert!(cpu.get_flag(Flag::I));
    }

    #[test]
    fn clv_clears_overflow() {
        let (mut cpu, mut bus) = make();
        cpu.set_flag(Flag::V, true);
        cpu.clv(&mut bus);
        assert!(!cpu.get_flag(Flag::V));
    }

    #[test]
    fn flag_ops_dont_touch_other_flags() {
        let (mut cpu, mut bus) = make();
        cpu.status = 0xFF; // all flags set
        cpu.clc(&mut bus);
        // only C cleared, everything else untouched
        assert_eq!(cpu.status, 0xFF & !(Flag::C as u8));
    }

    // --- load / store ---

    fn load_setup(val: u8) -> (Cpu, Bus) {
        let (mut cpu, mut bus) = make();
        bus.write(0x0042, val);
        cpu.addr_abs = 0x0042;
        (cpu, bus)
    }

    #[test]
    fn lda_loads_value_into_a() {
        let (mut cpu, mut bus) = load_setup(0x37);
        cpu.lda(&mut bus);
        assert_eq!(cpu.a, 0x37);
    }

    #[test]
    fn lda_sets_zero_flag() {
        let (mut cpu, mut bus) = load_setup(0x00);
        cpu.lda(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
        assert!(!cpu.get_flag(Flag::N));
    }

    #[test]
    fn lda_sets_negative_flag() {
        let (mut cpu, mut bus) = load_setup(0x80);
        cpu.lda(&mut bus);
        assert!(cpu.get_flag(Flag::N));
        assert!(!cpu.get_flag(Flag::Z));
    }

    #[test]
    fn lda_returns_1() {
        let (mut cpu, mut bus) = load_setup(0x01);
        assert_eq!(cpu.lda(&mut bus), 1);
    }

    #[test]
    fn ldx_loads_value_into_x() {
        let (mut cpu, mut bus) = load_setup(0x55);
        cpu.ldx(&mut bus);
        assert_eq!(cpu.x, 0x55);
    }

    #[test]
    fn ldx_sets_zero_and_negative_flags() {
        let (mut cpu, mut bus) = load_setup(0x00);
        cpu.ldx(&mut bus);
        assert!(cpu.get_flag(Flag::Z));

        let (mut cpu, mut bus) = load_setup(0xFF);
        cpu.ldx(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn ldy_loads_value_into_y() {
        let (mut cpu, mut bus) = load_setup(0xAB);
        cpu.ldy(&mut bus);
        assert_eq!(cpu.y, 0xAB);
    }

    #[test]
    fn ldy_sets_zero_and_negative_flags() {
        let (mut cpu, mut bus) = load_setup(0x00);
        cpu.ldy(&mut bus);
        assert!(cpu.get_flag(Flag::Z));

        let (mut cpu, mut bus) = load_setup(0x80);
        cpu.ldy(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn sta_writes_a_to_addr_abs() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x42;
        cpu.addr_abs = 0x0300;
        cpu.sta(&mut bus);
        assert_eq!(bus.read(0x0300), 0x42);
    }

    #[test]
    fn sta_does_not_change_flags() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x00; // would set Z if flags were touched
        cpu.addr_abs = 0x0300;
        let status_before = cpu.status;
        cpu.sta(&mut bus);
        assert_eq!(cpu.status, status_before);
    }

    #[test]
    fn sta_returns_0() {
        let (mut cpu, mut bus) = make();
        cpu.addr_abs = 0x0300;
        assert_eq!(cpu.sta(&mut bus), 0);
    }

    #[test]
    fn stx_writes_x_to_addr_abs() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x77;
        cpu.addr_abs = 0x0300;
        cpu.stx(&mut bus);
        assert_eq!(bus.read(0x0300), 0x77);
    }

    #[test]
    fn sty_writes_y_to_addr_abs() {
        let (mut cpu, mut bus) = make();
        cpu.y = 0xBB;
        cpu.addr_abs = 0x0300;
        cpu.sty(&mut bus);
        assert_eq!(bus.read(0x0300), 0xBB);
    }

    // --- register transfers ---

    #[test]
    fn tax_copies_a_to_x() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x42;
        cpu.tax(&mut bus);
        assert_eq!(cpu.x, 0x42);
    }

    #[test]
    fn tax_sets_n_z_flags() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x00;
        cpu.tax(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
        assert!(!cpu.get_flag(Flag::N));

        cpu.a = 0x80;
        cpu.tax(&mut bus);
        assert!(cpu.get_flag(Flag::N));
        assert!(!cpu.get_flag(Flag::Z));
    }

    #[test]
    fn tay_copies_a_to_y() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x55;
        cpu.tay(&mut bus);
        assert_eq!(cpu.y, 0x55);
    }

    #[test]
    fn tay_sets_n_z_flags() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x00;
        cpu.tay(&mut bus);
        assert!(cpu.get_flag(Flag::Z));

        cpu.a = 0xFF;
        cpu.tay(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn txa_copies_x_to_a() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x33;
        cpu.txa(&mut bus);
        assert_eq!(cpu.a, 0x33);
    }

    #[test]
    fn txa_sets_n_z_flags() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x00;
        cpu.txa(&mut bus);
        assert!(cpu.get_flag(Flag::Z));

        cpu.x = 0x90;
        cpu.txa(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn tya_copies_y_to_a() {
        let (mut cpu, mut bus) = make();
        cpu.y = 0x77;
        cpu.tya(&mut bus);
        assert_eq!(cpu.a, 0x77);
    }

    #[test]
    fn tsx_copies_stack_ptr_to_x() {
        let (mut cpu, mut bus) = make();
        cpu.stack.ptr = 0xFD;
        cpu.tsx(&mut bus);
        assert_eq!(cpu.x, 0xFD);
    }

    #[test]
    fn tsx_sets_n_z_flags() {
        let (mut cpu, mut bus) = make();
        cpu.stack.ptr = 0x00;
        cpu.tsx(&mut bus);
        assert!(cpu.get_flag(Flag::Z));

        cpu.stack.ptr = 0x80;
        cpu.tsx(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn txs_copies_x_to_stack_ptr() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0xAB;
        cpu.txs(&mut bus);
        assert_eq!(cpu.stack.ptr, 0xAB);
    }

    #[test]
    fn txs_does_not_change_flags() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x00; // would set Z if flags were touched
        let status_before = cpu.status;
        cpu.txs(&mut bus);
        assert_eq!(cpu.status, status_before);
    }

    // --- stack ---

    #[test]
    fn pha_pushes_a_to_stack() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x42;
        let sp_before = cpu.stack.ptr;
        cpu.pha(&mut bus);
        assert_eq!(bus.read(0x0100 + sp_before as u16), 0x42);
        assert_eq!(cpu.stack.ptr, sp_before.wrapping_sub(1));
    }

    #[test]
    fn pha_does_not_change_flags() {
        let (mut cpu, mut bus) = make();
        cpu.a = 0x00;
        let status_before = cpu.status;
        cpu.pha(&mut bus);
        assert_eq!(cpu.status, status_before);
    }

    #[test]
    fn php_pushes_status_with_b_and_u_set() {
        let (mut cpu, mut bus) = make();
        cpu.status = 0b0000_0000; // all clear
        let sp_before = cpu.stack.ptr;
        cpu.php(&mut bus);
        let pushed = bus.read(0x0100 + sp_before as u16);
        assert!(pushed & (Flag::B as u8) != 0);
        assert!(pushed & (Flag::U as u8) != 0);
    }

    #[test]
    fn php_does_not_modify_cpu_status() {
        let (mut cpu, mut bus) = make();
        cpu.status = 0b0000_0000;
        cpu.php(&mut bus);
        assert_eq!(cpu.status, 0b0000_0000);
    }

    #[test]
    fn pla_pops_into_a() {
        let (mut cpu, mut bus) = make();
        cpu.pha(&mut bus); // push 0 (a is 0 from new())
        cpu.a = 0xFF; // change A so we can verify it's overwritten
        bus.write(0x0100 + cpu.stack.ptr.wrapping_add(1) as u16, 0x42);
        cpu.pla(&mut bus);
        assert_eq!(cpu.a, 0x42);
    }

    #[test]
    fn pla_sets_n_z_flags() {
        let (mut cpu, mut bus) = make();
        bus.write(0x0100 + cpu.stack.ptr.wrapping_add(1) as u16, 0x00);
        cpu.pla(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
        assert!(!cpu.get_flag(Flag::N));

        bus.write(0x0100 + cpu.stack.ptr.wrapping_add(1) as u16, 0x80);
        cpu.pla(&mut bus);
        assert!(cpu.get_flag(Flag::N));
        assert!(!cpu.get_flag(Flag::Z));
    }

    #[test]
    fn plp_restores_status_clears_b_sets_u() {
        let (mut cpu, mut bus) = make();
        cpu.status = 0x00; // clear everything so initial state can't cause a false pass
        // place value with B set, U clear on the stack
        bus.write(0x0100 + cpu.stack.ptr.wrapping_add(1) as u16, 0b1101_1111);
        cpu.plp(&mut bus);
        assert!(!cpu.get_flag(Flag::B)); // B must be cleared
        assert!(cpu.get_flag(Flag::U)); // U must be set
        assert!(cpu.get_flag(Flag::N)); // N was set in pushed byte, must survive
        assert!(cpu.get_flag(Flag::C)); // C was set in pushed byte, must survive
    }

    // --- increment / decrement ---

    #[test]
    fn inx_increments_x() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x41;
        cpu.inx(&mut bus);
        assert_eq!(cpu.x, 0x42);
    }

    #[test]
    fn inx_wraps_at_ff() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0xFF;
        cpu.inx(&mut bus);
        assert_eq!(cpu.x, 0x00);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn inx_sets_negative_flag() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x7F;
        cpu.inx(&mut bus);
        assert_eq!(cpu.x, 0x80);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn iny_increments_y() {
        let (mut cpu, mut bus) = make();
        cpu.y = 0x10;
        cpu.iny(&mut bus);
        assert_eq!(cpu.y, 0x11);
    }

    #[test]
    fn iny_wraps_at_ff() {
        let (mut cpu, mut bus) = make();
        cpu.y = 0xFF;
        cpu.iny(&mut bus);
        assert_eq!(cpu.y, 0x00);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn dex_decrements_x() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x05;
        cpu.dex(&mut bus);
        assert_eq!(cpu.x, 0x04);
    }

    #[test]
    fn dex_wraps_at_zero() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x00;
        cpu.dex(&mut bus);
        assert_eq!(cpu.x, 0xFF);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn dey_decrements_y() {
        let (mut cpu, mut bus) = make();
        cpu.y = 0x03;
        cpu.dey(&mut bus);
        assert_eq!(cpu.y, 0x02);
    }

    #[test]
    fn dey_wraps_at_zero() {
        let (mut cpu, mut bus) = make();
        cpu.x = 0x10; // x != 0 so wrong register would give wrong N flag
        cpu.y = 0x00;
        cpu.dey(&mut bus);
        assert_eq!(cpu.y, 0xFF);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn dey_sets_zero_flag() {
        let (mut cpu, mut bus) = make();
        cpu.y = 0x01;
        cpu.dey(&mut bus);
        assert_eq!(cpu.y, 0x00);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn inc_increments_memory() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0042, 0x10)]);
        cpu.addr_abs = 0x0042;
        cpu.inc(&mut bus);
        assert_eq!(bus.read(0x0042), 0x11);
    }

    #[test]
    fn inc_wraps_memory_at_ff() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0042, 0xFF)]);
        cpu.addr_abs = 0x0042;
        cpu.inc(&mut bus);
        assert_eq!(bus.read(0x0042), 0x00);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn dec_decrements_memory() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0042, 0x10)]);
        cpu.addr_abs = 0x0042;
        cpu.dec(&mut bus);
        assert_eq!(bus.read(0x0042), 0x0F);
    }

    #[test]
    fn dec_wraps_memory_at_zero() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0042, 0x00)]);
        cpu.addr_abs = 0x0042;
        cpu.dec(&mut bus);
        assert_eq!(bus.read(0x0042), 0xFF);
        assert!(cpu.get_flag(Flag::N));
    }

    // --- logical ---

    #[test]
    fn and_ands_into_a() {
        let (mut cpu, mut bus) = load_setup(0b1010_1010);
        cpu.a = 0b1111_0000;
        cpu.and(&mut bus);
        assert_eq!(cpu.a, 0b1010_0000);
    }

    #[test]
    fn and_sets_zero_flag_when_result_zero() {
        let (mut cpu, mut bus) = load_setup(0b0000_1111);
        cpu.a = 0b1111_0000;
        cpu.and(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn and_sets_negative_flag() {
        let (mut cpu, mut bus) = load_setup(0b1100_0000);
        cpu.a = 0b1111_1111;
        cpu.and(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn and_returns_1() {
        let (mut cpu, mut bus) = load_setup(0xFF);
        assert_eq!(cpu.and(&mut bus), 1);
    }

    #[test]
    fn ora_ors_into_a() {
        let (mut cpu, mut bus) = load_setup(0b0000_1111);
        cpu.a = 0b1111_0000;
        cpu.ora(&mut bus);
        assert_eq!(cpu.a, 0b1111_1111);
    }

    #[test]
    fn ora_sets_zero_flag_when_both_zero() {
        let (mut cpu, mut bus) = load_setup(0x00);
        cpu.a = 0x00;
        cpu.ora(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn ora_returns_1() {
        let (mut cpu, mut bus) = load_setup(0x00);
        assert_eq!(cpu.ora(&mut bus), 1);
    }

    #[test]
    fn eor_xors_into_a() {
        let (mut cpu, mut bus) = load_setup(0b1010_1010);
        cpu.a = 0b1111_1111;
        cpu.eor(&mut bus);
        assert_eq!(cpu.a, 0b0101_0101);
    }

    #[test]
    fn eor_same_value_gives_zero() {
        let (mut cpu, mut bus) = load_setup(0x42);
        cpu.a = 0x42;
        cpu.eor(&mut bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn eor_returns_1() {
        let (mut cpu, mut bus) = load_setup(0x00);
        assert_eq!(cpu.eor(&mut bus), 1);
    }

    #[test]
    fn bit_sets_z_when_a_and_val_is_zero() {
        let (mut cpu, mut bus) = load_setup(0b0000_1111);
        cpu.a = 0b1111_0000;
        cpu.bit(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn bit_sets_n_from_bit7_of_memory_not_result() {
        let (mut cpu, mut bus) = load_setup(0b1000_0000); // bit 7 set
        cpu.a = 0b0000_0000; // A & val = 0, but N comes from val
        cpu.bit(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn bit_sets_v_from_bit6_of_memory() {
        let (mut cpu, mut bus) = load_setup(0b0100_0000); // bit 6 set
        cpu.a = 0b0000_0000;
        cpu.bit(&mut bus);
        assert!(cpu.get_flag(Flag::V));
    }

    #[test]
    fn bit_clears_v_when_bit6_clear() {
        let (mut cpu, mut bus) = load_setup(0b1000_0000); // bit 6 clear
        cpu.set_flag(Flag::V, true);
        cpu.bit(&mut bus);
        assert!(!cpu.get_flag(Flag::V));
    }

    // --- shifts and rotates ---

    // For imp mode tests, set opcode to a known imp instruction (0xEA = NOP/imp)
    // so fetch() returns fetched (which imp() set to A) rather than reading memory.
    fn imp_mode_setup() -> (Cpu, Bus) {
        let (mut cpu, bus) = make();
        cpu.opcode = 0xEA; // NOP — imp addressing
        (cpu, bus)
    }

    #[test]
    fn asl_shifts_memory_left() {
        let (mut cpu, mut bus) = load_setup(0b0100_0010);
        cpu.asl(&mut bus);
        assert_eq!(bus.read(0x0042), 0b1000_0100);
        assert!(!cpu.get_flag(Flag::C));
    }

    #[test]
    fn asl_puts_old_bit7_in_carry() {
        let (mut cpu, mut bus) = load_setup(0b1000_0001);
        cpu.asl(&mut bus);
        assert!(cpu.get_flag(Flag::C));
        assert_eq!(bus.read(0x0042), 0b0000_0010);
    }

    #[test]
    fn asl_accumulator_mode() {
        let (mut cpu, mut bus) = imp_mode_setup();
        cpu.a = 0b0000_0011;
        cpu.fetched = cpu.a;
        cpu.asl(&mut bus);
        assert_eq!(cpu.a, 0b0000_0110);
    }

    #[test]
    fn lsr_shifts_memory_right() {
        let (mut cpu, mut bus) = load_setup(0b1000_0100);
        cpu.lsr(&mut bus);
        assert_eq!(bus.read(0x0042), 0b0100_0010);
        assert!(!cpu.get_flag(Flag::C));
        assert!(!cpu.get_flag(Flag::N)); // N always 0 after LSR
    }

    #[test]
    fn lsr_puts_old_bit0_in_carry() {
        let (mut cpu, mut bus) = load_setup(0b0000_0011);
        cpu.lsr(&mut bus);
        assert!(cpu.get_flag(Flag::C));
        assert_eq!(bus.read(0x0042), 0b0000_0001);
    }

    #[test]
    fn lsr_accumulator_mode() {
        let (mut cpu, mut bus) = imp_mode_setup();
        cpu.a = 0b1000_0000;
        cpu.fetched = cpu.a;
        cpu.lsr(&mut bus);
        assert_eq!(cpu.a, 0b0100_0000);
        assert!(!cpu.get_flag(Flag::N));
    }

    #[test]
    fn rol_rotates_carry_into_bit0() {
        let (mut cpu, mut bus) = load_setup(0b0000_0001);
        cpu.set_flag(Flag::C, true);
        cpu.rol(&mut bus);
        assert_eq!(bus.read(0x0042), 0b0000_0011); // shifted left + carry in
        assert!(!cpu.get_flag(Flag::C)); // old bit 7 was 0
    }

    #[test]
    fn rol_puts_old_bit7_in_carry() {
        let (mut cpu, mut bus) = load_setup(0b1000_0000);
        cpu.set_flag(Flag::C, false);
        cpu.rol(&mut bus);
        assert!(cpu.get_flag(Flag::C));
        assert_eq!(bus.read(0x0042), 0b0000_0000);
    }

    #[test]
    fn rol_accumulator_mode() {
        let (mut cpu, mut bus) = imp_mode_setup();
        cpu.a = 0b0100_0000;
        cpu.fetched = cpu.a;
        cpu.set_flag(Flag::C, true);
        cpu.rol(&mut bus);
        assert_eq!(cpu.a, 0b1000_0001);
    }

    #[test]
    fn ror_rotates_carry_into_bit7() {
        let (mut cpu, mut bus) = load_setup(0b0000_0010);
        cpu.set_flag(Flag::C, true);
        cpu.ror(&mut bus);
        assert_eq!(bus.read(0x0042), 0b1000_0001); // carry into bit 7
        assert!(!cpu.get_flag(Flag::C)); // old bit 0 was 0
    }

    #[test]
    fn ror_puts_old_bit0_in_carry() {
        let (mut cpu, mut bus) = load_setup(0b0000_0001);
        cpu.set_flag(Flag::C, false);
        cpu.ror(&mut bus);
        assert!(cpu.get_flag(Flag::C));
        assert_eq!(bus.read(0x0042), 0b0000_0000);
    }

    #[test]
    fn ror_accumulator_mode() {
        let (mut cpu, mut bus) = imp_mode_setup();
        cpu.a = 0b0000_0010;
        cpu.fetched = cpu.a;
        cpu.set_flag(Flag::C, true);
        cpu.ror(&mut bus);
        assert_eq!(cpu.a, 0b1000_0001);
    }

    // --- compare ---

    #[test]
    fn cmp_equal_sets_z_and_c() {
        let (mut cpu, mut bus) = load_setup(0x42);
        cpu.a = 0x42;
        cpu.cmp(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
        assert!(cpu.get_flag(Flag::C));
        assert!(!cpu.get_flag(Flag::N));
    }

    #[test]
    fn cmp_greater_sets_c_clears_z() {
        let (mut cpu, mut bus) = load_setup(0x10);
        cpu.a = 0x20;
        cpu.cmp(&mut bus);
        assert!(cpu.get_flag(Flag::C));
        assert!(!cpu.get_flag(Flag::Z));
    }

    #[test]
    fn cmp_less_clears_c_sets_n() {
        let (mut cpu, mut bus) = load_setup(0x20);
        cpu.a = 0x10;
        cpu.cmp(&mut bus);
        assert!(!cpu.get_flag(Flag::C));
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn cmp_n_flag_uses_bit7_not_bit11() {
        // A=0x7F, mem=0x80 → tmp = 0xFF → bit 7 set → N=1
        // 0xFF & 0x0800 == 0, so this test would fail with the wrong mask
        let (mut cpu, mut bus) = load_setup(0x80);
        cpu.a = 0x7F;
        cpu.cmp(&mut bus);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn cmp_returns_1() {
        let (mut cpu, mut bus) = load_setup(0x00);
        assert_eq!(cpu.cmp(&mut bus), 1);
    }

    #[test]
    fn cpx_compares_x() {
        let (mut cpu, mut bus) = load_setup(0x42);
        cpu.x = 0x42;
        cpu.cpx(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
        assert!(cpu.get_flag(Flag::C));
    }

    #[test]
    fn cpx_less_clears_c() {
        let (mut cpu, mut bus) = load_setup(0x50);
        cpu.x = 0x30;
        cpu.cpx(&mut bus);
        assert!(!cpu.get_flag(Flag::C));
    }

    #[test]
    fn cpx_returns_0() {
        let (mut cpu, mut bus) = load_setup(0x00);
        assert_eq!(cpu.cpx(&mut bus), 0);
    }

    #[test]
    fn cpy_compares_y() {
        let (mut cpu, mut bus) = load_setup(0x42);
        cpu.y = 0x42;
        cpu.cpy(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
        assert!(cpu.get_flag(Flag::C));
    }

    #[test]
    fn cpy_less_clears_c() {
        let (mut cpu, mut bus) = load_setup(0x50);
        cpu.y = 0x30;
        cpu.cpy(&mut bus);
        assert!(!cpu.get_flag(Flag::C));
    }

    #[test]
    fn cpy_returns_0() {
        let (mut cpu, mut bus) = load_setup(0x00);
        assert_eq!(cpu.cpy(&mut bus), 0);
    }

    // --- adc ---

    #[test]
    fn adc_basic_no_carry_in() {
        // 0x10 + 0x20 = 0x30, no carry in, no carry out, no overflow
        let (mut cpu, mut bus) = load_setup(0x20);
        cpu.a = 0x10;
        cpu.set_flag(Flag::C, false);
        cpu.adc(&mut bus);
        assert_eq!(cpu.a, 0x30);
        assert!(!cpu.get_flag(Flag::C));
        assert!(!cpu.get_flag(Flag::Z));
        assert!(!cpu.get_flag(Flag::N));
        assert!(!cpu.get_flag(Flag::V));
    }

    #[test]
    fn adc_with_carry_in() {
        // 0x10 + 0x20 + C=1 = 0x31
        let (mut cpu, mut bus) = load_setup(0x20);
        cpu.a = 0x10;
        cpu.set_flag(Flag::C, true);
        cpu.adc(&mut bus);
        assert_eq!(cpu.a, 0x31);
    }

    #[test]
    fn adc_sets_carry_on_overflow() {
        // 0xFF + 0x01 = 0x100, carry out
        let (mut cpu, mut bus) = load_setup(0x01);
        cpu.a = 0xFF;
        cpu.set_flag(Flag::C, false);
        cpu.adc(&mut bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.get_flag(Flag::C));
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn adc_sets_z_when_result_zero() {
        // 0x00 + 0x00 = 0x00
        let (mut cpu, mut bus) = load_setup(0x00);
        cpu.a = 0x00;
        cpu.set_flag(Flag::C, false);
        cpu.adc(&mut bus);
        assert!(cpu.get_flag(Flag::Z));
        assert!(!cpu.get_flag(Flag::N));
    }

    #[test]
    fn adc_sets_n_when_result_negative() {
        // 0x40 + 0x40 = 0x80, bit 7 set → N=1
        let (mut cpu, mut bus) = load_setup(0x40);
        cpu.a = 0x40;
        cpu.set_flag(Flag::C, false);
        cpu.adc(&mut bus);
        assert_eq!(cpu.a, 0x80);
        assert!(cpu.get_flag(Flag::N));
    }

    #[test]
    fn adc_sets_v_positive_overflow() {
        // +64 + +64 = 0x80 (−128 in signed) → signed overflow → V=1
        let (mut cpu, mut bus) = load_setup(0x40);
        cpu.a = 0x40;
        cpu.set_flag(Flag::C, false);
        cpu.adc(&mut bus);
        assert!(cpu.get_flag(Flag::V));
    }

    #[test]
    fn adc_sets_v_negative_overflow() {
        // −1 (0xFF) + −1 (0xFF) = 0xFE (−2, no overflow — both negative, result negative)
        // but −128 (0x80) + −128 (0x80) = 0x00 → signed overflow → V=1
        let (mut cpu, mut bus) = load_setup(0x80);
        cpu.a = 0x80;
        cpu.set_flag(Flag::C, false);
        cpu.adc(&mut bus);
        assert!(cpu.get_flag(Flag::V));
    }

    #[test]
    fn adc_no_v_when_signs_differ() {
        // +127 (0x7F) + −1 (0xFF) = 0x7E (+126) — different input signs, never overflow
        let (mut cpu, mut bus) = load_setup(0xFF);
        cpu.a = 0x7F;
        cpu.set_flag(Flag::C, false);
        cpu.adc(&mut bus);
        assert_eq!(cpu.a, 0x7E);
        assert!(!cpu.get_flag(Flag::V));
    }

    #[test]
    fn adc_returns_1() {
        let (mut cpu, mut bus) = load_setup(0x00);
        assert_eq!(cpu.adc(&mut bus), 1);
    }

    // --- sbc ---

    #[test]
    fn sbc_basic_no_borrow() {
        // 0x30 - 0x10 = 0x20, C=1 (no borrow in), result no borrow out → C=1
        let (mut cpu, mut bus) = load_setup(0x10);
        cpu.a = 0x30;
        cpu.set_flag(Flag::C, true);
        cpu.sbc(&mut bus);
        assert_eq!(cpu.a, 0x20);
        assert!(cpu.get_flag(Flag::C));
        assert!(!cpu.get_flag(Flag::V));
    }

    #[test]
    fn sbc_with_borrow_in() {
        // 0x30 - 0x10 - borrow(1) = 0x1F
        let (mut cpu, mut bus) = load_setup(0x10);
        cpu.a = 0x30;
        cpu.set_flag(Flag::C, false);
        cpu.sbc(&mut bus);
        assert_eq!(cpu.a, 0x1F);
    }

    #[test]
    fn sbc_clears_carry_on_borrow() {
        // 0x10 - 0x20 → borrow, C=0
        let (mut cpu, mut bus) = load_setup(0x20);
        cpu.a = 0x10;
        cpu.set_flag(Flag::C, true);
        cpu.sbc(&mut bus);
        assert!(!cpu.get_flag(Flag::C));
    }

    #[test]
    fn sbc_sets_z_when_equal() {
        // 0x42 - 0x42 = 0x00 → Z=1
        let (mut cpu, mut bus) = load_setup(0x42);
        cpu.a = 0x42;
        cpu.set_flag(Flag::C, true);
        cpu.sbc(&mut bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.get_flag(Flag::Z));
    }

    #[test]
    fn sbc_sets_v_negative_minus_positive() {
        // −128 (0x80) - +1 (0x01) = −129, wraps to 0x7F → sign changed → V=1
        let (mut cpu, mut bus) = load_setup(0x01);
        cpu.a = 0x80;
        cpu.set_flag(Flag::C, true);
        cpu.sbc(&mut bus);
        assert!(cpu.get_flag(Flag::V));
    }

    #[test]
    fn sbc_no_v_when_no_signed_overflow() {
        // +5 - +3 = +2, no signed overflow
        let (mut cpu, mut bus) = load_setup(0x03);
        cpu.a = 0x05;
        cpu.set_flag(Flag::C, true);
        cpu.sbc(&mut bus);
        assert_eq!(cpu.a, 0x02);
        assert!(!cpu.get_flag(Flag::V));
    }

    #[test]
    fn sbc_returns_1() {
        let (mut cpu, mut bus) = load_setup(0x00);
        cpu.set_flag(Flag::C, true);
        assert_eq!(cpu.sbc(&mut bus), 1);
    }

    // --- branches ---
    //
    // Branch tests need pc and addr_rel set directly — no full clock() setup needed.
    // addr_rel is a u16 holding the sign-extended 8-bit offset (set by rel() addressing mode).
    // Positive offset example: addr_rel = 0x0004  → pc advances forward 4 bytes.
    // Negative offset example: addr_rel = 0xFFFC  → pc steps back 4 bytes (wraps correctly in u16).
    //
    // Three outcomes to test per branch:
    //   1. condition false  → pc unchanged, cycles unchanged, returns 0
    //   2. condition true, same page  → pc = pc + addr_rel, cycles += 1, returns 0
    //   3. condition true, page cross  → pc crosses 0xXX00 boundary, cycles += 2, returns 0

    fn branch_setup(pc: u16, addr_rel: u16) -> (Cpu, Bus) {
        let (mut cpu, bus) = make();
        cpu.pc = pc;
        cpu.addr_rel = addr_rel;
        (cpu, bus)
    }

    #[test]
    fn bcc_not_taken_when_carry_set() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0010);
        cpu.set_flag(Flag::C, true);
        let before_pc = cpu.pc;
        let before_cycles = cpu.cycles;
        cpu.bcc(&mut bus);
        assert_eq!(cpu.pc, before_pc);
        assert_eq!(cpu.cycles, before_cycles);
    }

    #[test]
    fn bcc_taken_same_page() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0010);
        cpu.set_flag(Flag::C, false);
        cpu.cycles = 0;
        cpu.bcc(&mut bus);
        assert_eq!(cpu.pc, 0x0210);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn bcc_taken_page_cross() {
        // pc=0x02F0 + offset=0x20 = 0x0310 — crosses page boundary (0x02xx → 0x03xx)
        let (mut cpu, mut bus) = branch_setup(0x02F0, 0x0020);
        cpu.set_flag(Flag::C, false);
        cpu.cycles = 0;
        cpu.bcc(&mut bus);
        assert_eq!(cpu.pc, 0x0310);
        assert_eq!(cpu.cycles, 2);
    }

    #[test]
    fn bcc_taken_negative_offset() {
        // addr_rel = 0xFFFC = -4 in sign-extended u16; pc=0x0210 → 0x020C
        let (mut cpu, mut bus) = branch_setup(0x0210, 0xFFFC);
        cpu.set_flag(Flag::C, false);
        cpu.cycles = 0;
        cpu.bcc(&mut bus);
        assert_eq!(cpu.pc, 0x020C);
    }

    #[test]
    fn bcc_returns_0() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0010);
        cpu.set_flag(Flag::C, false);
        assert_eq!(cpu.bcc(&mut bus), 0);
    }

    #[test]
    fn bcs_taken_when_carry_set() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0010);
        cpu.set_flag(Flag::C, true);
        cpu.cycles = 0;
        cpu.bcs(&mut bus);
        assert_eq!(cpu.pc, 0x0210);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn bcs_not_taken_when_carry_clear() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0010);
        cpu.set_flag(Flag::C, false);
        let before_pc = cpu.pc;
        cpu.bcs(&mut bus);
        assert_eq!(cpu.pc, before_pc);
    }

    #[test]
    fn beq_taken_when_zero_set() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0008);
        cpu.set_flag(Flag::Z, true);
        cpu.cycles = 0;
        cpu.beq(&mut bus);
        assert_eq!(cpu.pc, 0x0208);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn bne_taken_when_zero_clear() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0008);
        cpu.set_flag(Flag::Z, false);
        cpu.cycles = 0;
        cpu.bne(&mut bus);
        assert_eq!(cpu.pc, 0x0208);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn bmi_taken_when_negative_set() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0004);
        cpu.set_flag(Flag::N, true);
        cpu.cycles = 0;
        cpu.bmi(&mut bus);
        assert_eq!(cpu.pc, 0x0204);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn bpl_taken_when_negative_clear() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0004);
        cpu.set_flag(Flag::N, false);
        cpu.cycles = 0;
        cpu.bpl(&mut bus);
        assert_eq!(cpu.pc, 0x0204);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn bvc_taken_when_overflow_clear() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0006);
        cpu.set_flag(Flag::V, false);
        cpu.cycles = 0;
        cpu.bvc(&mut bus);
        assert_eq!(cpu.pc, 0x0206);
        assert_eq!(cpu.cycles, 1);
    }

    #[test]
    fn bvs_taken_when_overflow_set() {
        let (mut cpu, mut bus) = branch_setup(0x0200, 0x0006);
        cpu.set_flag(Flag::V, true);
        cpu.cycles = 0;
        cpu.bvs(&mut bus);
        assert_eq!(cpu.pc, 0x0206);
        assert_eq!(cpu.cycles, 1);
    }

    // --- control flow ---

    // Helper: set up cpu with a known stack pointer and program counter.
    fn ctrl_setup(pc: u16) -> (Cpu, Bus) {
        let (mut cpu, bus) = make();
        cpu.pc = pc;
        cpu.stack.ptr = 0xFF; // full stack
        (cpu, bus)
    }

    // --- jmp ---

    #[test]
    fn jmp_sets_pc_to_addr_abs() {
        let (mut cpu, mut bus) = ctrl_setup(0x0300);
        cpu.addr_abs = 0x1234;
        cpu.jmp(&mut bus);
        assert_eq!(cpu.pc, 0x1234);
    }

    #[test]
    fn jmp_returns_0() {
        let (mut cpu, mut bus) = ctrl_setup(0x0300);
        cpu.addr_abs = 0x1234;
        assert_eq!(cpu.jmp(&mut bus), 0);
    }

    // --- jsr / rts ---

    #[test]
    fn jsr_pushes_pc_minus_1_and_jumps() {
        // pc = 0x0303 (one past the full 3-byte JSR instruction)
        // jsr should push 0x0302 (hi=0x03, lo=0x02) and jump to addr_abs
        let (mut cpu, mut bus) = ctrl_setup(0x0303);
        cpu.addr_abs = 0x1000;
        cpu.jsr(&mut bus);
        assert_eq!(cpu.pc, 0x1000);
        // stack should hold the return address (pc - 1 = 0x0302)
        let lo = cpu.stack.pop(&mut bus);
        let hi = cpu.stack.pop(&mut bus);
        assert_eq!((hi as u16) << 8 | lo as u16, 0x0302);
    }

    #[test]
    fn jsr_returns_0() {
        let (mut cpu, mut bus) = ctrl_setup(0x0303);
        cpu.addr_abs = 0x1000;
        assert_eq!(cpu.jsr(&mut bus), 0);
    }

    #[test]
    fn rts_restores_pc_from_stack() {
        // push return address 0x0302; rts should set pc to 0x0303
        let (mut cpu, mut bus) = ctrl_setup(0x1000);
        cpu.stack.push(0x03, &mut bus); // hi
        cpu.stack.push(0x02, &mut bus); // lo
        cpu.rts(&mut bus);
        assert_eq!(cpu.pc, 0x0303);
    }

    #[test]
    fn jsr_rts_roundtrip() {
        let (mut cpu, mut bus) = ctrl_setup(0x0303);
        cpu.addr_abs = 0x1000;
        cpu.jsr(&mut bus);
        assert_eq!(cpu.pc, 0x1000);
        cpu.rts(&mut bus);
        assert_eq!(cpu.pc, 0x0303);
    }

    #[test]
    fn rts_returns_0() {
        let (mut cpu, mut bus) = ctrl_setup(0x1000);
        cpu.stack.push(0x03, &mut bus);
        cpu.stack.push(0x02, &mut bus);
        assert_eq!(cpu.rts(&mut bus), 0);
    }

    // --- rti ---

    #[test]
    fn rti_restores_status_and_pc() {
        let (mut cpu, mut bus) = ctrl_setup(0x1000);
        // push in reverse order: pc hi, pc lo, status
        cpu.stack.push(0x03, &mut bus); // pc hi
        cpu.stack.push(0x00, &mut bus); // pc lo
        cpu.stack.push(0b1100_1111, &mut bus); // status with B set
        cpu.rti(&mut bus);
        assert_eq!(cpu.pc, 0x0300);
        assert!(!cpu.get_flag(Flag::B)); // B cleared
        assert!(cpu.get_flag(Flag::U)); // U set
    }

    #[test]
    fn rti_no_plus_one_on_pc() {
        // unlike RTS, RTI restores the exact saved address (no +1)
        let (mut cpu, mut bus) = ctrl_setup(0x1000);
        cpu.stack.push(0x04, &mut bus); // pc hi
        cpu.stack.push(0x00, &mut bus); // pc lo
        cpu.stack.push(0x00, &mut bus); // status
        cpu.rti(&mut bus);
        assert_eq!(cpu.pc, 0x0400);
    }

    #[test]
    fn rti_returns_0() {
        let (mut cpu, mut bus) = ctrl_setup(0x1000);
        cpu.stack.push(0x03, &mut bus);
        cpu.stack.push(0x00, &mut bus);
        cpu.stack.push(0x00, &mut bus);
        assert_eq!(cpu.rti(&mut bus), 0);
    }

    // --- brk ---

    #[test]
    fn brk_pushes_pc_and_status_jumps_to_irq_vector() {
        let (mut cpu, mut bus) = ctrl_setup(0x0201);
        // set IRQ vector to 0xBEEF
        bus.write(IRQ_VECTOR, 0xEF);
        bus.write(IRQ_VECTOR + 1, 0xBE);
        cpu.status = 0b0000_0000;
        cpu.brk(&mut bus);
        assert_eq!(cpu.pc, 0xBEEF);
        assert!(cpu.get_flag(Flag::I)); // I set
    }

    #[test]
    fn brk_pushed_status_has_b_and_u_set() {
        let (mut cpu, mut bus) = ctrl_setup(0x0201);
        bus.write(IRQ_VECTOR, 0x00);
        bus.write(IRQ_VECTOR + 1, 0x10);
        cpu.status = 0x00;
        cpu.brk(&mut bus);
        // stack (top to bottom): status, pc lo, pc hi — pop all three
        let pushed_status = cpu.stack.pop(&mut bus);
        assert!(pushed_status & Flag::B as u8 != 0);
        assert!(pushed_status & Flag::U as u8 != 0);
    }

    #[test]
    fn brk_returns_0() {
        let (mut cpu, mut bus) = ctrl_setup(0x0201);
        bus.write(IRQ_VECTOR, 0x00);
        bus.write(IRQ_VECTOR + 1, 0x10);
        assert_eq!(cpu.brk(&mut bus), 0);
    }

    // --- clock ---

    #[test]
    fn clock_advances_pc_past_opcode() {
        // NOP = 0xEA, IMP addressing, 2 cycles
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xEA)]);
        cpu.clock(&mut bus);
        assert_eq!(cpu.pc, 0x0201);
    }

    #[test]
    fn clock_counts_down_cycles_for_nop() {
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xEA)]);
        cpu.clock(&mut bus); // execute: cycles set to 2, then decremented → 1
        assert_eq!(cpu.cycles, 1);
        cpu.clock(&mut bus); // just ticks: cycles → 0
        assert_eq!(cpu.cycles, 0);
    }

    #[test]
    fn clock_does_not_execute_until_cycles_zero() {
        // Two NOPs back to back
        let (mut cpu, mut bus) = setup(0x0200, &[(0x0200, 0xEA), (0x0201, 0xEA)]);
        cpu.clock(&mut bus); // executes first NOP, cycles = 1
        cpu.clock(&mut bus); // ticks down, cycles = 0, PC still at 0x0201
        assert_eq!(cpu.pc, 0x0201);
        cpu.clock(&mut bus); // executes second NOP
        assert_eq!(cpu.pc, 0x0202);
    }
}

fn build_lookup() -> [Instruction; 256] {
    macro_rules! i {
        ($name:literal, $op:ident, $am:ident, $c:literal) => {
            Instruction {
                name: $name,
                operate: Cpu::$op,
                addr_mode: Cpu::$am,
                cycles: $c,
            }
        };
    }

    [
        //         0                              1                              2                              3                              4                              5                              6                              7                              8                              9                              A                              B                              C                              D                              E                              F
        /* 0 */
        i!("BRK", brk, imm, 7),
        i!("ORA", ora, izx, 6),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 3),
        i!("ORA", ora, zp0, 3),
        i!("ASL", asl, zp0, 5),
        i!("???", xxx, imp, 5),
        i!("PHP", php, imp, 3),
        i!("ORA", ora, imm, 2),
        i!("ASL", asl, imp, 2),
        i!("???", xxx, imp, 2),
        i!("???", nop, imp, 4),
        i!("ORA", ora, abs, 4),
        i!("ASL", asl, abs, 6),
        i!("???", xxx, imp, 6),
        /* 1 */ i!("BPL", bpl, rel, 2),
        i!("ORA", ora, izy, 5),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 4),
        i!("ORA", ora, zpx, 4),
        i!("ASL", asl, zpx, 6),
        i!("???", xxx, imp, 6),
        i!("CLC", clc, imp, 2),
        i!("ORA", ora, aby, 4),
        i!("???", nop, imp, 2),
        i!("???", xxx, imp, 7),
        i!("???", nop, imp, 4),
        i!("ORA", ora, abx, 4),
        i!("ASL", asl, abx, 7),
        i!("???", xxx, imp, 7),
        /* 2 */ i!("JSR", jsr, abs, 6),
        i!("AND", and, izx, 6),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("BIT", bit, zp0, 3),
        i!("AND", and, zp0, 3),
        i!("ROL", rol, zp0, 5),
        i!("???", xxx, imp, 5),
        i!("PLP", plp, imp, 4),
        i!("AND", and, imm, 2),
        i!("ROL", rol, imp, 2),
        i!("???", xxx, imp, 2),
        i!("BIT", bit, abs, 4),
        i!("AND", and, abs, 4),
        i!("ROL", rol, abs, 6),
        i!("???", xxx, imp, 6),
        /* 3 */ i!("BMI", bmi, rel, 2),
        i!("AND", and, izy, 5),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 4),
        i!("AND", and, zpx, 4),
        i!("ROL", rol, zpx, 6),
        i!("???", xxx, imp, 6),
        i!("SEC", sec, imp, 2),
        i!("AND", and, aby, 4),
        i!("???", nop, imp, 2),
        i!("???", xxx, imp, 7),
        i!("???", nop, imp, 4),
        i!("AND", and, abx, 4),
        i!("ROL", rol, abx, 7),
        i!("???", xxx, imp, 7),
        /* 4 */ i!("RTI", rti, imp, 6),
        i!("EOR", eor, izx, 6),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 3),
        i!("EOR", eor, zp0, 3),
        i!("LSR", lsr, zp0, 5),
        i!("???", xxx, imp, 5),
        i!("PHA", pha, imp, 3),
        i!("EOR", eor, imm, 2),
        i!("LSR", lsr, imp, 2),
        i!("???", xxx, imp, 2),
        i!("JMP", jmp, abs, 3),
        i!("EOR", eor, abs, 4),
        i!("LSR", lsr, abs, 6),
        i!("???", xxx, imp, 6),
        /* 5 */ i!("BVC", bvc, rel, 2),
        i!("EOR", eor, izy, 5),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 4),
        i!("EOR", eor, zpx, 4),
        i!("LSR", lsr, zpx, 6),
        i!("???", xxx, imp, 6),
        i!("CLI", cli, imp, 2),
        i!("EOR", eor, aby, 4),
        i!("???", nop, imp, 2),
        i!("???", xxx, imp, 7),
        i!("???", nop, imp, 4),
        i!("EOR", eor, abx, 4),
        i!("LSR", lsr, abx, 7),
        i!("???", xxx, imp, 7),
        /* 6 */ i!("RTS", rts, imp, 6),
        i!("ADC", adc, izx, 6),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 3),
        i!("ADC", adc, zp0, 3),
        i!("ROR", ror, zp0, 5),
        i!("???", xxx, imp, 5),
        i!("PLA", pla, imp, 4),
        i!("ADC", adc, imm, 2),
        i!("ROR", ror, imp, 2),
        i!("???", xxx, imp, 2),
        i!("JMP", jmp, ind, 5),
        i!("ADC", adc, abs, 4),
        i!("ROR", ror, abs, 6),
        i!("???", xxx, imp, 6),
        /* 7 */ i!("BVS", bvs, rel, 2),
        i!("ADC", adc, izy, 5),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 4),
        i!("ADC", adc, zpx, 4),
        i!("ROR", ror, zpx, 6),
        i!("???", xxx, imp, 6),
        i!("SEI", sei, imp, 2),
        i!("ADC", adc, aby, 4),
        i!("???", nop, imp, 2),
        i!("???", xxx, imp, 7),
        i!("???", nop, imp, 4),
        i!("ADC", adc, abx, 4),
        i!("ROR", ror, abx, 7),
        i!("???", xxx, imp, 7),
        /* 8 */ i!("???", nop, imp, 2),
        i!("STA", sta, izx, 6),
        i!("???", nop, imp, 2),
        i!("???", xxx, imp, 6),
        i!("STY", sty, zp0, 3),
        i!("STA", sta, zp0, 3),
        i!("STX", stx, zp0, 3),
        i!("???", xxx, imp, 3),
        i!("DEY", dey, imp, 2),
        i!("???", nop, imp, 2),
        i!("TXA", txa, imp, 2),
        i!("???", xxx, imp, 2),
        i!("STY", sty, abs, 4),
        i!("STA", sta, abs, 4),
        i!("STX", stx, abs, 4),
        i!("???", xxx, imp, 4),
        /* 9 */ i!("BCC", bcc, rel, 2),
        i!("STA", sta, izy, 6),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 6),
        i!("STY", sty, zpx, 4),
        i!("STA", sta, zpx, 4),
        i!("STX", stx, zpy, 4),
        i!("???", xxx, imp, 4),
        i!("TYA", tya, imp, 2),
        i!("STA", sta, aby, 5),
        i!("TXS", txs, imp, 2),
        i!("???", xxx, imp, 5),
        i!("???", nop, imp, 5),
        i!("STA", sta, abx, 5),
        i!("???", xxx, imp, 5),
        i!("???", xxx, imp, 5),
        /* A */ i!("LDY", ldy, imm, 2),
        i!("LDA", lda, izx, 6),
        i!("LDX", ldx, imm, 2),
        i!("???", xxx, imp, 6),
        i!("LDY", ldy, zp0, 3),
        i!("LDA", lda, zp0, 3),
        i!("LDX", ldx, zp0, 3),
        i!("???", xxx, imp, 3),
        i!("TAY", tay, imp, 2),
        i!("LDA", lda, imm, 2),
        i!("TAX", tax, imp, 2),
        i!("???", xxx, imp, 2),
        i!("LDY", ldy, abs, 4),
        i!("LDA", lda, abs, 4),
        i!("LDX", ldx, abs, 4),
        i!("???", xxx, imp, 4),
        /* B */ i!("BCS", bcs, rel, 2),
        i!("LDA", lda, izy, 5),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 5),
        i!("LDY", ldy, zpx, 4),
        i!("LDA", lda, zpx, 4),
        i!("LDX", ldx, zpy, 4),
        i!("???", xxx, imp, 4),
        i!("CLV", clv, imp, 2),
        i!("LDA", lda, aby, 4),
        i!("TSX", tsx, imp, 2),
        i!("???", xxx, imp, 4),
        i!("LDY", ldy, abx, 4),
        i!("LDA", lda, abx, 4),
        i!("LDX", ldx, aby, 4),
        i!("???", xxx, imp, 4),
        /* C */ i!("CPY", cpy, imm, 2),
        i!("CMP", cmp, izx, 6),
        i!("???", nop, imp, 2),
        i!("???", xxx, imp, 8),
        i!("CPY", cpy, zp0, 3),
        i!("CMP", cmp, zp0, 3),
        i!("DEC", dec, zp0, 5),
        i!("???", xxx, imp, 5),
        i!("INY", iny, imp, 2),
        i!("CMP", cmp, imm, 2),
        i!("DEX", dex, imp, 2),
        i!("???", xxx, imp, 2),
        i!("CPY", cpy, abs, 4),
        i!("CMP", cmp, abs, 4),
        i!("DEC", dec, abs, 6),
        i!("???", xxx, imp, 6),
        /* D */ i!("BNE", bne, rel, 2),
        i!("CMP", cmp, izy, 5),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 4),
        i!("CMP", cmp, zpx, 4),
        i!("DEC", dec, zpx, 6),
        i!("???", xxx, imp, 6),
        i!("CLD", cld, imp, 2),
        i!("CMP", cmp, aby, 4),
        i!("NOP", nop, imp, 2),
        i!("???", xxx, imp, 7),
        i!("???", nop, imp, 4),
        i!("CMP", cmp, abx, 4),
        i!("DEC", dec, abx, 7),
        i!("???", xxx, imp, 7),
        /* E */ i!("CPX", cpx, imm, 2),
        i!("SBC", sbc, izx, 6),
        i!("???", nop, imp, 2),
        i!("???", xxx, imp, 8),
        i!("CPX", cpx, zp0, 3),
        i!("SBC", sbc, zp0, 3),
        i!("INC", inc, zp0, 5),
        i!("???", xxx, imp, 5),
        i!("INX", inx, imp, 2),
        i!("SBC", sbc, imm, 2),
        i!("NOP", nop, imp, 2),
        i!("???", sbc, imp, 2),
        i!("CPX", cpx, abs, 4),
        i!("SBC", sbc, abs, 4),
        i!("INC", inc, abs, 6),
        i!("???", xxx, imp, 6),
        /* F */ i!("BEQ", beq, rel, 2),
        i!("SBC", sbc, izy, 5),
        i!("???", xxx, imp, 2),
        i!("???", xxx, imp, 8),
        i!("???", nop, imp, 4),
        i!("SBC", sbc, zpx, 4),
        i!("INC", inc, zpx, 6),
        i!("???", xxx, imp, 6),
        i!("SED", sed, imp, 2),
        i!("SBC", sbc, aby, 4),
        i!("NOP", nop, imp, 2),
        i!("???", xxx, imp, 7),
        i!("???", nop, imp, 4),
        i!("SBC", sbc, abx, 4),
        i!("INC", inc, abx, 7),
        i!("???", xxx, imp, 7),
    ]
}
