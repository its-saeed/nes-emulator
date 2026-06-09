# Step 4 — Cartridge and Bus Dispatch

## Overview

So far the bus is 64 KB of flat RAM. Every read and write hits `ram[addr]` regardless of address. That was fine for CPU testing, but to run a real ROM the bus must behave like real hardware: route each address range to the right device.

This step adds:
1. **iNES parser** — reads a `.nes` file into structured ROM data
2. **Cartridge** — holds PRG-ROM, CHR-ROM, and mapper logic
3. **Bus dispatch** — routes reads/writes to RAM, cartridge, or (later) PPU by address range
4. **Mapper 0 (NROM)** — the simplest mapper; enough to run nestest and most early NES games

---

## The NES Address Space

The 6502 has a 16-bit address bus: 64 KB total. That space is not all RAM — it is partitioned between several devices.

```
Address Range   Size    Device
─────────────────────────────────────────────────────────────
0x0000–0x07FF   2 KB    Internal RAM
0x0800–0x1FFF   6 KB    RAM mirror (repeats 0x0000–0x07FF × 3)
0x2000–0x2007   8 B     PPU registers
0x2008–0x3FFF   8 KB    PPU register mirror (repeats every 8 bytes)
0x4000–0x4017   24 B    APU and I/O registers
0x4018–0x401F   8 B     APU/IO test mode (unused in normal operation)
0x4020–0x5FFF   8 KB    Cartridge space (mapper-dependent, often unused)
0x6000–0x7FFF   8 KB    PRG-RAM / battery-backed save RAM (mapper-dependent)
0x8000–0xBFFF   16 KB   PRG-ROM bank 0
0xC000–0xFFFF   16 KB   PRG-ROM bank 1 (or mirror of bank 0)
─────────────────────────────────────────────────────────────
```

Visually:

```
0xFFFF ┤████████████████████████████┤ PRG-ROM (upper bank / mirror)
0xC000 ┤                            │
0xBFFF ┤████████████████████████████┤ PRG-ROM (lower bank)
0x8000 ┤                            │
0x7FFF ┤░░░░░░░░░░░░░░░░░░░░░░░░░░░░┤ PRG-RAM / SRAM (optional)
0x6000 ┤                            │
0x5FFF ┤····························┤ Cartridge expansion (mapper-dependent)
0x4020 ┤                            │
0x401F ┤────────────────────────────┤ APU/IO test
0x4018 ┤                            │
0x4017 ┤▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒┤ APU and I/O
0x4000 ┤                            │
0x3FFF ┤▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓┤ PPU register mirror
0x2008 ┤                            │
0x2007 ┤▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓┤ PPU registers (8 bytes)
0x2000 ┤                            │
0x1FFF ┤▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒┤ RAM mirror (×3)
0x0800 ┤                            │
0x07FF ┤████████████████████████████┤ Internal RAM (2 KB)
0x0000 ┘                            │
```

---

## The iNES File Format

NES ROMs are distributed as `.nes` files in the iNES format. Every file starts with a 16-byte header followed by the ROM data.

### Header Layout

```
Byte   Field
────────────────────────────────────────────────────
0–3    Magic: 0x4E 0x45 0x53 0x1A  ("NES" + EOF)
4      PRG-ROM size in 16 KB units  (e.g. 1 = 16 KB, 2 = 32 KB)
5      CHR-ROM size in 8 KB units   (0 = uses CHR-RAM instead)
6      Flags 6  (see below)
7      Flags 7  (see below)
8      PRG-RAM size in 8 KB units   (0 = assume 8 KB)
9–15   Unused / iNES 2.0 fields
────────────────────────────────────────────────────
```

### Flags 6

```
Bit  7 6 5 4   3    2    1    0
     ┌─────┐   │    │    │    └── Mirroring: 0=horizontal, 1=vertical
     │Mapper│   │    │    └─────── Battery-backed PRG-RAM present
     │lo nibble│    └──────────── Trainer present (512-byte block before PRG)
     └─────┘   └───────────────── Four-screen VRAM
```

### Flags 7

```
Bit  7 6 5 4   3    2    1    0
     ┌─────┐   └────┴────┴────── VS/Playchoice/NES 2.0 indicator
     │Mapper│
     │hi nibble
     └─────┘
```

### Mapper Number

The mapper ID is 8 bits split across two bytes:
```
mapper = (flags7 & 0xF0) | (flags6 >> 4)
```

### Data Layout After Header

```
[16-byte header]
[512-byte trainer, if flags6 bit 2 is set — skip it]
[PRG-ROM: prg_banks × 16384 bytes]
[CHR-ROM: chr_banks × 8192 bytes]
```

### Parsing in Rust

```rust
pub struct CartridgeHeader {
    pub prg_banks: u8,    // number of 16 KB PRG-ROM banks
    pub chr_banks: u8,    // number of 8 KB CHR-ROM banks (0 = CHR-RAM)
    pub mapper_id: u8,
    pub mirroring: Mirroring,
    pub has_battery: bool,
}

// validate magic bytes: data[0..4] == [0x4E, 0x45, 0x53, 0x1A]
// skip 512-byte trainer if flags6 bit 2 is set
// read prg_banks × 16384 bytes → prg_rom: Vec<u8>
// read chr_banks × 8192 bytes  → chr_rom: Vec<u8>
//   if chr_banks == 0: allocate 8192 bytes of CHR-RAM instead
```

---

## Mappers

Most NES cartridges contain more ROM than the 32 KB address window at `0x8000–0xFFFF` can hold. A **mapper** is a chip on the cartridge that can swap ("bank switch") which section of ROM is currently visible in the CPU's address space.

```
Without mapper (NROM, 32 KB):         With mapper (e.g. MMC1, 256 KB+):

CPU address space                      CPU address space
┌─────────────┐ 0xFFFF                 ┌─────────────┐ 0xFFFF
│  PRG bank 1 │                        │  PRG bank N │ ← switchable
│   (fixed)   │                        │             │
├─────────────┤ 0xC000                 ├─────────────┤ 0xC000
│  PRG bank 0 │                        │  PRG bank 0 │ ← often fixed
│   (fixed)   │                        │             │
└─────────────┘ 0x8000                 └─────────────┘ 0x8000
       │                                      │
       ▼                                      ▼
  ROM (32 KB)                           ROM (256+ KB)
  ┌──────────┐                          ┌──────────┐
  │ bank 1   │ 0x8000–0xBFFF            │ bank 0   │
  │ bank 0   │ 0xC000–0xFFFF            │ bank 1   │
  └──────────┘                          │ bank 2   │
                                        │ ...      │
                                        │ bank N   │
                                        └──────────┘
```

Over 200 different mappers exist. For this step we only implement **Mapper 0 (NROM)** — it has no bank switching at all and covers nestest plus many early NES games.

---

## Mapper 0 — NROM

NROM is the simplest possible mapper: no switching, just a direct window into ROM.

### PRG-ROM mapping

| PRG-ROM size | 0x8000–0xBFFF | 0xC000–0xFFFF |
|---|---|---|
| 16 KB (1 bank) | bank 0 | bank 0 (mirror) |
| 32 KB (2 banks) | bank 0 | bank 1 |

For a 16 KB ROM, the same 16 KB appears twice. The reset vector at `0xFFFC`/`0xFFFD` lands in the mirrored copy — this is intentional.

Address translation for reads:
```
addr in 0x8000–0xFFFF:
  offset = addr & (prg_rom.len() - 1)
  return prg_rom[offset]
```

The mask `prg_rom.len() - 1` is either `0x3FFF` (16 KB) or `0x7FFF` (32 KB), which handles the mirror automatically.

### CHR-ROM mapping

CHR-ROM maps directly into the PPU address space at `0x0000–0x1FFF`. The CPU never reads CHR directly — that's the PPU's job, covered in a later step. For now, store it but don't wire it up.

---

## The Cartridge Struct

```rust
pub struct Cartridge {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper_id: u8,
    pub prg_banks: u8,
    pub chr_banks: u8,
    pub mirroring: Mirroring,
}

#[derive(Clone, Copy)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

impl Cartridge {
    pub fn from_bytes(data: &[u8]) -> Result<Self, &'static str> { ... }

    // CPU read: handles 0x8000–0xFFFF
    pub fn cpu_read(&self, addr: u16) -> Option<u8> { ... }

    // CPU write: most mappers ignore writes to ROM;
    // some (e.g. MMC1) use writes to ROM addresses as bank-switch commands
    pub fn cpu_write(&mut self, addr: u16, data: u8) -> bool { ... }
}
```

`cpu_read` returns `Option<u8>`: `Some(val)` if the cartridge handles this address, `None` if it doesn't (so the bus can try elsewhere).

---

## Bus Dispatch

The bus `read` and `write` methods become a dispatch table keyed on address range:

```
Bus::read(addr):

  0x0000–0x1FFF  → RAM (addr & 0x07FF — mirrors every 2 KB)
  0x2000–0x3FFF  → PPU registers (addr & 0x0007 — mirrors every 8 bytes)
  0x4000–0x4015  → APU registers          ← stub for now
  0x4016         → Controller 1           ← stub for now
  0x4017         → Controller 2           ← stub for now
  0x8000–0xFFFF  → Cartridge PRG-ROM
  _              → open bus (return 0)
```

```
Bus::write(addr, data):

  0x0000–0x1FFF  → RAM (addr & 0x07FF)
  0x2000–0x3FFF  → PPU registers (addr & 0x0007)
  0x4000–0x4015  → APU registers          ← stub for now
  0x4016         → Controller strobe      ← stub for now
  0x8000–0xFFFF  → Cartridge (mapper may act on writes)
  _              → ignore
```

### RAM Mirroring

The NES only has 2 KB of RAM at `0x0000–0x07FF`, but the address range `0x0000–0x1FFF` is 8 KB. The hardware mirrors the 2 KB three more times:

```
0x0000–0x07FF  actual RAM
0x0800–0x0FFF  mirror of 0x0000–0x07FF
0x1000–0x17FF  mirror of 0x0000–0x07FF
0x1800–0x1FFF  mirror of 0x0000–0x07FF
```

Mask: `addr & 0x07FF` always resolves to the real address.

### Updated Bus Struct

```rust
pub struct Bus {
    pub ram: [u8; 2048],           // was 64 KB, now correctly 2 KB
    pub cart: Option<Cartridge>,   // None until a ROM is loaded
}
```

---

## The Full Data Flow

Here is how a CPU read travels through the system once bus dispatch is wired up:

```
cpu.clock(&mut bus)
    │
    ▼
clock() calls addr_mode (e.g. abs())
    │
    └─► bus.read(0xAD34)
            │
            ▼
        addr in 0x8000–0xFFFF?
            │  yes
            ▼
        cart.cpu_read(0xAD34)
            │
            └─► offset = 0xAD34 & 0x7FFF = 0x2D34
                return prg_rom[0x2D34]
```

```
cpu.clock(&mut bus)
    │
    ▼
clock() calls addr_mode (e.g. zp0())
    │
    └─► bus.read(0x0042)
            │
            ▼
        addr in 0x0000–0x1FFF?
            │  yes
            ▼
        ram[0x0042 & 0x07FF]
        = ram[0x0042]
```

---

## Running nestest

nestest is a self-contained CPU test ROM. It does not need a PPU — just the CPU, RAM, and PRG-ROM on the bus.

### Setup

1. Load `nestest.nes` — it is Mapper 0, 1 PRG-ROM bank (16 KB)
2. Set `cpu.pc = 0xC000` directly (bypass reset vector)
3. Set `cpu.stack_ptr = 0xFD` (match the reference log)
4. Run `clock()` in a loop

### Output mechanism

nestest reports results through two memory-mapped registers that don't exist on real hardware — they are only meaningful when running under a test harness:

```
0x6000  result code: 0x00 = all tests passed, non-zero = failed test number
0x6001  reset trigger: write 0xFF to reset the machine
0x6004+ null-terminated status string
```

Your bus can route `0x6000–0x7FFF` to a small buffer in the cartridge struct or a dedicated array in the bus. After each clock, check `bus.read(0x6000)`:
- `0x80` — tests still running
- `0x00` — all passed
- anything else — that test number failed

### Comparing against the reference log

The reference log `nestest.log` looks like:

```
C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD CYC:  0
C5F5  A2 00     LDX #$00                        A:00 X:00 Y:00 P:24 SP:FD CYC:  9
C5F7  86 00     STX $00                         A:00 X:00 Y:00 P:26 SP:FD CYC: 12
...
```

Each line: `PC  bytes  disassembly  A X Y P(status) SP(stack ptr)  CYC(total cycles)`

To validate your emulator:
1. Before each `clock()`, record `pc`, `a`, `x`, `y`, `status`, `stack.ptr`, and total cycle count
2. Format it to match the log
3. Diff against `nestest.log`

You don't need a disassembler to diff — just the register values and cycle count are enough to catch errors.

---

## Mirroring

Mirroring affects the PPU's nametable layout (covered in the PPU step), not the CPU. Store the mirroring mode in the cartridge but don't act on it yet.

| Mode | Nametable arrangement |
|---|---|
| Horizontal | Top/bottom share; left/right are independent |
| Vertical | Left/right share; top/bottom are independent |
| Four-screen | All four nametables are independent (rare) |

---

## What You Should Have at the End of This Step

```
src/
  cartridge.rs   Cartridge struct, iNES parser, Mapper 0 cpu_read/cpu_write
  bus.rs         Dispatches by address range; ram is now 2 KB
  lib.rs         pub mod cartridge; added
```

Key behaviors verified:
- iNES header parsed correctly (magic, bank counts, mapper id)
- Trainer skipped when present
- 16 KB PRG-ROM mirrored correctly at 0x8000 and 0xC000
- 32 KB PRG-ROM fills 0x8000–0xFFFF without mirroring
- RAM reads/writes correctly masked to 2 KB
- Bus routes 0x8000–0xFFFF to cartridge, 0x0000–0x1FFF to RAM

Stretch goal:
- nestest passing (all 8991 lines match `nestest.log`)

**Next step:** PPU — rendering the 256×240 framebuffer.
