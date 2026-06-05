# Step 3 — Opcodes

## How Opcodes Get Their Data: `fetch()`

Before doing any arithmetic, an opcode needs its input byte. It gets it by calling `fetch()`:

```rust
let value = self.fetch(bus);
```

`fetch()` checks the current instruction's addressing mode:
- If **not** `imp`: reads one byte from `addr_abs` into `fetched` and returns it.
- If `imp`: `fetched` was already set by `imp()` (it captured `A`). Returns it unchanged.

This means opcodes never read memory directly — they always go through `fetch()`. The addressing mode already resolved the address; `fetch()` just reads it.

---

## Return Values

Every opcode returns `0` or `1`:

- `0` — no extra cycle needed regardless of addressing mode
- `1` — can accept an extra cycle if the addressing mode also returns 1

`clock()` ANDs the two return values:
```rust
self.cycles += operate_cycles & addr_mode_cycles;
```

Most opcodes return `0`. The ones that return `1` are load instructions that can tolerate a page-cross penalty: `LDA`, `LDX`, `LDY`, `EOR`, `AND`, `ORA`, `ADC`, `SBC`, `CMP`.

Store instructions (`STA`, `STX`, `STY`) always return `0` — a write must happen at the correct address regardless.

---

## The N and Z Flags

Nearly every opcode that produces a result sets the N (negative) and Z (zero) flags based on that result. Extract this into a pattern you'll use constantly:

```rust
self.set_flag(Flag::Z, result == 0);
self.set_flag(Flag::N, result & 0x80 != 0);
```

Z is set when the result is zero. N mirrors bit 7 of the result (the sign bit in two's complement).

---

## Group 1 — Flag Operations

The simplest group. Each instruction just sets or clears one flag. No memory access, no fetch, no N/Z update needed.

| Opcode | Effect |
|--------|--------|
| `CLC`  | `C = 0` |
| `SEC`  | `C = 1` |
| `CLD`  | `D = 0` |
| `SED`  | `D = 1` |
| `CLI`  | `I = 0` |
| `SEI`  | `I = 1` |
| `CLV`  | `V = 0` |

Note: there is no `SEV`. Overflow is set only by arithmetic.

All return `0`.

---

## Group 2 — Load / Store

### Load: LDA, LDX, LDY

Fetch a byte and put it in the target register. Set N and Z.

```
LDA: A = fetch(); set N, Z
LDX: X = fetch(); set N, Z
LDY: Y = fetch(); set N, Z
```

LDA, LDX, LDY return `1` (they can accept a page-cross extra cycle).

### Store: STA, STX, STY

Write a register to `addr_abs`. No flags affected.

```
STA: write(addr_abs, A)
STX: write(addr_abs, X)
STY: write(addr_abs, Y)
```

All return `0`.

---

## Group 3 — Register Transfers

Copy one register to another. Set N and Z on the result, except `TXS` which sets no flags (stack pointer writes never affect flags on the real hardware).

| Opcode | Effect |
|--------|--------|
| `TAX`  | `X = A`; set N, Z |
| `TAY`  | `Y = A`; set N, Z |
| `TXA`  | `A = X`; set N, Z |
| `TYA`  | `A = Y`; set N, Z |
| `TSX`  | `X = stack.ptr`; set N, Z |
| `TXS`  | `stack.ptr = X`; **no flags** |

All return `0`.

---

## Group 4 — Stack

### PHA / PHP — Push

```
PHA: stack.push(A)          no flags
PHP: stack.push(status | B | U)   B and U are set in the pushed byte
```

PHP pushes the status register with the B and U bits forced to 1 in the pushed value. This is a quirk of the real hardware — B is not a real flip-flop, it only appears set when status is pushed.

### PLA / PLP — Pull

```
PLA: A = stack.pop(); set N, Z
PLP: status = stack.pop(); clear B, set U
```

PLP restores the status register from the stack. The B bit is cleared and U is set in the restored value.

All return `0`.

---

## Group 5 — Increment / Decrement

Modify memory or a register by 1. Set N and Z.

```
INC: val = read(addr_abs) + 1; write(addr_abs, val); set N, Z
DEC: val = read(addr_abs) - 1; write(addr_abs, val); set N, Z
INX: X++; set N, Z
INY: Y++; set N, Z
DEX: X--; set N, Z
DEY: Y--; set N, Z
```

Use wrapping arithmetic — the 6502 wraps at 0xFF→0x00 and 0x00→0xFF.
All return `0`.

---

## Group 6 — Logical

### AND, ORA, EOR

Bitwise operations between `A` and `fetch()`. Result stored in `A`. Set N and Z.

```
AND: A = A & fetch(); set N, Z    return 1
ORA: A = A | fetch(); set N, Z    return 1
EOR: A = A ^ fetch(); set N, Z    return 1
```

### BIT

Tests bits without changing `A`. Sets flags based on the memory value and `A`:

```
val = fetch()
Z = (A & val) == 0      ← was the AND result zero?
N = val & 0x80           ← bit 7 of memory value
V = val & 0x40           ← bit 6 of memory value
```

Note: BIT sets N and V from the **memory value**, not from `A & val`. Returns `0`.

---

## Group 7 — Shifts and Rotates

These work on either the accumulator (`imp` mode) or a memory address.

### ASL — Arithmetic Shift Left
```
old_bit7 = val & 0x80
val = val << 1
C = old_bit7; set N, Z
```

### LSR — Logical Shift Right
```
old_bit0 = val & 0x01
val = val >> 1
C = old_bit0; N = 0; set Z
```

### ROL — Rotate Left through Carry
```
new_val = (val << 1) | C
C = old_bit7; set N, Z
```

### ROR — Rotate Right through Carry
```
new_val = (val >> 1) | (C << 7)
C = old_bit0; set N, Z
```

For memory targets, read the byte, modify, write it back. For accumulator (`imp`), modify `A` directly.

All return `0`.

---

## Group 8 — Compare

Subtract without storing the result. Sets C, Z, N.

```
CMP: tmp = A - fetch()
CPX: tmp = X - fetch()
CPY: tmp = Y - fetch()

C = register >= fetch()   (no borrow)
Z = (tmp & 0xFF) == 0
N = tmp & 0x80
```

Use a `u16` for the subtraction to inspect the borrow bit (`tmp & 0xFF00`).
CMP returns `1`; CPX and CPY return `0`.

---

## Group 9 — Arithmetic

### ADC — Add with Carry

```
tmp = (u16)A + (u16)fetch() + (u16)C

C = tmp > 0xFF
Z = (tmp & 0xFF) == 0
N = tmp & 0x80
V = (~(A ^ fetch()) & (A ^ tmp)) & 0x80   ← signed overflow
A = tmp & 0xFF
```

The overflow formula: V is set when the sign of the inputs are the same but the sign of the result is different.

Returns `1`.

### SBC — Subtract with Carry

SBC is ADC with the operand inverted:

```
tmp = (u16)A + (u16)(fetch() ^ 0xFF) + (u16)C
```

Same flag logic as ADC. Returns `1`.

---

## Group 10 — Branches

All branches use `rel` addressing. The pattern is:

```
if <condition>:
    cycles += 1
    addr_abs = pc + addr_rel
    if (addr_abs & 0xFF00) != (pc & 0xFF00):
        cycles += 1      ← extra cycle for page cross
    pc = addr_abs
```

Two possible extra cycles: +1 for the branch being taken, +1 more if it crosses a page. Return `0` (cycles are added directly, not through the `&` mechanism).

| Opcode | Condition |
|--------|-----------|
| `BCC`  | C == 0 |
| `BCS`  | C == 1 |
| `BEQ`  | Z == 1 |
| `BNE`  | Z == 0 |
| `BMI`  | N == 1 |
| `BPL`  | N == 0 |
| `BVC`  | V == 0 |
| `BVS`  | V == 1 |

---

## Group 11 — Control Flow

### JMP
```
pc = addr_abs
```
`abs` mode: `pc` is set directly. `ind` mode: already handled by the addressing mode.
Returns `0`.

### JSR — Jump to Subroutine
Push `pc - 1` (the last byte of the JSR instruction) to the stack, then jump:
```
stack.push(hi of pc-1)
stack.push(lo of pc-1)
pc = addr_abs
```
Returns `0`.

### RTS — Return from Subroutine
```
lo = stack.pop()
hi = stack.pop()
pc = (hi << 8 | lo) + 1
```
Returns `0`.

### RTI — Return from Interrupt
```
status = stack.pop(); clear B, set U
lo = stack.pop()
hi = stack.pop()
pc = hi << 8 | lo
```
No `+ 1` (unlike RTS) — the interrupt saved the exact return address.
Returns `0`.

### BRK — Software Interrupt
```
pc++
push hi(pc); push lo(pc)
set B = 1, U = 1
push status
set I = 1
pc = read_u16(IRQ_VECTOR)
```
Returns `0`.

---

## What You Should Have at the End of This Step

All tests passing:

```
cargo test
# → all passed
```

Key behaviors verified:
- Flag ops set/clear exactly one flag without touching others
- Load ops set N and Z correctly, including edge cases (0x00, 0x80, 0xFF)
- Store ops write to correct address, no flags changed
- Transfers set N/Z (except TXS)
- Stack push/pop round-trips correctly
- Inc/dec wraps at 0xFF/0x00
- Shifts move the evicted bit into C
- Rotates include C in both directions
- Compare sets C when no borrow, Z when equal
- ADC/SBC carry and overflow correct
- Branches add 1 cycle when taken, 2 when page crossed
- JSR/RTS round-trip restores PC correctly

**Next step:** wire up the PPU and Bus dispatch.
