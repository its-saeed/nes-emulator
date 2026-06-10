use eframe::egui;
use nes_emu::{bus::Bus, cpu::Cpu};
use rand::Rng;

const SCREEN_W: usize = 32;
const SCREEN_H: usize = 32;
const DEFAULT_SCALE: f32 = 10.0;
const DEFAULT_CYCLES_PER_FRAME: u32 = 100;
const PROGRAM_START: u16 = 0x0600;
const SIDE_PANEL_WIDTH: f32 = 360.0;

const FLAGS: [(&str, u8); 8] = [
    ("N", 0x80),
    ("V", 0x40),
    ("U", 0x20),
    ("B", 0x10),
    ("D", 0x08),
    ("I", 0x04),
    ("Z", 0x02),
    ("C", 0x01),
];

fn pixel_color(byte: u8) -> [u8; 3] {
    match byte {
        0 => [0, 0, 0],
        1 => [255, 255, 255],
        2 | 9 => [128, 128, 128],
        3 | 10 => [255, 0, 0],
        4 | 11 => [0, 255, 0],
        5 | 12 => [0, 0, 255],
        6 | 13 => [255, 0, 255],
        7 | 14 => [255, 255, 0],
        _ => [128, 0, 128],
    }
}

fn default_game_code() -> Vec<u8> {
    vec![
        0x20, 0x06, 0x06, 0x20, 0x38, 0x06, 0x20, 0x0d, 0x06, 0x20, 0x2a, 0x06, 0x60, 0xa9, 0x02,
        0x85, 0x02, 0xa9, 0x04, 0x85, 0x03, 0xa9, 0x11, 0x85, 0x10, 0xa9, 0x10, 0x85, 0x12, 0xa9,
        0x0f, 0x85, 0x14, 0xa9, 0x04, 0x85, 0x11, 0x85, 0x13, 0x85, 0x15, 0x60, 0xa5, 0xfe, 0x85,
        0x00, 0xa5, 0xfe, 0x29, 0x03, 0x18, 0x69, 0x02, 0x85, 0x01, 0x60, 0x20, 0x4d, 0x06, 0x20,
        0x8d, 0x06, 0x20, 0xc3, 0x06, 0x20, 0x19, 0x07, 0x20, 0x20, 0x07, 0x20, 0x2d, 0x07, 0x4c,
        0x38, 0x06, 0xa5, 0xff, 0xc9, 0x77, 0xf0, 0x0d, 0xc9, 0x64, 0xf0, 0x14, 0xc9, 0x73, 0xf0,
        0x1b, 0xc9, 0x61, 0xf0, 0x22, 0x60, 0xa9, 0x04, 0x24, 0x02, 0xd0, 0x26, 0xa9, 0x01, 0x85,
        0x02, 0x60, 0xa9, 0x08, 0x24, 0x02, 0xd0, 0x1b, 0xa9, 0x02, 0x85, 0x02, 0x60, 0xa9, 0x01,
        0x24, 0x02, 0xd0, 0x10, 0xa9, 0x04, 0x85, 0x02, 0x60, 0xa9, 0x02, 0x24, 0x02, 0xd0, 0x05,
        0xa9, 0x08, 0x85, 0x02, 0x60, 0x60, 0x20, 0x94, 0x06, 0x20, 0xa8, 0x06, 0x60, 0xa5, 0x00,
        0xc5, 0x10, 0xd0, 0x0d, 0xa5, 0x01, 0xc5, 0x11, 0xd0, 0x07, 0xe6, 0x03, 0xe6, 0x03, 0x20,
        0x2a, 0x06, 0x60, 0xa2, 0x02, 0xb5, 0x10, 0xc5, 0x10, 0xd0, 0x06, 0xb5, 0x11, 0xc5, 0x11,
        0xf0, 0x09, 0xe8, 0xe8, 0xe4, 0x03, 0xf0, 0x06, 0x4c, 0xaa, 0x06, 0x4c, 0x35, 0x07, 0x60,
        0xa6, 0x03, 0xca, 0x8a, 0xb5, 0x10, 0x95, 0x12, 0xca, 0x10, 0xf9, 0xa5, 0x02, 0x4a, 0xb0,
        0x09, 0x4a, 0xb0, 0x19, 0x4a, 0xb0, 0x1f, 0x4a, 0xb0, 0x2f, 0xa5, 0x10, 0x38, 0xe9, 0x20,
        0x85, 0x10, 0x90, 0x01, 0x60, 0xc6, 0x11, 0xa9, 0x01, 0xc5, 0x11, 0xf0, 0x28, 0x60, 0xe6,
        0x10, 0xa9, 0x1f, 0x24, 0x10, 0xf0, 0x1f, 0x60, 0xa5, 0x10, 0x18, 0x69, 0x20, 0x85, 0x10,
        0xb0, 0x01, 0x60, 0xe6, 0x11, 0xa9, 0x06, 0xc5, 0x11, 0xf0, 0x0c, 0x60, 0xc6, 0x10, 0xa5,
        0x10, 0x29, 0x1f, 0xc9, 0x1f, 0xf0, 0x01, 0x60, 0x4c, 0x35, 0x07, 0xa0, 0x00, 0xa5, 0xfe,
        0x91, 0x00, 0x60, 0xa6, 0x03, 0xa9, 0x00, 0x81, 0x10, 0xa2, 0x00, 0xa9, 0x01, 0x81, 0x10,
        0x60, 0xa2, 0x00, 0xea, 0xea, 0xca, 0xd0, 0xfb, 0x60,
    ]
}

fn format_code(code: &[u8]) -> String {
    code.chunks(16)
        .map(|chunk| {
            chunk
                .iter()
                .map(|byte| format!("{:02X}", byte))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_code(text: &str) -> Result<Vec<u8>, String> {
    let mut code = Vec::new();

    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let line = line.split("//").next().unwrap_or("");

        for token in line.split(|c: char| c.is_ascii_whitespace() || c == ',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }

            let token = token
                .strip_prefix("0x")
                .or_else(|| token.strip_prefix("0X"))
                .unwrap_or(token);
            let byte = u8::from_str_radix(token, 16)
                .map_err(|_| format!("Invalid byte `{token}`. Use hex values like A9 or 0xA9."))?;
            code.push(byte);
        }
    }

    if code.is_empty() {
        return Err("Program is empty.".to_owned());
    }

    let max_len = (u16::MAX - PROGRAM_START + 1) as usize;
    if code.len() > max_len {
        return Err(format!(
            "Program is too large: {} bytes, max is {}.",
            code.len(),
            max_len
        ));
    }

    Ok(code)
}

#[derive(Clone, Copy, PartialEq)]
enum DebugTab {
    Cpu,
    Memory,
    Program,
}

struct App {
    cpu: Cpu,
    bus: Bus,
    rng: rand::rngs::ThreadRng,
    paused: bool,
    cycles_per_frame: u32,
    total_ticks: u64,
    zoom: f32,
    loaded_program: Vec<u8>,
    code_text: String,
    code_status: String,
    code_status_is_error: bool,
    active_tab: DebugTab,
}

impl App {
    fn new() -> Self {
        let game_code = default_game_code();
        let mut bus = Bus::load(&game_code, PROGRAM_START);
        bus.write_u16(0xFFFC, PROGRAM_START);

        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        let code_text = format_code(&game_code);

        Self {
            cpu,
            bus,
            rng: rand::rng(),
            paused: false,
            cycles_per_frame: DEFAULT_CYCLES_PER_FRAME,
            total_ticks: 0,
            zoom: DEFAULT_SCALE,
            loaded_program: game_code,
            code_text,
            code_status: "Loaded sample program.".to_owned(),
            code_status_is_error: false,
            active_tab: DebugTab::Cpu,
        }
    }

    fn reset(&mut self) {
        self.bus = Bus::load(&self.loaded_program, PROGRAM_START);
        self.bus.write_u16(0xFFFC, PROGRAM_START);
        self.cpu = Cpu::new();
        self.cpu.reset(&mut self.bus);
        self.total_ticks = 0;
        self.paused = true;
    }

    fn restore_sample_program(&mut self) {
        let game_code = default_game_code();
        self.code_text = format_code(&game_code);
        self.load_program(game_code);
        self.code_status = "Loaded sample program.".to_owned();
        self.code_status_is_error = false;
    }

    fn load_program(&mut self, code: Vec<u8>) {
        self.loaded_program = code;
        self.reset();
    }

    fn load_code_text(&mut self) {
        match parse_code(&self.code_text) {
            Ok(code) => {
                let len = code.len();
                self.load_program(code);
                self.code_status = format!("Loaded {len} bytes at ${PROGRAM_START:04X}.");
                self.code_status_is_error = false;
            }
            Err(message) => {
                self.code_status = message;
                self.code_status_is_error = true;
            }
        }
    }

    fn clock(&mut self) {
        self.bus.write(0xFE, self.rng.random_range(1u8..16));
        self.cpu.clock(&mut self.bus);
        self.total_ticks += 1;
    }

    fn run_frame(&mut self) {
        for _ in 0..self.cycles_per_frame {
            self.clock();
        }
    }

    fn pixel_buffer(&self) -> Vec<u8> {
        let mut pixels = vec![0u8; SCREEN_W * SCREEN_H * 3];
        for i in 0..(SCREEN_W * SCREEN_H) {
            let [r, g, b] = pixel_color(self.bus.read(0x0200 + i as u16));
            pixels[i * 3] = r;
            pixels[i * 3 + 1] = g;
            pixels[i * 3 + 2] = b;
        }
        pixels
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("NES Emu");
            ui.add_space(12.0);

            let run_label = if self.paused { "Resume" } else { "Pause" };
            let run_color = if self.paused {
                egui::Color32::from_rgb(22, 163, 74)
            } else {
                egui::Color32::from_rgb(202, 138, 4)
            };
            if ui
                .add_sized(
                    [92.0, 30.0],
                    egui::Button::new(egui::RichText::new(run_label).strong()).fill(run_color),
                )
                .clicked()
            {
                self.paused = !self.paused;
            }

            let step = ui.add_enabled(
                self.paused,
                egui::Button::new("Step").min_size(egui::vec2(64.0, 30.0)),
            );
            if step.clicked() {
                self.clock();
            }

            if ui
                .add_sized([64.0, 30.0], egui::Button::new("Reset"))
                .clicked()
            {
                self.reset();
            }

            ui.separator();
            ui.label("Speed");
            ui.add(
                egui::Slider::new(&mut self.cycles_per_frame, 1..=1_000)
                    .show_value(false)
                    .step_by(1.0),
            );
            ui.monospace(format!("{}", self.cycles_per_frame));

            ui.separator();
            ui.label("Zoom");
            ui.add(
                egui::Slider::new(&mut self.zoom, 2.0..=24.0)
                    .show_value(false)
                    .step_by(1.0),
            );
            ui.monospace(format!("{:.0}x", self.zoom));
        });
    }

    fn debugger_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Debugger");
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label("State");
            let status = if self.paused { "Paused" } else { "Running" };
            let color = if self.paused {
                egui::Color32::from_rgb(245, 158, 11)
            } else {
                egui::Color32::from_rgb(34, 197, 94)
            };
            ui.colored_label(color, status);
        });

        ui.add_space(10.0);
        self.summary_grid(ui);

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, DebugTab::Cpu, "CPU");
            ui.selectable_value(&mut self.active_tab, DebugTab::Memory, "Memory");
            ui.selectable_value(&mut self.active_tab, DebugTab::Program, "Program");
        });
        ui.separator();
        ui.add_space(6.0);

        match self.active_tab {
            DebugTab::Cpu => self.cpu_tab(ui),
            DebugTab::Memory => self.memory_tab(ui),
            DebugTab::Program => self.program_tab(ui),
        }
    }

    fn summary_grid(&self, ui: &mut egui::Ui) {
        egui::Grid::new("debugger_stats")
            .num_columns(2)
            .spacing([18.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label("Ticks");
                ui.monospace(format!("{}", self.total_ticks));
                ui.end_row();
                ui.label("Speed");
                ui.monospace(format!("{}/frame", self.cycles_per_frame));
                ui.end_row();
                ui.label("Next");
                ui.monospace(format!(
                    "{:04X}  {}",
                    self.cpu.pc,
                    self.cpu.disassemble(self.cpu.pc, &self.bus)
                ));
                ui.end_row();
                ui.label("Program");
                ui.monospace(format!("{} bytes", self.loaded_program.len()));
                ui.end_row();
            });
    }

    fn cpu_tab(&self, ui: &mut egui::Ui) {
        ui.strong("Registers");
        ui.add_space(6.0);
        egui::Grid::new("registers")
            .num_columns(4)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                self.register_value(ui, "PC", format!("{:04X}", self.cpu.pc));
                self.register_value(ui, "SP", format!("{:02X}", self.cpu.stack.ptr));
                ui.end_row();
                self.register_value(ui, "A", format!("{:02X}", self.cpu.a));
                self.register_value(ui, "P", format!("{:08b}", self.cpu.status));
                ui.end_row();
                self.register_value(ui, "X", format!("{:02X}", self.cpu.x));
                self.register_value(ui, "Y", format!("{:02X}", self.cpu.y));
                ui.end_row();
            });

        ui.add_space(16.0);
        ui.strong("Flags");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for (name, mask) in FLAGS {
                let enabled = self.cpu.status & mask != 0;
                let fill = if enabled {
                    egui::Color32::from_rgb(37, 99, 235)
                } else {
                    egui::Color32::from_gray(45)
                };
                ui.label(
                    egui::RichText::new(format!(" {name} "))
                        .monospace()
                        .strong()
                        .background_color(fill)
                        .color(egui::Color32::WHITE),
                );
            }
        });
    }

    fn register_value(&self, ui: &mut egui::Ui, name: &str, value: String) {
        ui.label(egui::RichText::new(name).small());
        ui.monospace(egui::RichText::new(value).size(18.0).strong());
    }

    fn memory_tab(&self, ui: &mut egui::Ui) {
        ui.strong("Zero Page");
        ui.add_space(6.0);
        egui::Grid::new("zero_page")
            .num_columns(8)
            .spacing([10.0, 5.0])
            .show(ui, |ui| {
                for row in 0..4u16 {
                    for col in 0..4u16 {
                        let addr = row * 4 + col;
                        ui.monospace(format!("${addr:02X}"));
                        ui.monospace(format!("{:02X}", self.bus.read(addr)));
                    }
                    ui.end_row();
                }
            });
    }

    fn program_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Program");
            ui.monospace(format!("${PROGRAM_START:04X}"));
        });
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::multiline(&mut self.code_text)
                .font(egui::TextStyle::Monospace)
                .desired_rows(14)
                .desired_width(f32::INFINITY)
                .code_editor(),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_sized([104.0, 30.0], egui::Button::new("Load Code"))
                .clicked()
            {
                self.load_code_text();
            }
            if ui
                .add_sized([124.0, 30.0], egui::Button::new("Restore Sample"))
                .clicked()
            {
                self.restore_sample_program();
            }
        });

        ui.add_space(6.0);
        let color = if self.code_status_is_error {
            egui::Color32::from_rgb(248, 113, 113)
        } else {
            egui::Color32::from_rgb(34, 197, 94)
        };
        ui.colored_label(color, &self.code_status);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // input
        let wants_kb = ctx.wants_keyboard_input();
        ctx.input(|i| {
            if !wants_kb && i.key_pressed(egui::Key::Space) {
                self.paused = !self.paused;
            }

            if !wants_kb && self.paused && i.key_pressed(egui::Key::ArrowRight) {
                self.clock();
            }

            if i.key_down(egui::Key::W) {
                self.bus.write(0xFF, 0x77);
            }
            if i.key_down(egui::Key::S) {
                self.bus.write(0xFF, 0x73);
            }
            if i.key_down(egui::Key::A) {
                self.bus.write(0xFF, 0x61);
            }
            if i.key_down(egui::Key::D) {
                self.bus.write(0xFF, 0x64);
            }
        });

        if !self.paused {
            self.run_frame();
        }

        // game screen
        let pixels = self.pixel_buffer();
        let image = egui::ColorImage::from_rgb([SCREEN_W, SCREEN_H], &pixels);
        let texture = ctx.load_texture("screen", image, egui::TextureOptions::NEAREST);
        let screen_size = egui::vec2(SCREEN_W as f32 * self.zoom, SCREEN_H as f32 * self.zoom);

        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0)),
            )
            .show(ctx, |ui| {
                self.top_bar(ui);
            });

        egui::SidePanel::right("debugger")
            .min_width(SIDE_PANEL_WIDTH)
            .resizable(false)
            .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::same(12.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.debugger_panel(ui);
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                let (rect, _) = ui.allocate_exact_size(screen_size, egui::Sense::hover());
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            });
        });

        if !self.paused {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NES Emu")
            .with_visible(true)
            .with_active(true)
            .with_inner_size([
                SCREEN_W as f32 * DEFAULT_SCALE + SIDE_PANEL_WIDTH,
                SCREEN_H as f32 * DEFAULT_SCALE + 56.0,
            ]),
        ..Default::default()
    };
    eframe::run_native("NES Emu", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}
