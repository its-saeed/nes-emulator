use crate::cartridge::Cartridge;

pub struct Bus {
    // 2 KB of internal RAM — mirrored across 0x0000–0x1FFF via addr & 0x07FF
    pub ram: [u8; 2048],
    // None until a cartridge is inserted
    pub cart: Option<Cartridge>,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            ram: [0; 2048],
            cart: None,
        }
    }

    // Load a program into flat RAM starting at start_addr (cart-less / test mode).
    // Addresses are masked to 2KB so all addresses stay in bounds.
    pub fn load(data: &[u8], start_addr: u16) -> Self {
        let mut bus = Self::new();
        for (i, &byte) in data.iter().enumerate() {
            let addr = start_addr.wrapping_add(i as u16);
            bus.ram[(addr & 0x07FF) as usize] = byte;
        }
        bus
    }

    pub fn insert_cartridge(&mut self, cart: Cartridge) {
        self.cart = Some(cart);
    }

    pub fn read(&self, addr: u16) -> u8 {
        // Dispatch order (cart is checked first — it can handle any address):
        //
        //   1. cart.cpu_read(addr) → Some(val): return val
        //   2. 0x0000–0x1FFF  → ram[addr & 0x07FF]   (2 KB mirrored)
        //   3. 0x2000–0x3FFF  → PPU registers (stub: return 0 for now)
        //   4. _              → open bus: return 0
        //
        // When no cartridge is loaded (cart == None), fall through to masked
        // RAM for the full address space. This keeps all CPU tests working
        // since they write to addresses like 0xFFFC (reset vector) directly.
        if let Some(cart) = &self.cart {
            if let Some(data) = cart.cpu_read(addr) {
                return data;
            }
            // cart present but didn't claim this address → dispatch to devices
            return match addr {
                0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
                0x2000..=0x3FFF => 0, // PPU stub
                _ => 0,
            };
        }
        // No cart: masked flat RAM (test mode)
        self.ram[(addr & 0x07FF) as usize]
    }

    pub fn read_u16(&self, addr: u16) -> u16 {
        self.read(addr) as u16 | (self.read(addr.wrapping_add(1)) as u16) << 8
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        // Dispatch order (cart is checked first):
        //
        //   1. cart.cpu_write(addr, data) → true: done (cart handled it)
        //   2. 0x0000–0x1FFF → ram[addr & 0x07FF] = data
        //   3. 0x2000–0x3FFF → PPU registers (stub: ignore for now)
        //   4. _             → ignore
        //
        // When no cart is loaded, write to masked flat RAM (test mode).
        if let Some(cart) = &mut self.cart {
            if cart.cpu_write(addr, data) {
                return;
            }
            match addr {
                0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = data,
                0x2000..=0x3FFF => {} // PPU stub
                _ => {}
            }
            return;
        }
        // No cart: masked flat RAM
        self.ram[(addr & 0x07FF) as usize] = data;
    }

    pub fn write_u16(&mut self, addr: u16, data: u16) {
        self.write(addr, data as u8);
        self.write(addr.wrapping_add(1), (data >> 8) as u8);
    }
}
