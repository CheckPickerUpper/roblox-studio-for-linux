mod command_completion;
mod last_action_panel;
mod launcher_action;
mod mcp_connection_panel;

use crate::config::{load_config, LauncherConfig, StudioLoginMode};
use crate::error::LauncherError;
use crate::mcp::generate_client_configuration;
use command_completion::CommandCompletion;
use eframe::egui;
use last_action_panel::LastActionPanel;
use launcher_action::LauncherAction;
use mcp_connection_panel::{McpCheckCompletion, McpConnectionPanel};
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
    mcp_connection: McpConnectionPanel,
    last_action: LastActionPanel,
    logo_image: egui::ColorImage,
    logo: Option<egui::TextureHandle>,
    operation: Option<Operation>,
}

struct Operation {
    action: LauncherAction,
    result: Receiver<CommandCompletion>,
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
            mcp_connection: McpConnectionPanel::new(),
            last_action: LastActionPanel::ready(),
            logo_image,
            logo: None,
            operation: None,
        }
    }

    fn start_operation(&mut self, action: LauncherAction, command: Vec<String>) {
        if self.operation.is_some() {
            return;
        }

        let executable = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                self.last_action.could_not_find_launcher(
                    action,
                    format!("Could not start the launcher: {error}"),
                );
                return;
            }
        };
        let config_path = self.config_path.clone();
        let arguments = launcher_arguments(&config_path, &command);
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut child_command = Command::new(executable);
            child_command
                .args(arguments)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let result = match child_command.spawn() {
                Ok(child) => match child.wait_with_output() {
                    Ok(output) => CommandCompletion::from_output(output),
                    Err(error) => CommandCompletion::CouldNotCollect {
                        message: error.to_string(),
                    },
                },
                Err(error) => CommandCompletion::CouldNotStart {
                    message: error.to_string(),
                },
            };
            if let Err(error) = sender.send(result) {
                tracing::debug!(error = %error, "The launcher GUI stopped listening for command output");
            }
        });
        self.operation = Some(Operation {
            action,
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
        self.start_operation(LauncherAction::SaveSettings, command);
    }

    fn poll_operation(&mut self, context: &egui::Context) {
        let Some(operation) = &self.operation else {
            return;
        };
        let action = operation.action;
        match operation.result.try_recv() {
            Ok(completion) => {
                let minimize = action.minimizes_launcher_after_success() && completion.succeeded();
                if action == LauncherAction::CheckAiConnection {
                    match self.mcp_connection.complete_check(&completion) {
                        McpCheckCompletion::Recognized => self
                            .last_action
                            .diagnostic_completed(action, completion.diagnostics()),
                        McpCheckCompletion::Unreadable => {
                            self.last_action.complete(action, completion);
                        }
                    }
                } else {
                    self.last_action.complete(action, completion);
                }
                self.operation = None;
                if minimize {
                    context.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
            Err(mpsc::TryRecvError::Empty) => context.request_repaint_after(POLL_INTERVAL),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.last_action.worker_disconnected(action);
                self.operation = None;
            }
        }
    }

    fn show_action_buttons(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let busy = self.operation.is_some();
        ui.horizontal_wrapped(|ui| {
            if action_button(ui, "Launch Studio", !busy, true, palette).clicked() {
                self.start_operation(LauncherAction::LaunchStudio, vec!["launch".to_owned()]);
            }
            if action_button(ui, "Install / update", !busy, false, palette).clicked() {
                self.start_operation(LauncherAction::InstallStudio, vec!["install".to_owned()]);
            }
            if action_button(ui, "Check setup", !busy, false, palette).clicked() {
                self.start_operation(LauncherAction::CheckSetup, vec!["doctor".to_owned()]);
            }
            if action_button(ui, "Browser sign-in", !busy, false, palette).clicked() {
                self.start_operation(
                    LauncherAction::BrowserSignIn,
                    vec!["browser-login".to_owned()],
                );
            }
        });
        if let Some(operation) = &self.operation {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new(format!("{} is working…", operation.action.label()))
                        .color(palette.info_text),
                );
            });
        }
    }

    fn show_mcp_controls(&mut self, ui: &mut egui::Ui, context: &egui::Context, palette: Palette) {
        section_heading(
            ui,
            "AI connection",
            "Let an AI client work with the place currently open in Studio.",
            palette,
        );
        ui.add_space(6.0);
        status_badge(
            ui,
            &self.mcp_connection.status(),
            palette,
            self.mcp_connection.tone(),
        );

        if let Some(guidance) = self.mcp_connection.guidance() {
            ui.add_space(4.0);
            ui.add(egui::Label::new(egui::RichText::new(guidance).color(palette.muted)).wrap());
        }

        ui.add_space(8.0);
        let busy = self.operation.is_some();
        ui.horizontal_wrapped(|ui| {
            if action_button(ui, "Check connection", !busy, true, palette).clicked() {
                self.start_operation(
                    LauncherAction::CheckAiConnection,
                    vec!["mcp".to_owned(), "doctor".to_owned(), "--json".to_owned()],
                );
            }
            if action_button(ui, "Copy setup", !busy, false, palette).clicked() {
                match generate_client_configuration(&self.config_path) {
                    Ok(configuration) => {
                        context.copy_text(configuration.clone());
                        self.last_action.configuration_copied(configuration);
                    }
                    Err(error) => {
                        self.last_action
                            .configuration_copy_failed(error.to_string());
                    }
                }
            }
            if action_button(ui, "Details", true, false, palette).clicked() {
                self.mcp_connection.toggle_details();
            }
        });

        if self.mcp_connection.details_are_visible() {
            ui.add_space(6.0);
            egui::Frame::none()
                .fill(palette.input)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .rounding(8.0)
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Technical details")
                            .strong()
                            .color(palette.text),
                    );
                    let mut diagnostics = self.mcp_connection.diagnostics();
                    ui.add(
                        egui::TextEdit::multiline(&mut diagnostics)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(ui.available_width())
                            .desired_rows(6)
                            .interactive(false),
                    );
                });
        }
    }

    fn show_settings(&mut self, ui: &mut egui::Ui, palette: Palette) {
        egui::CollapsingHeader::new(
            egui::RichText::new("Sign-in settings")
                .strong()
                .color(palette.text),
        )
        .default_open(false)
        .show(ui, |ui| {
            ui.radio_value(
                &mut self.login_mode,
                StudioLoginMode::EmbeddedWebView,
                "Inside Studio (recommended)",
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Opens sign-in inside Studio.").color(palette.muted),
                )
                .wrap(),
            );
            ui.radio_value(
                &mut self.login_mode,
                StudioLoginMode::ExternalBrowser,
                "Web browser (if Studio sign-in is blank)",
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Opens sign-in in your default browser.")
                        .color(palette.muted),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            if action_button(
                ui,
                "Save sign-in setting",
                self.operation.is_none(),
                false,
                palette,
            )
            .clicked()
            {
                self.save_settings();
            }
        });
    }

    fn show_command_status(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let running_status = self
            .operation
            .as_ref()
            .map(|operation| format!("{} is running…", operation.action.label()));
        let status = running_status.unwrap_or_else(|| self.last_action.status());
        let tone = if self.operation.is_some() {
            StatusTone::Info
        } else {
            self.last_action.tone()
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("Last action")
                    .strong()
                    .color(palette.text),
            );
            status_badge(ui, &status, palette, tone);
            if ui
                .button(if self.last_action.details_are_visible() {
                    "Hide details"
                } else {
                    "Show details"
                })
                .clicked()
            {
                self.last_action.toggle_details();
            }
        });
        if self.last_action.details_are_visible() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Details").color(palette.muted));
            egui::ScrollArea::vertical()
                .max_height(OUTPUT_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut details = self.last_action.details();
                    ui.add(
                        egui::TextEdit::multiline(&mut details)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(ui.available_width())
                            .desired_rows(7)
                            .interactive(false),
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
                                            egui::RichText::new("Run Roblox Studio on Linux.")
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
                                    "Open the installed Studio version.",
                                    palette,
                                );
                                ui.add_space(8.0);
                                self.show_action_buttons(ui, palette);
                                ui.add_space(8.0);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(
                                            "Sign-in opens in Studio. Use Browser sign-in only if needed.",
                                        )
                                        .color(palette.muted),
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

#[cfg(test)]
mod tests {
    use super::command_completion::CommandCompletion;
    use super::last_action_panel::LastActionPanel;
    use super::launcher_action::LauncherAction;
    use super::mcp_connection_panel::McpConnectionPanel;
    use super::{launcher_arguments, page_content_width, StatusTone};
    use behave::prelude::*;
    use std::path::Path;

    behave! {
        "Showing launcher status" {
            "a successful launch request" {
                "uses the success tone" {
                    let mut panel = LastActionPanel::ready();
                    panel.complete(
                        LauncherAction::LaunchStudio,
                        CommandCompletion::Succeeded {
                            stdout: String::new(),
                            stderr: String::new(),
                        },
                    );
                    expect!(panel.tone()).to_equal(StatusTone::Success)?;
                }
            }

            "a failed command" {
                "uses the error tone" {
                    let mut panel = LastActionPanel::ready();
                    panel.complete(
                        LauncherAction::LaunchStudio,
                        CommandCompletion::Failed {
                            exit_code: Some(1),
                            stdout: String::new(),
                            stderr: String::new(),
                        },
                    );
                    expect!(panel.tone()).to_equal(StatusTone::Error)?;
                }
            }

            "an unchecked MCP connection" {
                "uses the warning tone" {
                    expect!(McpConnectionPanel::new().tone()).to_equal(StatusTone::Warning)?;
                }
            }

            "Studio is not running" {
                "uses the warning tone" {
                    let mut panel = McpConnectionPanel::new();
                    panel.complete_check(&CommandCompletion::Failed {
                        exit_code: Some(1),
                        stdout: "{\"status\":\"studio_not_running\"}".to_owned(),
                        stderr: String::new(),
                    });
                    expect!(panel.status())
                        .to_equal("Studio is not running with an open place".to_owned())?;
                    expect!(panel.tone()).to_equal(StatusTone::Warning)?;
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
