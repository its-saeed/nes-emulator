use nes_emu::{
    bus::Bus,
    cartridge::Cartridge,
    cpu::{AddrMode, Cpu, TraceState},
};

fn to_string(trace: &TraceState) -> String {
    let bytes_str = match trace.instr_len {
        1 => format!("{:02X}      ", trace.raw_bytes[0]),
        2 => format!("{:02X} {:02X}   ", trace.raw_bytes[0], trace.raw_bytes[1]),
        _ => format!(
            "{:02X} {:02X} {:02X}",
            trace.raw_bytes[0], trace.raw_bytes[1], trace.raw_bytes[2]
        ),
    };

    let name = trace.instr_name;
    let b1 = trace.raw_bytes[1];
    let b2 = trace.raw_bytes[2];
    let addr16 = (b1 as u16) | ((b2 as u16) << 8);
    let disasm = match trace.addr_mode_type {
        AddrMode::Imp => name.to_string(),
        AddrMode::Imm => format!("{name} #${b1:02X}"),
        AddrMode::Zp0 => format!("{name} ${b1:02X}"),
        AddrMode::Zpx => format!("{name} ${b1:02X},X"),
        AddrMode::Zpy => format!("{name} ${b1:02X},Y"),
        AddrMode::Abs => format!("{name} ${addr16:04X}"),
        AddrMode::Abx => format!("{name} ${addr16:04X},X"),
        AddrMode::Aby => format!("{name} ${addr16:04X},Y"),
        AddrMode::Ind => format!("{name} (${addr16:04X})"),
        AddrMode::Izx => format!("{name} (${b1:02X},X)"),
        AddrMode::Izy => format!("{name} (${b1:02X}),Y"),
        AddrMode::Rel => {
            let offset = b1 as i8;
            let target = trace.pc.wrapping_add(2).wrapping_add(offset as u16);
            format!("{name} ${target:04X}")
        }
    };

    let ppu_cycle = trace.total_cycles * 3;
    let ppu_dot = ppu_cycle % 341;
    let ppu_scanline = ppu_cycle / 341;

    format!(
        "{:04X}  {}  {:<32}A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} PPU:{:3},{:3} CYC:{}",
        trace.pc,
        bytes_str,
        disasm,
        trace.a,
        trace.x,
        trace.y,
        trace.status,
        trace.sp,
        ppu_scanline,
        ppu_dot,
        trace.total_cycles
    )
}

#[test]
fn nestest() {
    let rom = std::fs::read("tests/roms/nestest.nes").expect("nestest.nes not found");
    let cart = Cartridge::from_bytes(&rom).expect("failed to parse ROM");
    let mut bus = Bus::new();
    bus.insert_cartridge(cart);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);
    cpu.pc = 0xC000;

    let log = std::fs::read_to_string("tests/roms/nestest.log").expect("nestest.log not found");

    // Drain the 7 reset cycles to reach the first instruction boundary
    while cpu.trace(&bus).is_none() {
        cpu.clock(&mut bus);
    }

    for (line_num, expected) in log.lines().enumerate() {
        let trace = cpu.trace(&bus).expect("expected instruction boundary");
        let got = to_string(&trace);

        // The disasm column (chars 16..48) in the reference log includes effective-address
        // annotations (e.g. "STX $00 = 00") that we don't generate, so we skip that column
        // and only compare PC+bytes (0..16) and registers+PPU+cycles (48..).
        assert_eq!(
            &expected[..16],
            &got[..16],
            "PC/bytes mismatch at line {}",
            line_num + 1
        );
        assert_eq!(
            &expected[48..],
            &got[48..],
            "registers/PPU/cycles mismatch at line {}\n  expected: {}\n  got:      {}",
            line_num + 1,
            expected,
            got
        );

        // Execute the current instruction and advance to the next boundary
        loop {
            cpu.clock(&mut bus);
            if cpu.trace(&bus).is_some() {
                break;
            }
        }
    }
}
