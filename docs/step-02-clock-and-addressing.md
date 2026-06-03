# Step 2 — Clock, Interrupts, and Addressing Modes

## How `clock()` Works

The CPU does not execute one instruction per `clock()` call. It executes one instruction every N calls, where N is that instruction's cycle count. `cycles` counts down:

```
NOP takes 2 cycles:

call 1: cycles == 0 → fetch + execute NOP, cycles = 2, then cycles-- → cycles = 1
call 2: cycles == 1 → do nothing,                                      cycles-- → cycles = 0
call 3: cycles == 0 → fetch + execute next instruction ...
```

This matters for timing-accurate emulation. The PPU runs 3 cycles for every 1 CPU cycle, so the two components stay in sync only if the CPU accurately accounts for cycle counts.

The extra cycle logic: both the addressing mode and the opcode return 0 or 1. The extra cycle is only added when **both** return 1 — bitwise AND, not addition:

```rust
cycles += extra_cycle1 & extra_cycle2;
```

Example: `LDA abs,X` — the addressing mode returns 1 if a page boundary was crossed, and `lda` returns 1 because it can accept the extra cycle. Result: 1 extra cycle. But `STA abs,X` — the addressing mode may return 1, but `sta` always returns 0. Result: no extra cycle even if a page was crossed. This matches the real hardware.

---

## Interrupt Vectors

Three hardwired ROM addresses hold 16-bit pointers to interrupt handler routines:

```
0xFFFA–0xFFFB  →  NMI handler address
0xFFFC–0xFFFD  →  RESET handler address
0xFFFE–0xFFFF  →  IRQ handler address
```

All are little-endian: low byte first, high byte second.

**RESET** fires at power-on. The CPU reads the address at 0xFFFC/0xFFFD and jumps there. Registers are cleared, `stack_ptr` is set to `0xFD` (not 0 — reset does three phantom stack pushes internally), and 8 cycles are consumed.

**IRQ** (maskable interrupt) fires when external hardware signals the CPU. It only executes if the `I` flag is clear. It pushes `pc` and `status` to the stack, sets `I = 1` to block further IRQs, then jumps to the address at 0xFFFE/0xFFFF. Takes 7 cycles.

**NMI** (non-maskable interrupt) always fires regardless of the `I` flag. Same push sequence as IRQ but jumps to 0xFFFA/0xFFFB. Takes 8 cycles. On the NES, the PPU triggers an NMI at the start of vertical blank — this is how games know when to update graphics.

**The stack** lives at page 1: physical address = `0x0100 + stack_ptr`. It grows downward: write then decrement to push, increment then read to pop.

---

## Addressing Modes

Every instruction is 1–3 bytes. The first byte is always the opcode. The remaining bytes are the operand — but the *meaning* of that operand depends on the addressing mode. The mode is baked into the opcode byte itself (it's not a separate field).

The goal of every addressing mode function is to set `addr_abs` (or `addr_rel` for branches) so the opcode knows where to read/write data.

---

### IMP — Implied

No operand. The instruction acts on a register directly.

```
CLC   →  opcode: 0x18  (1 byte total)
```

`imp()` captures `a` into `fetched` as a convenience — some implied instructions like `PHA` (push accumulator) need the accumulator value available without reading from memory.

```
INX: increment X register. No address needed at all.
PHA: push accumulator. imp() puts A into fetched so the opcode can use it.
```

---

### IMM — Immediate

The operand IS the next byte in the instruction stream. No memory lookup needed.

```
LDA #$42  →  opcode: 0xA9, operand: 0x42  (2 bytes total)

Memory:  [0x0200]=0xA9  [0x0201]=0x42

clock() reads opcode 0xA9 from 0x0200, increments PC → PC = 0x0201
imm() is called: addr_abs = 0x0201, PC++ → PC = 0x0202
LDA reads from addr_abs (0x0201) → gets 0x42 → loads into A
```

The key: `clock()` always increments PC past the opcode byte before calling the
addressing mode. So when `imm()` runs, PC already points at the operand byte.
`imm()` captures that address then advances PC past it.

---

### ZP0 — Zero Page

Uses a 1-byte address to access the first 256 bytes of RAM (0x0000–0x00FF). Faster than absolute because it only needs one operand byte instead of two.

```
LDA $42  →  opcode: 0xA5, operand: 0x42  (2 bytes total)

zp0() reads 0x42 from PC, sets addr_abs = 0x0042
LDA reads from address 0x0042
```

The mask `& 0x00FF` ensures the address can't accidentally escape page zero even if arithmetic wraps.

---

### ZPX — Zero Page, X

Same as ZP0 but the X register is added to the address. Useful for iterating through a table in zero page.

```
LDA $20, X   →  opcode: 0xB5, operand: 0x20  (2 bytes)

If X = 0x03:
  addr_abs = (0x20 + 0x03) & 0xFF = 0x0023
  LDA reads from 0x0023

Wrap example — if X = 0xF0:
  addr_abs = (0x20 + 0xF0) & 0xFF = 0x0010  (wraps within zero page)
```

The wrap is intentional — zero page addressing always stays in page zero.

---

### ZPY — Zero Page, Y

Same as ZPX but uses Y. Mostly used with `LDX` and `STX`.

```
LDX $10, Y   →  opcode: 0xB6, operand: 0x10

If Y = 0x05:
  addr_abs = (0x10 + 0x05) & 0xFF = 0x0015
```

---

### REL — Relative

Used **only by branch instructions**. The operand is a signed 8-bit offset added to PC after the branch instruction. Range: -128 to +127 bytes from the instruction after the branch.

```
BNE $10  →  opcode: 0xD0, operand: 0x10  (2 bytes)

PC after reading operand: 0x0202
Branch taken: new PC = 0x0202 + 0x10 = 0x0212

BNE $F0  →  operand: 0xF0 (= -16 as signed)
Sign extension: addr_rel = 0xFFF0
Branch taken: new PC = 0x0202 + 0xFFF0 = 0x01F2
  (in u16 arithmetic: 0x0202 + 0xFFF0 = 0x01F2, correct)
```

The sign extension (`addr_rel |= 0xFF00` when bit 7 is set) makes the 8-bit offset work correctly in 16-bit arithmetic.

---

### ABS — Absolute

Full 16-bit address in the next two bytes, little-endian (low byte first).

```
LDA $1234  →  opcode: 0xAD, lo: 0x34, hi: 0x12  (3 bytes total)

Memory:  [0x0200]=0xAD  [0x0201]=0x34  [0x0202]=0x12

clock() reads opcode 0xAD from 0x0200, PC → 0x0201
abs() reads lo = 0x34 from 0x0201, PC → 0x0202
abs() reads hi = 0x12 from 0x0202, PC → 0x0203
addr_abs = (0x12 << 8) | 0x34 = 0x1234
```

Little-endian is the native byte order of the 6502.

---

### ABX — Absolute, X

Same as ABS then adds X. If the result crosses a page boundary (the high byte changes), an extra cycle is needed.

```
LDA $2000, X  →  opcode: 0xBD, lo: 0x00, hi: 0x20  (3 bytes)

If X = 0x01:
  base    = 0x2000
  final   = 0x2001   high byte unchanged (0x20 == 0x20) → 0 extra cycles

If X = 0xFF:
  base    = 0x2000
  final   = 0x20FF   high byte unchanged → 0 extra cycles

If X = 0x01 and base = 0x20FF:
  base    = 0x20FF
  final   = 0x2100   high byte changed (0x20 → 0x21) → 1 extra cycle
```

Page boundary check: `(addr_abs & 0xFF00) != (hi << 8)`.

---

### ABY — Absolute, Y

Same as ABX but uses Y. Same page-cross extra cycle rule.

---

### IND — Indirect

The operand is a 16-bit pointer. The CPU reads that pointer to find the actual address. Used only by `JMP`.

```
JMP ($3000)  →  opcode: 0x6C, lo: 0x00, hi: 0x30  (3 bytes)

Pointer address: 0x3000
Memory:  [0x3000]=0x78  [0x3001]=0x56
addr_abs = 0x5678
JMP jumps to 0x5678
```

**Hardware bug:** If the pointer's low byte is `0xFF`, the CPU should read the high byte from the next page (e.g. pointer at `0x30FF`, high byte at `0x3100`). But the real chip wraps within the same page — it reads the high byte from `0x3000` instead of `0x3100`:

```
Pointer address: 0x30FF
[0x30FF] = 0x80  ← low byte of target
[0x3100] = 0x60  ← high byte you'd expect (correct behaviour)
[0x3000] = 0x50  ← high byte the chip actually reads (the bug — wraps in page)

Correct behaviour:  addr_abs = (0x60 << 8) | 0x80 = 0x6080
Real hardware (bug): addr_abs = (0x50 << 8) | 0x80 = 0x5080
```

Some NES games accidentally rely on this bug. We must emulate it exactly.

---

### IZX — Indexed Indirect (Indirect, X)

The operand is an 8-bit zero-page address. Add X to it to get a zero-page pointer. Read the 16-bit target address from that pointer.

```
LDA ($20, X)  →  opcode: 0xA1, operand: 0x20  (2 bytes)

If X = 0x04:
  pointer address = (0x20 + 0x04) & 0xFF = 0x0024
  [0x0024] = 0xCD  (lo byte of target)
  [0x0025] = 0xAB  (hi byte of target)
  addr_abs = 0xABCD
  LDA reads from 0xABCD
```

Think of it as: "use X to pick which pointer in zero page to follow."

---

### IZY — Indirect Indexed (Indirect, Y)

The operand is an 8-bit zero-page address. Read the 16-bit base address from zero page. Add Y to get the final address. Page-cross check applies.

```
LDA ($40), Y  →  opcode: 0xB1, operand: 0x40  (2 bytes)

[0x0040] = 0x00  (lo byte of base address)
[0x0041] = 0x30  (hi byte of base address)
base = 0x3000

If Y = 0x10:
  addr_abs = 0x3010   no page cross → 0 extra cycles

If Y = 0x01 and base = 0x30FF:
  addr_abs = 0x3100   page cross → 1 extra cycle
```

Think of it as: "follow the fixed pointer at zero page, then use Y as an array index."

---

### IZX vs IZY — The Common Confusion

```
IZX: (addr + X) → pointer → target      X selects the pointer
IZY:  addr → pointer → (target + Y)     Y offsets the result
```

IZX is used when you have a table of pointers and X selects which one.
IZY is used when you have a single pointer and Y is an array index into the data it points to.

---

## What You Should Have at the End of This Step

All 30 tests passing:

```
cargo test
# → 30 passed
```

Key things verified by the tests:
- `reset()` reads the reset vector and initialises registers correctly
- `irq()` respects the I flag, pushes 3 bytes to stack, jumps to IRQ vector
- `nmi()` always fires, jumps to NMI vector
- All 12 addressing modes set `addr_abs`/`addr_rel` correctly
- Page-cross cases return 1, non-page-cross return 0
- `ind` page-boundary bug emulated correctly
- `clock()` advances PC, counts cycles down, executes only when cycles == 0

**Next step:** implement the 56 opcodes.
