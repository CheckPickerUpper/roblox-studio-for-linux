use crate::config::{load_config, LauncherConfig, StudioLoginMode};
use crate::error::LauncherError;
use crate::mcp::generate_client_configuration;
use eframe::egui;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

const WINDOW_SIZE: [f32; 2] = [900.0, 760.0];
const MIN_WINDOW_SIZE: [f32; 2] = [720.0, 600.0];
const PAGE_PADDING: f32 = 24.0;
const CARD_PADDING: f32 = 18.0;
const SECTION_GAP: f32 = 16.0;
const CONTROL_HEIGHT: f32 = 38.0;
const OUTPUT_HEIGHT: f32 = 190.0;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const APP_ID: &str = "io.github.checkpickerupper.RobloxStudioLinuxLauncher";
const APP_TITLE: &str = "Roblox Studio Linux Launcher";

const APP_ICON_BYTES: &[u8] =
    include_bytes!("../assets/io.github.checkpickerupper.RobloxStudioLinuxLauncher.png");

#[derive(Clone, Copy)]
struct Palette {
    page: egui::Color32,
    card: egui::Color32,
    card_alt: egui::Color32,
    input: egui::Color32,
    border: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
    accent: egui::Color32,
    accent_hover: egui::Color32,
    accent_text: egui::Color32,
    success_bg: egui::Color32,
    success_text: egui::Color32,
    warning_bg: egui::Color32,
    warning_text: egui::Color32,
    error_bg: egui::Color32,
    error_text: egui::Color32,
    info_bg: egui::Color32,
    info_text: egui::Color32,
}

fn palette() -> Palette {
    Palette {
        page: egui::Color32::from_rgb(245, 247, 250),
        card: egui::Color32::WHITE,
        card_alt: egui::Color32::from_rgb(246, 247, 249),
        input: egui::Color32::from_rgb(249, 250, 251),
        border: egui::Color32::from_rgb(222, 226, 233),
        text: egui::Color32::from_rgb(25, 32, 45),
        muted: egui::Color32::from_rgb(101, 112, 132),
        accent: egui::Color32::from_rgb(91, 83, 214),
        accent_hover: egui::Color32::from_rgb(112, 104, 231),
        accent_text: egui::Color32::WHITE,
        success_bg: egui::Color32::from_rgb(231, 247, 237),
        success_text: egui::Color32::from_rgb(28, 119, 69),
        warning_bg: egui::Color32::from_rgb(255, 246, 219),
        warning_text: egui::Color32::from_rgb(150, 100, 0),
        error_bg: egui::Color32::from_rgb(253, 235, 237),
        error_text: egui::Color32::from_rgb(180, 35, 48),
        info_bg: egui::Color32::from_rgb(237, 239, 255),
        info_text: egui::Color32::from_rgb(72, 77, 158),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusTone {
    Neutral,
    Info,
    Success,
    Warning,
    Error,
}

fn status_tone(status: &str) -> StatusTone {
    let status = status.to_ascii_lowercase();
    if status.contains("fail")
        || status.contains("could not")
        || status.contains("error")
        || status.contains("stopped")
    {
        StatusTone::Error
    } else if status.contains("running") {
        StatusTone::Info
    } else if status.contains("needs") || status.contains("not checked") {
        StatusTone::Warning
    } else if status.contains("connected")
        || status.contains("completed")
        || status.contains("requested")
        || status.contains("copied")
        || status.contains("opened")
        || status == "ready"
    {
        StatusTone::Success
    } else {
        StatusTone::Neutral
    }
}

fn status_colors(palette: Palette, tone: StatusTone) -> (egui::Color32, egui::Color32) {
    match tone {
        StatusTone::Neutral => (palette.card_alt, palette.text),
        StatusTone::Info => (palette.info_bg, palette.info_text),
        StatusTone::Success => (palette.success_bg, palette.success_text),
        StatusTone::Warning => (palette.warning_bg, palette.warning_text),
        StatusTone::Error => (palette.error_bg, palette.error_text),
    }
}

fn apply_visual_style(context: &egui::Context, palette: Palette) {
    context.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style.spacing.interact_size = egui::vec2(44.0, CONTROL_HEIGHT);
        style.spacing.window_margin = egui::Margin::same(0.0);

        let visuals = &mut style.visuals;
        visuals.dark_mode = false;
        visuals.override_text_color = Some(palette.text);
        visuals.panel_fill = palette.page;
        visuals.window_fill = palette.card;
        visuals.extreme_bg_color = palette.input;
        visuals.faint_bg_color = palette.card_alt;
        visuals.code_bg_color = palette.input;
        visuals.warn_fg_color = palette.warning_text;
        visuals.error_fg_color = palette.error_text;
        visuals.hyperlink_color = palette.accent;
        visuals.button_frame = true;
        visuals.collapsing_header_frame = false;

        visuals.widgets.noninteractive.bg_fill = palette.card;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, palette.border);
        visuals.widgets.noninteractive.fg_stroke.color = palette.text;
        visuals.widgets.inactive.bg_fill = palette.card_alt;
        visuals.widgets.inactive.weak_bg_fill = palette.card_alt;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, palette.border);
        visuals.widgets.inactive.fg_stroke.color = palette.text;
        visuals.widgets.hovered.bg_fill = palette.card_alt;
        visuals.widgets.hovered.weak_bg_fill = palette.card_alt;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, palette.accent);
        visuals.widgets.hovered.fg_stroke.color = palette.text;
        visuals.widgets.active.bg_fill = palette.accent;
        visuals.widgets.active.weak_bg_fill = palette.accent;
        visuals.widgets.active.fg_stroke.color = palette.accent_text;
        visuals.selection.bg_fill = palette.accent.gamma_multiply(0.35);
        visuals.selection.stroke.color = palette.accent_hover;
    });
}

pub(crate) fn run_gui(config_path: PathBuf) -> Result<(), LauncherError> {
    let config = load_config(&config_path)?;
    let app_icon = eframe::icon_data::from_png_bytes(APP_ICON_BYTES).map_err(|error| {
        LauncherError::GuiStartup {
            message: format!("could not load the launcher icon: {error}"),
        }
    })?;
    let logo_image = egui::ColorImage::from_rgba_unmultiplied(
        [app_icon.width as usize, app_icon.height as usize],
        &app_icon.rgba,
    );
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(MIN_WINDOW_SIZE)
            .with_position([120.0, 120.0])
            .with_icon(app_icon),
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(move |creation_context| {
            apply_visual_style(&creation_context.egui_ctx, palette());
            Ok(Box::new(LauncherApp::new(config, logo_image)))
        }),
    )
    .map_err(|error| LauncherError::GuiStartup {
        message: error.to_string(),
    })
}

struct LauncherApp {
    config_path: PathBuf,
    wine_binary: String,
    wine_prefix: String,
    studio_executable: String,
    login_mode: StudioLoginMode,
    mcp_client_config: String,
    mcp_status: String,
    show_mcp_diagnostics: bool,
    show_command_output: bool,
    logo_image: egui::ColorImage,
    logo: Option<egui::TextureHandle>,
    operation: Option<Operation>,
    status: String,
    output: String,
}

struct Operation {
    name: String,
    result: Receiver<CommandResult>,
}

struct CommandResult {
    status: String,
    output: String,
}

fn launcher_arguments(config_path: &Path, command: &[String]) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("--config"),
        config_path.as_os_str().to_os_string(),
    ];
    arguments.extend(command.iter().map(OsString::from));
    arguments
}

impl LauncherApp {
    fn new(config: LauncherConfig, logo_image: egui::ColorImage) -> Self {
        Self {
            config_path: config.config_path,
            wine_binary: config.wine_binary,
            wine_prefix: config.wine_prefix.display().to_string(),
            studio_executable: config
                .studio_executable
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            login_mode: config.login_mode,
            mcp_client_config: String::new(),
            mcp_status: "Not checked".to_owned(),
            show_mcp_diagnostics: false,
            show_command_output: false,
            logo_image,
            logo: None,
            operation: None,
            status: "Ready".to_owned(),
            output: "Use Check setup to inspect the current Wine and Studio installation."
                .to_owned(),
        }
    }

    fn start_operation(&mut self, name: &str, command: Vec<String>) {
        if self.operation.is_some() {
            return;
        }

        let executable = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                self.status = format!("Could not find the launcher executable: {error}");
                self.show_command_output = false;
                return;
            }
        };
        let config_path = self.config_path.clone();
        let arguments = launcher_arguments(&config_path, &command);
        let (sender, receiver) = mpsc::channel();
        let operation_name = name.to_owned();
        let thread_name = operation_name.clone();
        thread::spawn(move || {
            let mut child_command = Command::new(executable);
            child_command
                .args(arguments)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let result = match child_command.spawn() {
                Ok(child) => match child.wait_with_output() {
                    Ok(output) => {
                        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                        text.push_str(&String::from_utf8_lossy(&output.stderr));
                        let status = if output.status.success() {
                            match thread_name.as_str() {
                                "Launch Studio" => {
                                    "Studio opened; sign in inside Studio if asked".to_owned()
                                }
                                "Browser sign-in" => {
                                    "Studio opened; browser sign-in requested".to_owned()
                                }
                                _ => format!("{thread_name} completed successfully"),
                            }
                        } else {
                            format!(
                                "{thread_name} failed with {}",
                                output.status.code().map_or_else(
                                    || "no exit code".to_owned(),
                                    |code| code.to_string()
                                )
                            )
                        };
                        CommandResult {
                            status,
                            output: text,
                        }
                    }
                    Err(error) => CommandResult {
                        status: format!("{thread_name} could not be collected: {error}"),
                        output: String::new(),
                    },
                },
                Err(error) => CommandResult {
                    status: format!("{thread_name} could not start: {error}"),
                    output: String::new(),
                },
            };
            if let Err(error) = sender.send(result) {
                tracing::debug!(error = %error, "The launcher GUI stopped listening for command output");
            }
        });
        self.status = format!("{operation_name} is running…");
        self.show_command_output = false;
        self.output.clear();
        self.operation = Some(Operation {
            name: operation_name,
            result: receiver,
        });
    }

    fn save_settings(&mut self) {
        let mut command = vec!["configure".to_owned()];
        command.extend(["--wine-binary".to_owned(), self.wine_binary.clone()]);
        command.extend(["--wine-prefix".to_owned(), self.wine_prefix.clone()]);
        if !self.studio_executable.trim().is_empty() {
            command.extend([
                "--studio-executable".to_owned(),
                self.studio_executable.clone(),
            ]);
        } else {
            command.push("--clear-studio-executable".to_owned());
        }
        command.push(self.login_mode.configure_flag().to_owned());
        self.start_operation("Save settings", command);
    }

    fn poll_operation(&mut self, context: &egui::Context) {
        let Some(operation) = &self.operation else {
            return;
        };
        let operation_name = operation.name.clone();
        match operation.result.try_recv() {
            Ok(result) => {
                let launch_succeeded =
                    matches!(operation_name.as_str(), "Launch Studio" | "Browser sign-in")
                        && result.status.starts_with("Studio opened;");
                if operation_name == "Test MCP connection" {
                    self.mcp_status = classify_mcp_status(&result.output);
                }
                self.show_command_output = result.status.contains("failed")
                    || result.status.contains("could not")
                    || result.status.contains("stopped");
                self.status = result.status;
                self.output = if result.output.is_empty() {
                    "The command did not produce any output.".to_owned()
                } else {
                    result.output
                };
                self.operation = None;
                if launch_succeeded {
                    context.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
            Err(mpsc::TryRecvError::Empty) => context.request_repaint_after(POLL_INTERVAL),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = format!("{} stopped unexpectedly", operation_name);
                self.show_command_output = true;
                self.operation = None;
            }
        }
    }

    fn show_action_buttons(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let busy = self.operation.is_some();
        ui.horizontal_wrapped(|ui| {
            if action_button(ui, "Launch Studio", !busy, true, palette).clicked() {
                self.start_operation("Launch Studio", vec!["launch".to_owned()]);
            }
            if action_button(ui, "Install / update", !busy, false, palette).clicked() {
                self.start_operation("Install / update Studio", vec!["install".to_owned()]);
            }
            if action_button(ui, "Check setup", !busy, false, palette).clicked() {
                self.start_operation("Check setup", vec!["doctor".to_owned()]);
            }
            if action_button(ui, "Browser sign-in", !busy, false, palette).clicked() {
                self.start_operation("Browser sign-in", vec!["browser-login".to_owned()]);
            }
        });
        if let Some(operation) = &self.operation {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new(format!("{} is working…", operation.name))
                        .color(palette.info_text),
                );
            });
        }
    }

    fn show_mcp_controls(&mut self, ui: &mut egui::Ui, context: &egui::Context, palette: Palette) {
        section_heading(
            ui,
            "Connect AI tools",
            "Let an AI client work with the place currently open in Studio.",
            palette,
        );
        ui.add_space(6.0);
        status_badge(ui, &self.mcp_status, palette, status_tone(&self.mcp_status));

        ui.add_space(8.0);
        egui::Frame::none()
            .fill(palette.info_bg.gamma_multiply(0.55))
            .stroke(egui::Stroke::new(1.0_f32, palette.info_bg))
            .rounding(8.0)
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "In Studio: Assistant > ... > Manage MCP Servers > Enable Studio as MCP server.",
                        )
                        .color(palette.info_text),
                    )
                    .wrap(),
                );
            });

        ui.add_space(10.0);
        labeled_text_field(
            ui,
            "AI client config",
            &mut self.mcp_client_config,
            "Optional path to the client's mcp.json",
            palette,
        );
        ui.small(
            egui::RichText::new(
                "Use Copy configuration if you only want the ready-to-paste setup.",
            )
            .color(palette.muted),
        );

        ui.add_space(6.0);
        let busy = self.operation.is_some();
        ui.horizontal_wrapped(|ui| {
            if action_button(ui, "Set up for this client", !busy, true, palette).clicked() {
                if self.mcp_client_config.trim().is_empty() {
                    self.status =
                        "Enter the client JSON path, or use Copy configuration instead.".to_owned();
                    self.show_command_output = false;
                } else {
                    self.start_operation(
                        "Set up MCP",
                        vec![
                            "mcp".to_owned(),
                            "setup".to_owned(),
                            "--client-config".to_owned(),
                            self.mcp_client_config.clone(),
                        ],
                    );
                }
            }
            if action_button(ui, "Copy configuration", !busy, false, palette).clicked() {
                match generate_client_configuration(&self.config_path) {
                    Ok(configuration) => {
                        context.copy_text(configuration.clone());
                        self.output = configuration;
                        self.status = "Client configuration copied to the clipboard".to_owned();
                        self.show_command_output = true;
                    }
                    Err(error) => {
                        self.status = format!("Could not create client configuration: {error}");
                        self.show_command_output = false;
                    }
                }
            }
            if action_button(ui, "Check AI connection", !busy, false, palette).clicked() {
                self.start_operation(
                    "Test MCP connection",
                    vec!["mcp".to_owned(), "doctor".to_owned()],
                );
            }
            if action_button(ui, "Connection details", true, false, palette).clicked() {
                self.show_mcp_diagnostics = !self.show_mcp_diagnostics;
                self.show_command_output = self.show_mcp_diagnostics;
            }
        });

        if self.show_mcp_diagnostics {
            ui.add_space(6.0);
            egui::Frame::none()
                .fill(palette.input)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .rounding(8.0)
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Connection details")
                            .strong()
                            .color(palette.text),
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut self.output)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(ui.available_width())
                            .desired_rows(6),
                    );
                });
        }
    }

    fn show_settings(&mut self, ui: &mut egui::Ui, palette: Palette) {
        egui::CollapsingHeader::new(
            egui::RichText::new("Advanced settings")
                .strong()
                .color(palette.text),
        )
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Only change these paths if you already manage a different Wine installation.",
                )
                .color(palette.muted),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Studio sign-in")
                    .strong()
                    .color(palette.text),
            );
            ui.radio_value(
                &mut self.login_mode,
                StudioLoginMode::EmbeddedWebView,
                "Inside Studio (recommended)",
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Keeps sign-in and the saved session inside the one Studio installation.",
                    )
                    .color(palette.muted),
                )
                .wrap(),
            );
            ui.radio_value(
                &mut self.login_mode,
                StudioLoginMode::ExternalBrowser,
                "Linux browser (backup)",
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Use this only if the sign-in page inside Studio cannot be used.",
                    )
                    .color(palette.muted),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            labeled_text_field(ui, "Wine command", &mut self.wine_binary, "wine", palette);
            labeled_text_field(
                ui,
                "Wine prefix",
                &mut self.wine_prefix,
                "Where Studio is installed",
                palette,
            );
            labeled_text_field(
                ui,
                "Studio path",
                &mut self.studio_executable,
                "Optional fallback path",
                palette,
            );
            ui.small(
                egui::RichText::new(
                    "MCP automatically uses the matching StudioMCP.exe in this prefix.",
                )
                .color(palette.muted),
            );
            ui.add_space(8.0);
            if action_button(
                ui,
                "Save settings",
                self.operation.is_none(),
                false,
                palette,
            )
            .clicked()
            {
                self.save_settings();
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("Config file: {}", self.config_path.display()))
                        .color(palette.muted),
                )
                .wrap(),
            );
        });
    }

    fn show_command_status(&mut self, ui: &mut egui::Ui, palette: Palette) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("Last action")
                    .strong()
                    .color(palette.text),
            );
            status_badge(ui, &self.status, palette, status_tone(&self.status));
            if ui
                .button(if self.show_command_output {
                    "Hide details"
                } else {
                    "Show details"
                })
                .clicked()
            {
                self.show_command_output = !self.show_command_output;
            }
        });
        if self.show_command_output {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Command details").color(palette.muted));
            egui::ScrollArea::vertical()
                .max_height(OUTPUT_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.output)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(ui.available_width())
                            .desired_rows(7),
                    );
                });
        }
    }

    fn logo_texture(&mut self, context: &egui::Context) -> egui::TextureHandle {
        if let Some(texture) = &self.logo {
            return texture.clone();
        }

        let texture = context.load_texture(
            APP_ID,
            self.logo_image.clone(),
            egui::TextureOptions::LINEAR,
        );
        self.logo = Some(texture.clone());
        texture
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_operation(context);
        let palette = palette();
        let logo = self.logo_texture(context);
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(palette.page))
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            let content_width = page_content_width(ui.available_width());
                            ui.set_width(content_width);
                            ui.add_space(PAGE_PADDING);

                            card_frame(palette).show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add(egui::Image::from_texture(&logo).fit_to_exact_size(
                                        egui::vec2(76.0, 76.0),
                                    ));
                                    ui.add_space(12.0);
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(APP_TITLE)
                                                .size(26.0)
                                                .strong()
                                                .color(palette.text),
                                        )
                                        .wrap(),
                                    );
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(
                                                "Install, launch, and connect Roblox Studio on Linux.",
                                            )
                                            .color(palette.muted),
                                        )
                                        .wrap(),
                                    );
                                    ui.add_space(4.0);
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(
                                                "Everything important is on this page. Advanced paths are kept below.",
                                            )
                                            .small()
                                            .color(palette.muted),
                                        )
                                        .wrap(),
                                    );
                                });
                            });

                            ui.add_space(SECTION_GAP);
                            card_frame(palette).show(ui, |ui| {
                                section_heading(
                                    ui,
                                    "Studio",
                                    "Use the main button to open the installed Studio version.",
                                    palette,
                                );
                                ui.add_space(8.0);
                                self.show_action_buttons(ui, palette);
                                ui.add_space(8.0);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(
                                            "Launch from this open window so Flatpak keeps Studio and MCP in the same running sandbox.",
                                        )
                                        .color(palette.muted),
                                    )
                                    .wrap(),
                                );
                                ui.add_space(4.0);
                                ui.add(
                                    egui::Label::new(
                                            egui::RichText::new(
                                                "Studio sign-in opens inside Studio by default. If that page cannot be used, Browser sign-in opens the same request in your Linux browser and returns it to Studio.",
                                            )
                                        .color(palette.info_text),
                                    )
                                    .wrap(),
                                );
                            });

                            ui.add_space(SECTION_GAP);
                            card_frame(palette).show(ui, |ui| {
                                self.show_mcp_controls(ui, context, palette);
                            });

                            ui.add_space(SECTION_GAP);
                            card_frame(palette).show(ui, |ui| {
                                self.show_settings(ui, palette);
                            });

                            ui.add_space(SECTION_GAP);
                            card_frame(palette).show(ui, |ui| {
                                self.show_command_status(ui, palette);
                            });

                            ui.add_space(PAGE_PADDING);
                        });
                    });
            });
    }
}

fn card_frame(palette: Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(12.0)
        .inner_margin(egui::Margin::same(CARD_PADDING))
}

fn section_heading(ui: &mut egui::Ui, title: &str, description: &str, palette: Palette) {
    ui.label(
        egui::RichText::new(title)
            .size(20.0)
            .strong()
            .color(palette.text),
    );
    ui.add(egui::Label::new(egui::RichText::new(description).color(palette.muted)).wrap());
}

fn status_badge(ui: &mut egui::Ui, label: &str, palette: Palette, tone: StatusTone) {
    let (background, foreground) = status_colors(palette, tone);
    egui::Frame::none()
        .fill(background)
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(label).strong().color(foreground)).wrap());
        });
}

fn page_content_width(available_width: f32) -> f32 {
    (available_width - PAGE_PADDING * 2.0).clamp(0.0, 980.0)
}

fn action_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    primary: bool,
    palette: Palette,
) -> egui::Response {
    let (fill, foreground, stroke) = if primary {
        (
            palette.accent,
            palette.accent_text,
            egui::Stroke::new(1.0_f32, palette.accent_hover),
        )
    } else {
        (
            palette.card_alt,
            palette.text,
            egui::Stroke::new(1.0_f32, palette.border),
        )
    };
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(14.0, 8.0);
        ui.add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(label).strong().color(foreground))
                .min_size(egui::vec2(0.0, CONTROL_HEIGHT))
                .fill(fill)
                .stroke(stroke)
                .rounding(8.0),
        )
    })
    .inner
}

fn labeled_text_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
    palette: Palette,
) {
    ui.vertical(|ui| {
        ui.add(egui::Label::new(egui::RichText::new(label).strong().color(palette.text)).wrap());
        let field_width = ui.available_width();
        ui.add_sized(
            [field_width, CONTROL_HEIGHT],
            egui::TextEdit::singleline(value).hint_text(hint),
        );
    });
}

fn classify_mcp_status(output: &str) -> String {
    if output.contains("passed state/tree tool checks") {
        "Connected to Studio".to_owned()
    } else if output.contains("no MCP session is attached") {
        "Studio needs MCP enabled".to_owned()
    } else if output.contains("Assistant plugin changed") {
        "Restart Studio after its Assistant update".to_owned()
    } else if output.contains("no open place was found") {
        "Open a Studio place first".to_owned()
    } else if output.contains("waiting for sign-in") {
        "Studio needs sign-in".to_owned()
    } else if output.contains("not running with an open place") {
        "Studio is not running with an open place".to_owned()
    } else if output.contains("Multiple Roblox Studio sessions") {
        "Multiple Studio sessions need a target".to_owned()
    } else if output.contains("StudioMCP.exe is missing") {
        "Studio MCP needs repair".to_owned()
    } else if output.is_empty() {
        "No diagnostic output".to_owned()
    } else {
        "MCP check failed; view diagnostics below".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{launcher_arguments, page_content_width, status_tone, StatusTone};
    use behave::prelude::*;
    use std::path::Path;

    behave! {
        "Showing launcher status" {
            "a successful launch request" {
                "uses the success tone" {
                    expect!(status_tone("Studio launch requested; finish sign-in in its window if needed"))
                        .to_equal(StatusTone::Success)?;
                }
            }

            "a failed command" {
                "uses the error tone" {
                    expect!(status_tone("Launch Studio failed with 1"))
                        .to_equal(StatusTone::Error)?;
                }
            }

            "an unchecked MCP connection" {
                "uses the warning tone" {
                    expect!(status_tone("Not checked")).to_equal(StatusTone::Warning)?;
                }
            }
        }

        "Running a launcher action from the GUI" {
            "a Launch Studio click" {
                "passes the config and launch command exactly once" {
                    let command = vec!["launch".to_owned()];
                    let arguments = launcher_arguments(Path::new("/tmp/launcher.ini"), &command)
                        .into_iter()
                        .map(|argument| argument.to_string_lossy().into_owned())
                        .collect::<Vec<_>>();

                    expect!(arguments).to_equal(vec![
                        "--config".to_owned(),
                        "/tmp/launcher.ini".to_owned(),
                        "launch".to_owned(),
                    ])?;
                }
            }
        }

        "Sizing the launcher page" {
            "a 900 pixel wide window" {
                "keeps equal breathing room on both sides" {
                    expect!(page_content_width(900.0)).to_equal(852.0)?;
                }
            }

            "a narrower 720 pixel window" {
                "shrinks the page content instead of clipping it" {
                    expect!(page_content_width(720.0)).to_equal(672.0)?;
                }
            }
        }
    }
}
