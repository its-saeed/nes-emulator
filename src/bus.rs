pub struct Bus {
    pub ram: [u8; 64 * 1024],
}

impl Bus {
    pub fn new() -> Self {
        Self {
            ram: [0; 64 * 1024],
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.ram[addr as usize]
    }

    pub fn read_u16(&self, addr: u16) -> u16 {
        self.read(addr) as u16 | (self.read(addr.wrapping_add(1)) as u16) << 8
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        self.ram[addr as usize] = data;
    }
}
