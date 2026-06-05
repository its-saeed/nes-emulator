# nes-emu

A NES emulator written in Rust, built incrementally with a test-driven approach.

## Build & Test

```sh
cargo build
cargo test
```

No external dependencies. The library is fully testable without a window — all emulator logic lives in `src/lib.rs` and can be exercised via `cargo test`.

## Project Structure

```
src/
  lib.rs      exports bus and cpu modules
  main.rs     thin binary entry point (empty for now)
  bus.rs      64 KB flat address space; will dispatch to devices by address range
  cpu.rs      MOS 6502 CPU — registers, flags, instruction table, clock
docs/
  step-01-cpu-skeleton.md
  step-02-clock-and-addressing.md
  step-03-opcodes.md
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
| Arithmetic | `ADC` `SBC` | in progress |
| Branches | `BCC` `BCS` `BEQ` `BMI` `BNE` `BPL` `BVC` `BVS` | pending |
| Control flow | `JMP` `JSR` `RTS` `RTI` `BRK` | pending |

### PPU, APU, Cartridge

Not yet started.

## Reference

Built against [OneLoneCoder/olcNES](https://github.com/OneLoneCoder/olcNES) as a C++ reference implementation.
