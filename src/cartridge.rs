// iNES magic bytes: "NES" followed by MS-DOS EOF (0x1A)
const INES_MAGIC: [u8; 4] = [0x4E, 0x45, 0x53, 0x1A];
const HEADER_SIZE: usize = 16;
const TRAINER_SIZE: usize = 512;
const PRG_BANK_SIZE: usize = 16384; // 16 KB
const CHR_BANK_SIZE: usize = 8192; // 8 KB

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

pub struct Cartridge {
    pub mirroring: Mirroring,
    pub mapper_id: u8,
    pub prg_banks: u8,
    pub chr_banks: u8,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>, // 0 chr_banks → CHR-RAM (zeroed, writable)
}

impl Cartridge {
    pub fn from_bytes(data: &[u8]) -> Result<Self, &'static str> {
        // validate: data must be at least HEADER_SIZE bytes
        // validate: data[0..4] == INES_MAGIC
        // prg_banks = data[4]
        // chr_banks = data[5]
        // mapper1   = data[6]  (flags 6)
        // mapper2   = data[7]  (flags 7)
        // mapper_id = (mapper2 & 0xF0) | (mapper1 >> 4)
        // mirroring: if mapper1 & 0x08 != 0 → FourScreen
        //            else if mapper1 & 0x01 != 0 → Vertical
        //            else Horizontal
        // trainer present: mapper1 & 0x04 != 0 → skip TRAINER_SIZE bytes after header
        // prg_rom: read prg_banks * PRG_BANK_SIZE bytes starting after header (+trainer)
        // chr_rom: read chr_banks * CHR_BANK_SIZE bytes
        //          if chr_banks == 0: allocate CHR_BANK_SIZE bytes of zeroed CHR-RAM
        // only mapper 0 is supported; return Err("unsupported mapper") for anything else
        if data.len() < HEADER_SIZE {
            return Err("too short");
        }
        if data[0..4] != INES_MAGIC {
            return Err("invalid magic");
        }

        todo!()
    }

    // CPU bus read — Mapper 0:
    //   0x8000–0xFFFF → prg_rom[addr & mask]
    //   mask = 0x3FFF if prg_banks == 1 (16 KB, upper half mirrors lower)
    //   mask = 0x7FFF if prg_banks >= 2 (32 KB, no mirror)
    //   returns None for any address outside 0x8000–0xFFFF
    pub fn cpu_read(&self, addr: u16) -> Option<u8> {
        todo!()
    }

    // CPU bus write — Mapper 0 has no writable PRG; always returns false
    pub fn cpu_write(&mut self, _addr: u16, _data: u8) -> bool {
        false
    }

    // PPU bus read — Mapper 0:
    //   0x0000–0x1FFF → chr_rom[addr]  (works for both CHR-ROM and CHR-RAM)
    //   returns None for any address outside that range
    pub fn ppu_read(&self, addr: u16) -> Option<u8> {
        todo!()
    }

    // PPU bus write:
    //   only valid when using CHR-RAM (chr_banks == 0)
    //   0x0000–0x1FFF → chr_rom[addr] = data; return true
    //   returns false if CHR-ROM (read-only) or address out of range
    pub fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Builds a minimal valid iNES byte stream.
    // PRG-ROM is filled with an incrementing pattern so reads can be verified.
    fn make_ines(prg_banks: u8, chr_banks: u8, mapper_id: u8, trainer: bool) -> Vec<u8> {
        let mut data = Vec::new();

        data.extend_from_slice(&INES_MAGIC);
        data.push(prg_banks);
        data.push(chr_banks);
        // flags 6: mapper low nibble in bits 7-4, trainer in bit 2
        let flags6: u8 = ((mapper_id & 0x0F) << 4) | if trainer { 0x04 } else { 0x00 };
        data.push(flags6);
        // flags 7: mapper high nibble in bits 7-4
        let flags7: u8 = mapper_id & 0xF0;
        data.push(flags7);
        data.extend_from_slice(&[0u8; 8]); // bytes 8–15 unused

        if trainer {
            data.extend(vec![0xFFu8; TRAINER_SIZE]);
        }

        let prg_size = prg_banks as usize * PRG_BANK_SIZE;
        for i in 0..prg_size {
            data.push((i & 0xFF) as u8);
        }

        let chr_size = chr_banks as usize * CHR_BANK_SIZE;
        for i in 0..chr_size {
            data.push((i & 0xFF) as u8);
        }

        data
    }

    // --- parsing ---

    #[test]
    fn parse_valid_16kb_prg_no_chr() {
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.prg_banks, 1);
        assert_eq!(cart.chr_banks, 0);
        assert_eq!(cart.mapper_id, 0);
    }

    #[test]
    fn parse_valid_32kb_prg_8kb_chr() {
        let cart = Cartridge::from_bytes(&make_ines(2, 1, 0, false)).unwrap();
        assert_eq!(cart.prg_banks, 2);
        assert_eq!(cart.chr_banks, 1);
    }

    #[test]
    fn parse_rejects_invalid_magic() {
        let mut rom = make_ines(1, 0, 0, false);
        rom[0] = 0x00;
        assert!(Cartridge::from_bytes(&rom).is_err());
    }

    #[test]
    fn parse_rejects_too_short() {
        assert!(Cartridge::from_bytes(&[0u8; 8]).is_err());
    }

    #[test]
    fn parse_rejects_unsupported_mapper() {
        let rom = make_ines(1, 0, 1, false); // mapper 1 = MMC1
        assert!(Cartridge::from_bytes(&rom).is_err());
    }

    #[test]
    fn parse_skips_trainer() {
        // PRG content must be identical whether or not a trainer is present
        let with_trainer = Cartridge::from_bytes(&make_ines(1, 0, 0, true)).unwrap();
        let without_trainer = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(with_trainer.prg_rom, without_trainer.prg_rom);
    }

    #[test]
    fn parse_horizontal_mirroring() {
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.mirroring, Mirroring::Horizontal);
    }

    #[test]
    fn parse_vertical_mirroring() {
        let mut rom = make_ines(1, 0, 0, false);
        rom[6] |= 0x01; // set bit 0 of flags6
        let cart = Cartridge::from_bytes(&rom).unwrap();
        assert_eq!(cart.mirroring, Mirroring::Vertical);
    }

    #[test]
    fn parse_prg_rom_size() {
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.prg_rom.len(), PRG_BANK_SIZE);

        let cart2 = Cartridge::from_bytes(&make_ines(2, 0, 0, false)).unwrap();
        assert_eq!(cart2.prg_rom.len(), PRG_BANK_SIZE * 2);
    }

    #[test]
    fn parse_chr_rom_size() {
        let cart = Cartridge::from_bytes(&make_ines(1, 1, 0, false)).unwrap();
        assert_eq!(cart.chr_rom.len(), CHR_BANK_SIZE);
    }

    #[test]
    fn parse_chr_ram_allocated_when_no_chr_banks() {
        // chr_banks == 0 → CHR-RAM: a full CHR_BANK_SIZE of zeroed memory
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.chr_rom.len(), CHR_BANK_SIZE);
        assert!(cart.chr_rom.iter().all(|&b| b == 0));
    }

    // --- cpu_read (Mapper 0) ---

    #[test]
    fn cpu_read_16kb_start_of_bank() {
        // 0x8000 & 0x3FFF = 0x0000 → prg_rom[0]
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.cpu_read(0x8000), Some(cart.prg_rom[0]));
    }

    #[test]
    fn cpu_read_16kb_mirrored_upper_bank() {
        // 16 KB: 0xC000–0xFFFF mirrors 0x8000–0xBFFF
        // 0xC000 & 0x3FFF = 0x0000 — same as 0x8000
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.cpu_read(0xC000), cart.cpu_read(0x8000));
        assert_eq!(cart.cpu_read(0xFFFF), cart.cpu_read(0xBFFF));
    }

    #[test]
    fn cpu_read_32kb_no_mirror() {
        // 32 KB: upper and lower halves are distinct
        // 0x8000 & 0x7FFF = 0x0000 → prg_rom[0x0000]
        // 0xC000 & 0x7FFF = 0x4000 → prg_rom[0x4000]
        let cart = Cartridge::from_bytes(&make_ines(2, 0, 0, false)).unwrap();
        assert_ne!(cart.cpu_read(0x8000), cart.cpu_read(0xC000));
        assert_eq!(cart.cpu_read(0xC000), Some(cart.prg_rom[0x4000]));
    }

    #[test]
    fn cpu_read_returns_none_below_8000() {
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.cpu_read(0x0000), None);
        assert_eq!(cart.cpu_read(0x6000), None);
        assert_eq!(cart.cpu_read(0x7FFF), None);
    }

    // --- cpu_write ---

    #[test]
    fn cpu_write_always_false_mapper0() {
        let mut cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert!(!cart.cpu_write(0x8000, 0x42));
        assert!(!cart.cpu_write(0x0000, 0x42));
    }

    // --- ppu_read ---

    #[test]
    fn ppu_read_chr_rom() {
        let cart = Cartridge::from_bytes(&make_ines(1, 1, 0, false)).unwrap();
        assert_eq!(cart.ppu_read(0x0000), Some(cart.chr_rom[0]));
        assert_eq!(cart.ppu_read(0x1FFF), Some(cart.chr_rom[0x1FFF]));
    }

    #[test]
    fn ppu_read_chr_ram_zeroed_on_init() {
        let cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert_eq!(cart.ppu_read(0x0000), Some(0x00));
        assert_eq!(cart.ppu_read(0x1FFF), Some(0x00));
    }

    #[test]
    fn ppu_read_returns_none_outside_chr_range() {
        let cart = Cartridge::from_bytes(&make_ines(1, 1, 0, false)).unwrap();
        assert_eq!(cart.ppu_read(0x2000), None);
    }

    // --- ppu_write ---

    #[test]
    fn ppu_write_chr_ram_roundtrip() {
        let mut cart = Cartridge::from_bytes(&make_ines(1, 0, 0, false)).unwrap();
        assert!(cart.ppu_write(0x0010, 0xAB));
        assert_eq!(cart.ppu_read(0x0010), Some(0xAB));
    }

    #[test]
    fn ppu_write_chr_rom_returns_false() {
        let mut cart = Cartridge::from_bytes(&make_ines(1, 1, 0, false)).unwrap();
        assert!(!cart.ppu_write(0x0010, 0xAB));
    }
}
