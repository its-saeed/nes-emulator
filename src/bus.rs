pub struct Bus {
    pub ram: [u8; 64 * 1024],
}

impl Bus {
    pub fn new() -> Self {
        Self {
            ram: [0; 64 * 1024],
        }
    }

    pub fn load(data: &[u8], start_addr: u16) -> Self {
        let mut bus = Self {
            ram: [0; 64 * 1024],
        };

        let start_addr = start_addr as usize;
        bus.ram[start_addr..(start_addr + data.len())].copy_from_slice(data);
        bus
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

    pub fn write_u16(&mut self, addr: u16, data: u16) {
        self.ram[addr as usize] = data as u8;
        self.ram[(addr + 1) as usize] = (data >> 8) as u8;
    }
}
