# Step 1 — CPU Skeleton

## NES Architecture Overview

The NES is built around five major components that communicate through a shared address bus:

```
 ┌─────────────────────────────────────────────┐
 │                   Bus                        │
 │  (16-bit address space, 0x0000 – 0xFFFF)    │
 └──┬──────────┬──────────┬──────────┬─────────┘
    │          │          │          │
  CPU        PPU        APU      Cartridge
 (2A03)    (2C02)              (ROM + Mapper)
```

- **CPU (2A03)** — MOS 6502 variant, no decimal mode. Runs the game logic.
- **PPU (2C02)** — Picture Processing Unit. Draws the 256×240 framebuffer.
- **APU** — Audio Processing Unit. Generates sound via five channels.
- **Cartridge** — ROM containing game code and data, plus a mapper chip that extends the address space beyond 64KB.
- **Bus** — Shared communication layer. Every read and write from any component goes through it.

We build in this order: **CPU → Bus → Cartridge → PPU → APU**. Nothing runs without the CPU, and the CPU is the easiest component to test in isolation.

---

## Project Structure

Use a lib + bin split so the emulator logic is fully testable without a window:

```
nes-emu/
  src/
    lib.rs      ← exports bus and cpu modules
    main.rs     ← thin binary entry point
    bus.rs
    cpu.rs
  docs/
  Cargo.toml
```

`Cargo.toml` needs no special configuration — Cargo auto-detects both `src/lib.rs` and `src/main.rs` in the same crate. Tests live in the lib.

```toml
[package]
name = "nes-emu"
version = "0.1.0"
edition = "2024"

[dependencies]
```

`src/lib.rs`:
```rust
pub mod bus;
pub mod cpu;
```

`src/main.rs`:
```rust
fn main() {}
```

---

## The Bus

The bus is a thin memory abstraction. At this stage it is just 64 KB of flat RAM. Later it will dispatch reads and writes to the right device (RAM, PPU registers, cartridge, etc.) based on the address range.

```rust
pub struct Bus {
    pub ram: [u8; 64 * 1024],
}

impl Bus {
    pub fn new() -> Self {
        Self { ram: [0; 64 * 1024] }
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        self.ram[addr as usize] = data;
    }
}
```

**Why `u16` addresses?** The 6502 has a 16-bit address bus, giving it a 64 KB address space. `u16` maps directly to that.

**Ownership note:** In many C++ NES emulators the bus owns the CPU and the CPU holds a raw pointer back to the bus (a circular reference). Rust does not allow this without `unsafe` or `Rc<RefCell<...>>`. We avoid the problem entirely: the CPU owns nothing. Instead of `cpu.clock()`, we call `cpu.clock(&mut bus)` and pass the bus in. Clean, zero overhead.

---

## The CPU

### Registers

The 6502 has five registers:

| Register | Width | Purpose |
|----------|-------|---------|
| `a` | 8-bit | Accumulator — all arithmetic goes through here |
| `x` | 8-bit | Index register X |
| `y` | 8-bit | Index register Y |
| `stack_ptr` | 8-bit | Stack pointer — offset into page 1 (0x0100–0x01FF) |
| `pc` | 16-bit | Program counter — address of the next instruction |
| `status` | 8-bit | Processor status — eight flags packed into one byte |

These are the only registers. The 6502 is a very minimal chip; all complexity lives in its addressing modes and instruction set.

### The Status Register

`status` is a single `u8` where each bit is a named flag:

```
 bit:  7  6  5  4  3  2  1  0
 flag: N  V  U  B  D  I  Z  C
```

| Flag | Meaning |
|------|---------|
| N | Negative — set when the result of an operation has bit 7 set |
| V | Overflow — set on signed arithmetic overflow |
| U | Unused — always 1 |
| B | Break — set when BRK instruction is executed |
| D | Decimal — unused on the NES (the 2A03 has no decimal mode) |
| I | Interrupt disable — when set, IRQ signals are ignored |
| Z | Zero — set when the result of an operation is zero |
| C | Carry — set on unsigned arithmetic overflow/underflow |

Represent this as a `#[repr(u8)]` enum so each variant holds its own bitmask and can be cast directly to `u8`:

```rust
#[repr(u8)]
pub enum Flag {
    C = 1 << 0,
    Z = 1 << 1,
    I = 1 << 2,
    D = 1 << 3,
    B = 1 << 4,
    U = 1 << 5,
    V = 1 << 6,
    N = 1 << 7,
}
```

`Flag::N as u8` gives `0b10000000`. This lets the two flag helpers stay to single bit operations:

```rust
pub fn get_flag(&self, f: Flag) -> bool {
    self.status & (f as u8) != 0
}

pub fn set_flag(&mut self, f: Flag, v: bool) {
    let mask = f as u8;
    if v {
        self.status |= mask;   // set the bit
    } else {
        self.status &= !mask;  // clear the bit
    }
}
```

`|=` sets a bit without disturbing others. `&= !mask` clears it without disturbing others.

### Internal Working State

Beyond the registers visible to programs, the CPU needs several private fields to track its own execution:

```rust
fetched: u8,      // data fetched from memory for the current instruction
addr_abs: u16,    // resolved absolute address for the current instruction
addr_rel: u16,    // relative address offset (used by branch instructions)
opcode: u8,       // the current instruction byte
cycles: u8,       // remaining clock cycles for the current instruction
```

The 6502 is a multi-cycle processor — most instructions take 2–7 clock cycles to complete. `cycles` counts down from the instruction's base cycle count, and `clock()` only executes a new instruction when it reaches zero.

### `Cpu::new()`

Initialize all fields to zero, except `status` which starts with the `U` (unused) flag set — this reflects the real hardware behaviour:

```rust
pub fn new() -> Self {
    Self {
        a: 0, x: 0, y: 0,
        stack_ptr: 0,
        pc: 0,
        status: Flag::U as u8,
        fetched: 0,
        addr_abs: 0,
        addr_rel: 0,
        opcode: 0,
        cycles: 0,
        lookup: build_lookup(),
    }
}
```

---

## The Instruction Table

### Design

The 6502 opcode is a single byte — 256 possible values. Rather than a `match` with 256 arms, we use a lookup table of 256 entries indexed directly by the opcode byte:

```
opcode byte 0xA9  →  lookup[0xA9]  →  { "LDA", lda, imm, 2 }
                                              ↑     ↑    ↑
                                          execute  addr  cycles
                                                   mode
```

Each entry holds:

```rust
pub struct Instruction {
    pub name: &'static str,               // mnemonic, for debugging
    pub operate:  fn(&mut Cpu, &mut Bus) -> u8,   // opcode handler
    pub addr_mode: fn(&mut Cpu, &mut Bus) -> u8,  // addressing mode handler
    pub cycles: u8,                       // base cycle count
}
```

### Why function pointers?

In C++, this uses member function pointers (`uint8_t (Cpu::*fn)(void)`). In Rust, methods defined as `fn lda(&mut self, bus: &mut Bus) -> u8` have the unbound function pointer type `fn(&mut Cpu, &mut Bus) -> u8`, and are referenced as `Cpu::lda`. The types match exactly.

Storing these in the table means `clock()` never branches on the opcode — it just indexes and calls.

### The field on `Cpu`

```rust
lookup: [Instruction; 256],
```

An array, not a `Vec` — the size is fixed at compile time and known to be exactly 256. The compiler will reject `build_lookup` if it returns anything other than 256 elements.

### Building the table

Use a local macro to keep each row readable:

```rust
fn build_lookup() -> [Instruction; 256] {
    macro_rules! i {
        ($name:literal, $op:ident, $am:ident, $c:literal) => {
            Instruction { name: $name, operate: Cpu::$op, addr_mode: Cpu::$am, cycles: $c }
        };
    }

    [
        /* 0x00 */ i!("BRK", brk, imm, 7),  i!("ORA", ora, izx, 6),  /* ... */
        // 16 rows × 16 columns = 256 entries
    ]
}
```

### Illegal opcodes

The 6502 has 56 official opcodes. The remaining 200 opcode values are undefined (the hardware behaviour is a side effect of the chip's internal circuitry). We handle them with two stubs:

- `nop` — used for illegal opcodes whose side effect is harmless (just waste cycles)
- `xxx` — used for truly undefined behaviour

Both return `0` for now. Separating them makes it easy to add logging later if you want to detect illegal opcode usage.

---

## Addressing Modes

Addressing modes determine *where* an instruction gets its data. The same opcode can read from different places depending on the mode. There are 12 on the 6502:

| Name | Code | Description |
|------|------|-------------|
| Implied | `imp` | Operand is implicit (e.g. `CLC` clears carry, no address needed) |
| Immediate | `imm` | Operand is the next byte in the instruction stream |
| Zero Page | `zp0` | 8-bit address into page zero (0x0000–0x00FF) — fast |
| Zero Page, X | `zpx` | Zero page address + X register |
| Zero Page, Y | `zpy` | Zero page address + Y register |
| Relative | `rel` | Signed 8-bit offset from PC (branch instructions only) |
| Absolute | `abs` | Full 16-bit address |
| Absolute, X | `abx` | 16-bit address + X register |
| Absolute, Y | `aby` | 16-bit address + Y register |
| Indirect | `ind` | 16-bit pointer — reads the actual address from that pointer |
| Indexed Indirect | `izx` | Zero-page pointer offset by X, then dereference |
| Indirect Indexed | `izy` | Zero-page pointer dereference, then offset by Y |

Each addressing mode function sets `self.addr_abs` (or `self.addr_rel` for branches) and returns an extra cycle count (0 or 1) when a page boundary is crossed. Stub them all returning `0` for now — they will be implemented in the next step.

---

## What You Should Have at the End of This Step

A project that compiles cleanly with only dead-code warnings:

```
src/
  lib.rs        pub mod bus; pub mod cpu;
  main.rs       fn main() {}
  bus.rs        Bus struct with read/write
  cpu.rs        Flag enum, Instruction struct, Cpu struct,
                get_flag/set_flag, all 12 addr mode stubs,
                all 56 opcode stubs + xxx/nop,
                build_lookup() returning [Instruction; 256]
```

```
cargo build
# → 0 errors, dead_code warnings only (expected — nothing calls these yet)
```

**Next step:** implement `clock()`, `reset()`, `irq()`, `nmi()`, and the 12 addressing modes.
