# Step 05 — The PPU: How a Frame Is Built

## Why This Is Hard to Understand

The PPU is confusing because it runs **at the same time as the CPU**, produces output
**one pixel at a time**, and the two chips communicate through a tiny set of shared
registers rather than shared memory. Most explanations jump straight into registers and
bit fields. This document starts from the TV screen and works backwards.

---

## 1. The Television

The NES was designed to drive a 1970s NTSC CRT television. Understanding how CRTs work
is the key to understanding why the PPU is designed the way it is.

A CRT has one electron gun. The gun fires electrons at the phosphor coating on the inside
of the screen, which glows where it is hit. The gun can only be in **one place at a time**.
It draws the picture by sweeping left to right across the screen, row by row:

```
 gun travels →→→→→→→→→→→→→→→→→→→→→→→→→→→→→→→
 ┌──────────────────────────────────────────┐
 │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ↵ scanline 0
 │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ↵ scanline 1
 │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ↵ scanline 2
 │                  ...
 │▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ↵ scanline 239
 │
 │         ↑ beam off, gun flies back to top-left
 └──────────────────────────────────────────┘
```

At the end of each row the gun jumps back to the left — **horizontal blank (H-blank)**.
At the end of the last row it flies back to the top-left corner —
**vertical blank (V-blank)**. During both blanks the beam is switched off so you do not
see the return path.

The PPU mimics this process exactly. It does not have a framebuffer. It does not draw the
whole picture and then display it. It produces **one pixel per clock cycle**, in the same
left-to-right, top-to-bottom order the TV gun scans, in real time.

---

## 2. Why 256 × 240?

The visible NES screen is **256 pixels wide and 240 pixels tall**. This is not an
arbitrary choice — it is the largest rectangle that fits inside the NTSC signal timing
budget while leaving enough blanking time for the CPU to do useful work.

| Dimension | Why |
|---|---|
| 256 wide | 8 tiles × 32 columns. 256 = 32 × 8. Fits neatly in the NTSC horizontal timing. |
| 240 tall | 8 tiles × 30 rows. Many TVs overscan the top and bottom 8px, so the effective visible area is often 224 or 232 lines. |

But the PPU does not just output 256 × 240 clock cycles. It spends **extra cycles per
row** on blanking and tile prefetching, and extra rows per frame on vertical blanking.
The full frame is:

```
341 dots per scanline × 262 scanlines per frame = 89,342 PPU clocks per frame
```

The PPU runs at **3× the CPU clock speed** (5.37 MHz vs 1.79 MHz), so:

```
89,342 PPU clocks ÷ 3 = ~29,780 CPU cycles per frame
```

That 29,780 is the CPU's entire budget per frame at 60 fps.

### Full scanline breakdown

```
 scanline  -1 (261)   pre-render — clears leftover sprite state, fills shift registers
 scanlines  0–239     visible picture  ← 240 rows × 256 pixels
 scanline   240       idle (PPU does nothing)
 scanlines 241–260    vertical blank  ← ~1340 CPU cycles of free time
```

### Full dot breakdown per scanline

```
 dot   0           idle
 dots  1–256       render pixels (256 visible pixels)
 dots 257–320      sprite evaluation for next scanline
 dots 321–336      prefetch first two tiles of next scanline
 dots 337–340      garbage fetches (hardware quirk)
```

---

## 3. Two Separate Buses

The CPU and PPU do not share memory. They each have their own 16-bit address space:

```
┌─────────────────────────────────────────────────────────────────┐
│                         CARTRIDGE                               │
│   PRG-ROM (program)              CHR-ROM (tile graphics)        │
│       │                                  │                      │
└───────┼──────────────────────────────────┼──────────────────────┘
        │                                  │
   CPU BUS (16-bit)                  PPU BUS (14-bit)
   0x0000–0xFFFF                    0x0000–0x3FFF
        │                                  │
   ┌────┴────┐                        ┌────┴────┐
   │   CPU   │◄──── NMI ─────────────►│   PPU   │
   │  2A03   │                        │  2C02   │
   └─────────┘                        └─────────┘
```

### CPU address space

| Range | What lives there |
|---|---|
| `0x0000–0x07FF` | 2 KB internal RAM (mirrored to 0x1FFF) |
| `0x2000–0x2007` | PPU registers (8 bytes, mirrored to 0x3FFF) |
| `0x4000–0x4017` | APU and joypad registers |
| `0x8000–0xFFFF` | PRG-ROM from cartridge |

### PPU address space

| Range | What lives there |
|---|---|
| `0x0000–0x0FFF` | Pattern table 0 (left half of CHR — from cartridge) |
| `0x1000–0x1FFF` | Pattern table 1 (right half of CHR — from cartridge) |
| `0x2000–0x27FF` | Nametable 0 and 1 (2 KB VRAM on NES mainboard) |
| `0x2800–0x2FFF` | Nametables 2 and 3 (mirrored from 0/1 in most games) |
| `0x3F00–0x3F1F` | Palette RAM (32 bytes) |

The CPU can **never directly read or write PPU memory**. It must go through the PPU's
eight memory-mapped registers. The PPU, in turn, never touches CPU memory.

---

## 4. What Is a Tile?

All NES graphics are built from **8×8 pixel tiles**. Every background tile and every
sprite is exactly 8×8 pixels.

A tile's pixel data is stored in **two bit planes** in CHR-ROM. Each plane is 8 bytes
(one byte per row). The two planes are interleaved in memory:

```
CHR address 0x0000:  plane 0, row 0   ← low bit of each pixel
CHR address 0x0001:  plane 0, row 1
...
CHR address 0x0007:  plane 0, row 7
CHR address 0x0008:  plane 1, row 0   ← high bit of each pixel
CHR address 0x0009:  plane 1, row 1
...
CHR address 0x000F:  plane 1, row 7
```

For each pixel, the PPU combines bit N from plane 0 and bit N from plane 1 to get a
2-bit color index (0–3):

```
plane 0 byte: 0b01000001   →  pixel bits: 0 1 0 0 0 0 0 1
plane 1 byte: 0b11000010   →  pixel bits: 1 1 0 0 0 0 1 0

combined:                      pixel indices: 2 3 0 0 0 0 1 2
                                              (each 0–3)
```

Color index 0 is always transparent (background color). Indices 1–3 pick from a 3-color
subpalette chosen by the attribute byte.

One tile = 16 bytes. The pattern tables hold 256 tiles each (4 KB).

---

## 5. What Is a Nametable?

The nametable is a **32×30 grid of tile indices**. Each byte says "draw tile number N
here". That is the entire background map for one screen.

```
Nametable (32 × 30 = 960 bytes of tile indices):

  col: 0    1    2   ...  31
row 0: [12] [00] [3F] ... [00]   ← tile 12 at top-left, tile 0 next, etc.
row 1: [00] [00] [00] ... [00]
...
row 29:[00] [00] [00] ... [00]
```

After the 960 tile bytes, 64 more bytes hold the **attribute table** — palette
assignments for 2×2 blocks of tiles (one attribute byte covers a 32×32 pixel area).

Total nametable size: 960 + 64 = **1024 bytes** (1 KB).

The NES has **2 KB of VRAM** on the mainboard, enough for two nametables. Games use
mirroring (horizontal or vertical, controlled by the cartridge) to map the four logical
nametable addresses to those two physical nametables.

---

## 6. How the PPU Renders One Pixel

On every visible dot, the PPU outputs exactly one pixel. Here is what it does — all
happening within a single PPU clock:

```
1. Read shift registers (loaded with tile data during previous fetches)
       ↓
2. Combine plane 0 bit + plane 1 bit → 2-bit tile color index (0–3)
       ↓
3. Look up 2-bit attribute data → which of 4 subpalettes to use
       ↓
4. Index into palette RAM: subpalette base + color index → palette entry (0–63)
       ↓
5. Look up palette entry in the 64-color NES palette → RGB
       ↓
6. Output pixel to video signal
```

Every 8 dots (one tile column), the PPU fetches data for the next tile:

```
dots 1–2:   fetch nametable byte (which tile?)
dots 3–4:   fetch attribute byte (which palette?)
dots 5–6:   fetch CHR low plane byte (tile bitmap low bits)
dots 7–8:   fetch CHR high plane byte (tile bitmap high bits)
            → load all four into shift registers
```

This is why the PPU bus exists separately: the PPU is doing **4 memory fetches per 8
pixels**, all on its own bus, while the CPU is running game logic on its bus simultaneously.

---

## 7. How CPU and PPU Run Together

This is the part that confuses most people. The two chips **never take turns**. They run
at the same time, every cycle.

```
PPU clock:  1  2  3  4  5  6  7  8  9  10  11  12  ...
CPU clock:  .     .     .     .     .      .      .  ...
            ↑           ↑           ↑             ↑
         CPU tick    CPU tick    CPU tick       CPU tick
         (every 3    PPU dots)
```

The CPU gets one clock for every three PPU dots. In a full frame:

```
PPU:  89,342 dots  (341 × 262)
CPU:  ~29,780 cycles  (89,342 ÷ 3)
```

### What each chip does during a frame

```
┌─────────────────────────────────────────────────┐
│  Scanlines 0–239: VISIBLE PICTURE               │
│                                                 │
│  PPU: fetches tiles, outputs pixels             │
│  CPU: runs game logic (AI, physics, input)      │
│       MUST NOT touch PPU memory here or         │
│       the picture will glitch                   │
├─────────────────────────────────────────────────┤
│  Scanline 240: IDLE                             │
│                                                 │
│  PPU: does nothing                              │
│  CPU: running                                   │
├─────────────────────────────────────────────────┤
│  Scanlines 241–260: VERTICAL BLANK (~1340 CPU   │
│                     cycles)                     │
│                                                 │
│  PPU: fires NMI to CPU on dot 1 of scanline 241 │
│       screen is done, beam is off               │
│                                                 │
│  CPU: jumps to NMI handler                      │
│       ← upload new nametable data               │
│       ← update scroll position                  │
│       ← move sprites (write OAM)                │
│       ← must finish before scanline 0 starts!   │
└─────────────────────────────────────────────────┘
```

The NMI is the heartbeat. Every frame, the PPU says "screen done" and the CPU gets a
short window (~1340 cycles) to prepare everything for the next frame. Miss the window
and the game tears or stutters.

### The communication channel: PPU registers

Since the two chips cannot share memory, the CPU controls the PPU through 8 registers
mapped into CPU address space at `0x2000–0x2007`:

| Register | Address | Direction | Purpose |
|---|---|---|---|
| PPUCTRL | `0x2000` | write | NMI enable, sprite size, background/sprite pattern table, VRAM increment |
| PPUMASK | `0x2001` | write | Show background, show sprites, clip edges |
| PPUSTATUS | `0x2002` | read | V-blank flag, sprite overflow, sprite 0 hit |
| OAMADDR | `0x2003` | write | Set OAM (sprite) write address |
| OAMDATA | `0x2004` | read/write | Read/write one byte of OAM |
| PPUSCROLL | `0x2005` | write ×2 | Set X and Y scroll position |
| PPUADDR | `0x2006` | write ×2 | Set VRAM address for PPUDATA access |
| PPUDATA | `0x2007` | read/write | Read/write one byte of PPU memory (auto-increments address) |

A game uploads a nametable like this:

```
; set VRAM address to nametable 0 (0x2000)
LDA #$20
STA $2006      ; high byte
LDA #$00
STA $2006      ; low byte

; write 960 tile bytes
LDX #$00
loop:
  LDA tile_data, X
  STA $2007    ; auto-increments VRAM address after each write
  INX
  CPX #$C0     ; 192 bytes = 3 × 64
  BNE loop
```

This only works during vblank. If the CPU writes to `0x2007` while the PPU is rendering,
it corrupts the tile being drawn.

---

## 8. Sprites

The NES has a separate sprite system running in parallel with the background. Up to
**64 sprites** can be defined, but only **8 sprites per scanline** can be displayed (a
hardware limit — exceeding it causes "sprite flicker" visible in many games).

Sprite data lives in **OAM (Object Attribute Memory)** — 256 bytes inside the PPU,
not in VRAM. Each sprite is 4 bytes:

```
byte 0: Y position (top of sprite, minus 1)
byte 1: tile index into pattern table
byte 2: attributes (palette, flip H/V, priority)
byte 3: X position (left of sprite)
```

The CPU uploads sprites by writing to OAM through the OAMADDR/OAMDATA registers, or
more commonly via **OAM DMA** — a single write to `0x4014` triggers a hardware DMA
that copies 256 bytes from CPU RAM to OAM in 513 CPU cycles.

During H-blank (dots 257–320 of each scanline), the PPU scans OAM, finds up to 8
sprites that overlap the next scanline, and loads their tile data into secondary OAM.
On the next scanline those sprites are drawn on top of (or behind) the background
depending on their priority bit.

---

## 9. Palette

The NES can display **25 colors simultaneously** from a master palette of **64 colors**.

The palette RAM in the PPU holds:
- **16 bytes** for the background: 4 subpalettes × 4 colors (color 0 of each is shared
  as the universal background color)
- **16 bytes** for sprites: 4 subpalettes × 4 colors (color 0 is transparent)

The 64-color master palette is hardcoded in the PPU — there is no way to change it.
Each entry is an analog voltage level, not an RGB value. Emulators use a lookup table
to map the 64 NES palette entries to RGB.

```
NES palette entry (0x00–0x3F) → lookup table → (R, G, B)
```

---

## 10. The Full Picture: One Frame Top to Bottom

```
                    CPU                         PPU
                     │                           │
frame start ─────────┼───────────────────────────┼─────────────
                     │                           │
scanline 0 ──────────┤  running game logic        ├── fetching tiles,
                     │  (can read joypad,         │   outputting pixels
                     │  update game state)        │   DOT by DOT
                     │                           │
                     │  ← CANNOT touch PPU       │
                     │    memory here →           │
                     │                           │
scanlines 1-238 ─────┤  same                      ├── same
                     │                           │
scanline 239 ────────┤                            ├── last visible row
                     │                           │
scanline 240 ────────┤  running                   ├── idle
                     │                           │
scanline 241 ────────┤                            ├── sets VBLANK flag
                     │  ◄── NMI fires ────────────┤   in PPUSTATUS
                     │                           │
                     │  NMI handler runs:         │
                     │  - write nametable data    │
                     │  - set scroll              │
                     │  - OAM DMA (sprites)       │
                     │  - RTI                     │
                     │                           │
scanlines 242-260 ───┤  back to game logic        ├── still in vblank
                     │                           │
scanline 261 ────────┤                            ├── pre-render:
                     │                           │   clears VBLANK flag,
                     │                           │   loads first tiles
frame end ───────────┼───────────────────────────┼─────────────
                     │                           │
                     └── next frame starts ───────┘
```

---

## 11. Implementation Order

Now that you understand how the PPU works, here is the order we will implement it:

| Phase | What | Why first |
|---|---|---|
| 1 | PPU struct + 8 registers | CPU needs to write to them immediately |
| 2 | PPU bus + VRAM | PPU needs somewhere to read nametables/palettes |
| 3 | Scanline renderer | Background tiles from nametable + CHR |
| 4 | NMI | Games hang without it |
| 5 | Sprites (OAM) | Layered on top of background |

After phase 3 you can load a real NES ROM and see a static background image.
After phase 4 the game actually runs (Mario walks, enemies move).
After phase 5 everything is visible.
