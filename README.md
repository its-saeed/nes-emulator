# nes-emu

A NES emulator written in Rust, built incrementally with a test-driven approach.

## Quick Start — Snake Demo

```sh
cargo run
```

A 32×32 snake game runs on the emulated 6502 CPU. The window shows the game on the left and a live debugger panel on the right.

### Controls

| Key | Action |
|---|---|
| `W` / `A` / `S` / `D` | Move snake up / left / down / right |
| `Space` | Pause / Resume |
| `→` (Arrow Right) | Step one CPU cycle (while paused) |

### Toolbar

| Control | Description |
|---|---|
| **Resume / Pause** | Toggle execution |
| **Step** | Advance one CPU clock (enabled only when paused) |
| **Reset** | Reload the program and restart the CPU |
| **Speed** | Cycles executed per frame (1 – 1000, default 100) |
| **Zoom** | Screen scale (2× – 24×, default 10×) |

### Debugger Panel

**Summary** — always visible at the top:

| Field | Description |
|---|---|
| Ticks | Total CPU clocks since last reset |
| Speed | Current cycles-per-frame setting |
| Next | Address and disassembled next instruction (e.g. `0638  LDA #$04`) |
| Program | Loaded program size in bytes |

**CPU tab** — all registers and status flags in real time.

**Memory tab** — zero-page bytes `$00`–`$0F` in a grid.

**Program tab** — editable hex editor. Paste raw hex bytes (e.g. `A9 42 85 00`), click **Load Code** to flash the CPU, or **Restore Sample** to go back to the snake game.

## Build & Test

```sh
cargo build
cargo test
```

All emulator logic lives in `src/lib.rs` and is fully testable without a window.

## Project Structure

```
src/
  lib.rs        exports bus, cartridge, and cpu modules
  main.rs       egui/eframe debugger + snake demo
  bus.rs        CPU bus — dispatches to RAM, PPU registers, and cartridge
  cpu.rs        MOS 6502 CPU — registers, flags, instruction table, clock, disassembler
  cartridge.rs  iNES parser + Mapper 0 (NROM)
tests/
  roms/
    nestest.nes   (not committed — download separately)
    nestest.log   reference output for CPU validation
docs/
  step-01-cpu-skeleton.md
  step-02-clock-and-addressing.md
  step-03-opcodes.md
  step-04-cartridge-and-bus.md
```

## Architecture

The NES has five major components connected through a shared 16-bit address bus:

```
CPU (2A03) ──┐
PPU (2C02) ──┤
APU        ──┼── Bus (0x0000 – 0xFFFF)
Cartridge  ──┘
```

**Ownership model:** the CPU owns nothing. The bus is passed in as `&mut Bus` on every call (`cpu.clock(&mut bus)`), avoiding the circular-reference problem common in C++ emulators.

## Implementation Status

### CPU (MOS 6502)

| Group | Opcodes | Status |
|---|---|---|
| Clock, interrupts | `clock`, `reset`, `irq`, `nmi` | done |
| Addressing modes | all 12 | done |
| Flag ops | `CLC` `SEC` `CLD` `SED` `CLI` `SEI` `CLV` | done |
| Load / store | `LDA` `LDX` `LDY` `STA` `STX` `STY` | done |
| Register transfers | `TAX` `TAY` `TXA` `TYA` `TSX` `TXS` | done |
| Stack | `PHA` `PHP` `PLA` `PLP` | done |
| Increment / decrement | `INC` `DEC` `INX` `INY` `DEX` `DEY` | done |
| Logical | `AND` `ORA` `EOR` `BIT` | done |
| Shifts / rotates | `ASL` `LSR` `ROL` `ROR` | done |
| Compare | `CMP` `CPX` `CPY` | done |
| Arithmetic | `ADC` `SBC` | done |
| Branches | `BCC` `BCS` `BEQ` `BMI` `BNE` `BPL` `BVC` `BVS` | done |
| Control flow | `JMP` `JSR` `RTS` `RTI` `BRK` | done |
| Disassembler | all 12 addressing modes | done |

### PPU, APU, Cartridge

| Component | Status |
|---|---|
| Cartridge / iNES parser | done (Mapper 0 / NROM) |
| PPU | not started |
| APU | not started |

## Reference

Built against [OneLoneCoder/olcNES](https://github.com/OneLoneCoder/olcNES) as a C++ reference implementation.
