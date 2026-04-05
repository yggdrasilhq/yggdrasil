use anyhow::Result;
use eframe::egui::{
    self, Align, Color32, FontId, Frame, Layout, Margin, RichText, Stroke, TextEdit, Vec2,
};
use maker_app::{BuildInputs, MakerApp, StoredSetupSummary};
use maker_copy::preset_cards;
use maker_model::{BuildProfile, JourneyStage, PresetId, SetupDocument};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const SHELL_TEXT: Color32 = Color32::from_rgb(44, 57, 73);
const SHELL_MUTED: Color32 = Color32::from_rgb(108, 123, 141);
const SHELL_LINE: Color32 = Color32::from_rgb(219, 228, 236);
const SHELL_BLUE: Color32 = Color32::from_rgb(77, 122, 232);
const SHELL_BLUE_SOFT: Color32 = Color32::from_rgb(231, 239, 252);
const SHELL_RAIL: Color32 = Color32::from_rgb(237, 243, 247);
const SHELL_PANEL: Color32 = Color32::from_rgb(251, 253, 254);
const SHELL_PANEL_ALT: Color32 = Color32::from_rgb(245, 249, 252);

pub fn launch() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 900.0])
            .with_min_inner_size([960.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "yggdrasil-maker",
        options,
        Box::new(|cc| {
            configure_visuals(&cc.egui_ctx);
            Ok(Box::new(MakerGui::bootstrap()?))
        }),
    )
    .map_err(|error| anyhow::anyhow!(error))
}

struct MakerGui {
    app: MakerApp,
    saved_setups: Vec<StoredSetupSummary>,
    current_setup: SetupDocument,
    artifacts_dir: String,
    repo_root: String,
    plan_preview: String,
    config_preview: String,
    build_log: Vec<String>,
    build_status: String,
    build_result: String,
    build_rx: Option<Receiver<GuiBuildMessage>>,
    build_running: bool,
    utility_tab: UtilityTab,
}

enum GuiBuildMessage {
    Event(String),
    Finished(String),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UtilityTab {
    Config,
    Plan,
    Stream,
}

impl UtilityTab {
    fn label(self) -> &'static str {
        match self {
            Self::Config => "Config",
            Self::Plan => "Plan",
            Self::Stream => "Build",
        }
    }
}

impl MakerGui {
    fn bootstrap() -> Result<Self> {
        let app = MakerApp::new_for_current_platform()?;
        let mut saved_setups = app.setup_store().list()?;
        let current_setup = if let Some(first) = saved_setups.first() {
            app.setup_store().load(&first.setup_id)?
        } else {
            app.create_setup_document("Lab NAS".to_owned(), PresetId::Nas, None, None)
        };
        saved_setups.sort_by(|left, right| right.modified_unix_secs.cmp(&left.modified_unix_secs));

        let config_preview = app.emit_config_toml(&current_setup)?;
        let plan_preview = app
            .plan_build(BuildInputs {
                setup_document: current_setup.clone(),
                artifacts_dir: "./artifacts".into(),
                authorized_keys_file: None,
                host_keys_dir: None,
                repo_root: None,
                skip_smoke: false,
            })
            .and_then(|plan| serde_json::to_string_pretty(&plan).map_err(|error| error.into()))
            .unwrap_or_else(|error| format!("Build plan unavailable:\n{error}"));

        Ok(Self {
            app,
            saved_setups,
            current_setup,
            artifacts_dir: "./artifacts".to_owned(),
            repo_root: String::new(),
            plan_preview,
            config_preview,
            build_log: Vec::new(),
            build_status: "Ready".to_owned(),
            build_result: String::new(),
            build_rx: None,
            build_running: false,
            utility_tab: UtilityTab::Config,
        })
    }

    fn refresh_saved_setups(&mut self) {
        if let Ok(mut saved) = self.app.setup_store().list() {
            saved.sort_by(|left, right| right.modified_unix_secs.cmp(&left.modified_unix_secs));
            self.saved_setups = saved;
        }
    }

    fn refresh_previews(&mut self) {
        self.config_preview = self
            .app
            .emit_config_toml(&self.current_setup)
            .unwrap_or_else(|error| format!("Config preview unavailable:\n{error}"));
        self.plan_preview = self
            .app
            .plan_build(self.build_inputs())
            .and_then(|plan| serde_json::to_string_pretty(&plan).map_err(|error| error.into()))
            .unwrap_or_else(|error| format!("Build plan unavailable:\n{error}"));
    }

    fn save_current_setup(&mut self) {
        match self.app.setup_store().save(&self.current_setup) {
            Ok(path) => {
                self.build_status = format!("Saved {}", path.display());
                self.utility_tab = UtilityTab::Plan;
                self.refresh_saved_setups();
            }
            Err(error) => {
                self.build_status = format!("Save failed: {error}");
                self.utility_tab = UtilityTab::Stream;
            }
        }
    }

    fn build_inputs(&self) -> BuildInputs {
        BuildInputs {
            setup_document: self.current_setup.clone(),
            artifacts_dir: self.artifacts_dir.clone().into(),
            authorized_keys_file: None,
            host_keys_dir: None,
            repo_root: if self.repo_root.trim().is_empty() {
                None
            } else {
                Some(self.repo_root.clone().into())
            },
            skip_smoke: false,
        }
    }

    fn start_build(&mut self) {
        if self.build_running {
            return;
        }

        self.build_running = true;
        self.build_log.clear();
        self.build_result.clear();
        self.build_status = "Building...".to_owned();
        self.utility_tab = UtilityTab::Stream;

        let inputs = self.build_inputs();
        let (tx, rx) = mpsc::channel();
        self.build_rx = Some(rx);

        thread::spawn(move || {
            let app = match MakerApp::new_for_current_platform() {
                Ok(app) => app,
                Err(error) => {
                    let _ = tx.send(GuiBuildMessage::Failed(error.to_string()));
                    return;
                }
            };

            let result = app.run_build(inputs, |event| {
                let line = serde_json::to_string(&event).unwrap_or_else(|_| format!("{event:?}"));
                let _ = tx.send(GuiBuildMessage::Event(line));
            });

            match result {
                Ok(outcome) => {
                    let payload = serde_json::to_string_pretty(&outcome.manifest)
                        .unwrap_or_else(|error| error.to_string());
                    let _ = tx.send(GuiBuildMessage::Finished(payload));
                }
                Err(error) => {
                    let _ = tx.send(GuiBuildMessage::Failed(error.to_string()));
                }
            }
        });
    }

    fn poll_build_channel(&mut self) {
        let mut finished = false;
        if let Some(rx) = self.build_rx.as_ref() {
            while let Ok(message) = rx.try_recv() {
                match message {
                    GuiBuildMessage::Event(line) => self.build_log.push(line),
                    GuiBuildMessage::Finished(payload) => {
                        self.build_running = false;
                        self.build_status = "Build finished".to_owned();
                        self.build_result = payload;
                        finished = true;
                    }
                    GuiBuildMessage::Failed(error) => {
                        self.build_running = false;
                        self.build_status = format!("Build failed: {error}");
                        finished = true;
                    }
                }
            }
        }
        if finished {
            self.build_rx = None;
        }
    }

    #[allow(deprecated)]
    fn render_root(&mut self, ctx: &egui::Context) {
        let viewport_width = ctx.content_rect().width();
        let compact_shell = viewport_width < 1080.0;
        let left_width = if compact_shell {
            180.0
        } else if viewport_width < 1180.0 {
            196.0
        } else {
            224.0
        };
        let right_width = if compact_shell {
            216.0
        } else if viewport_width < 1180.0 {
            236.0
        } else {
            280.0
        };
        let utility_tab_width = if compact_shell { 60.0 } else { 78.0 };
        let canvas_max_width = if compact_shell {
            560.0
        } else if viewport_width < 1180.0 {
            600.0
        } else {
            760.0
        };

        egui::SidePanel::left("saved_setups")
            .resizable(false)
            .min_width(left_width)
            .max_width(left_width)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Your Yggdrasils")
                        .font(FontId::proportional(28.0))
                        .color(SHELL_TEXT),
                );
                ui.label(RichText::new("Saved setups and journey progress.").color(SHELL_MUTED));
                ui.add_space(10.0);

                if ui
                    .add_sized([left_width - 12.0, 40.0], primary_button("New Setup"))
                    .clicked()
                {
                    self.current_setup = self.app.create_setup_document(
                        "New Yggdrasil".to_owned(),
                        PresetId::Nas,
                        None,
                        None,
                    );
                    self.refresh_previews();
                    self.utility_tab = UtilityTab::Config;
                }

                ui.add_space(12.0);
                for summary in self.saved_setups.clone() {
                    let selected = summary.setup_id == self.current_setup.setup_id;
                    let response = rail_card_button(ui, &summary.name, selected, left_width - 12.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(summary.journey_stage.label()).color(if selected {
                                SHELL_BLUE
                            } else {
                                SHELL_MUTED
                            }),
                        );
                        ui.label(
                            RichText::new(format!("• {}", summary.slug))
                                .color(Color32::from_rgb(128, 139, 152)),
                        );
                    });
                    ui.add_space(8.0);

                    if response.clicked() {
                        if let Ok(document) = self.app.setup_store().load(&summary.setup_id) {
                            self.current_setup = document;
                            self.refresh_previews();
                        }
                    }
                }
            });

        egui::SidePanel::right("utility_pane")
            .resizable(false)
            .min_width(right_width)
            .max_width(right_width)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(SHELL_RAIL)
                    .stroke(Stroke::new(1.0, SHELL_LINE))
                    .corner_radius(18.0)
                    .inner_margin(Margin::same(16))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Shell Truth")
                                .font(FontId::proportional(22.0))
                                .color(SHELL_TEXT),
                        );
                        ui.label(
                            RichText::new(
                                "Keep one honest surface open: config, plan, or live build output.",
                            )
                            .color(SHELL_MUTED),
                        );
                        ui.add_space(8.0);

                        ui.horizontal_wrapped(|ui| {
                            for tab in [UtilityTab::Config, UtilityTab::Plan, UtilityTab::Stream] {
                                if segmented_chip(
                                    ui,
                                    tab.label(),
                                    self.utility_tab == tab,
                                    [utility_tab_width, 34.0],
                                )
                                .clicked()
                                {
                                    self.utility_tab = tab;
                                }
                            }
                        });

                        ui.add_space(10.0);
                        match self.utility_tab {
                            UtilityTab::Config => {
                                ui.label(
                                    RichText::new("Native config preview.").color(SHELL_MUTED),
                                );
                                ui.add_space(6.0);
                                ui.add(
                                    TextEdit::multiline(&mut self.config_preview)
                                        .font(FontId::monospace(13.0))
                                        .desired_rows(24),
                                );
                            }
                            UtilityTab::Plan => {
                                ui.label(
                                    RichText::new("Exact invocation and output contract.")
                                        .color(SHELL_MUTED),
                                );
                                ui.add_space(6.0);
                                ui.add(
                                    TextEdit::multiline(&mut self.plan_preview)
                                        .font(FontId::monospace(13.0))
                                        .desired_rows(24),
                                );
                            }
                            UtilityTab::Stream => {
                                ui.label(
                                    RichText::new(
                                        "Live build status and resulting artifact manifest.",
                                    )
                                    .color(SHELL_MUTED),
                                );
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(&self.build_status)
                                        .font(FontId::proportional(18.0))
                                        .color(SHELL_BLUE),
                                );
                                ui.add_space(10.0);

                                if !self.build_result.is_empty() {
                                    ui.label(
                                        RichText::new("Artifact Manifest")
                                            .font(FontId::proportional(18.0))
                                            .color(SHELL_TEXT),
                                    );
                                    ui.add_space(6.0);
                                    ui.add(
                                        TextEdit::multiline(&mut self.build_result)
                                            .font(FontId::monospace(13.0))
                                            .desired_rows(10),
                                    );
                                    ui.add_space(10.0);
                                }

                                ui.label(
                                    RichText::new("Build stream")
                                        .font(FontId::proportional(18.0))
                                        .color(SHELL_TEXT),
                                );
                                ui.add_space(6.0);
                                let mut stream = if self.build_log.is_empty() {
                                    "No build activity yet.".to_owned()
                                } else {
                                    self.build_log.join("\n")
                                };
                                ui.add(
                                    TextEdit::multiline(&mut stream)
                                        .font(FontId::monospace(13.0))
                                        .desired_rows(16),
                                );
                            }
                        }
                    });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(8.0);
                let content_width = ui.available_width().min(canvas_max_width);
                let leading_space = ((ui.available_width() - content_width) * 0.5).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(leading_space);
                    ui.allocate_ui_with_layout(
                        Vec2::new(content_width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            let canvas_inner_width = (content_width - 56.0).max(420.0);
                            Frame::new()
                                .fill(SHELL_PANEL)
                                .stroke(Stroke::new(1.0, SHELL_LINE))
                                .corner_radius(26.0)
                                .inner_margin(Margin::same(28))
                                .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new("GUIDED STUDIO")
                                        .font(FontId::proportional(14.0))
                                        .color(SHELL_BLUE),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Current stage: {}",
                                        self.current_setup.journey_stage.label()
                                    ))
                                    .color(SHELL_MUTED),
                                );
                            });
                            ui.add_space(6.0);
                            ui.add_sized(
                                [canvas_inner_width, 64.0],
                                egui::Label::new(
                                    RichText::new("What are you making?")
                                        .font(FontId::proportional(42.0))
                                        .color(SHELL_TEXT),
                                )
                                .wrap(),
                            );
                            ui.label(
                                RichText::new(
                                    "Shape the outcome first. Keep the native config real. Build with enough proof that the result feels serious.",
                                )
                                .color(SHELL_MUTED),
                            );
                            ui.add_space(18.0);

                            ui.horizontal_wrapped(|ui| {
                                for stage in [
                                    JourneyStage::Outcome,
                                    JourneyStage::Profile,
                                    JourneyStage::Personalize,
                                    JourneyStage::Review,
                                    JourneyStage::Build,
                                    JourneyStage::Boot,
                                ] {
                                    if segmented_chip(
                                        ui,
                                        stage.label(),
                                        self.current_setup.journey_stage == stage,
                                        [88.0, 34.0],
                                    )
                                    .clicked()
                                    {
                                        self.current_setup.journey_stage = stage;
                                    }
                                }
                            });

                            ui.add_space(20.0);
                            studio_section(ui, "Outcome", "Pick the machine you want to bring to life.", |ui| {
                                let card_width = (ui.available_width() - 12.0).max(220.0);
                                for row in preset_cards().chunks(1) {
                                    ui.horizontal_wrapped(|ui| {
                                        for card in row {
                                            if preset_card(
                                                ui,
                                                card.title,
                                                card.summary,
                                                self.current_setup.setup.preset == card.id,
                                                card_width,
                                            )
                                            .clicked()
                                            {
                                                self.current_setup.setup.preset = card.id;
                                                self.refresh_previews();
                                            }
                                        }
                                    });
                                    ui.add_space(10.0);
                                }
                            });

                            studio_section(
                                ui,
                                "Identity",
                                "Name the setup and give the future host a real identity.",
                                |ui| {
                                    egui::Grid::new("identity-grid")
                                        .num_columns(2)
                                        .spacing([16.0, 12.0])
                                        .show(ui, |ui| {
                                            ui.label("Setup name");
                                            if ui
                                                .add_sized(
                                                    [300.0, 34.0],
                                                    TextEdit::singleline(&mut self.current_setup.setup.name),
                                                )
                                                .changed()
                                            {
                                                self.refresh_previews();
                                            }
                                            ui.end_row();

                                            ui.label("Hostname");
                                            if ui
                                                .add_sized(
                                                    [300.0, 34.0],
                                                    TextEdit::singleline(
                                                        &mut self.current_setup.setup.personalization.hostname,
                                                    ),
                                                )
                                                .changed()
                                            {
                                                self.refresh_previews();
                                            }
                                            ui.end_row();
                                        });
                                },
                            );

                            studio_section(
                                ui,
                                "Build posture",
                                "Choose the public profile, hardware stance, and output paths.",
                                |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        for profile in [
                                            BuildProfile::Server,
                                            BuildProfile::Kde,
                                            BuildProfile::Both,
                                        ] {
                                            let selected = self.current_setup.setup.profile_override
                                                == Some(profile)
                                                || (self.current_setup.setup.profile_override.is_none()
                                                    && self.current_setup.setup.preset.recommended_profile()
                                                        == profile);
                                            if segmented_chip(ui, profile.slug(), selected, [104.0, 38.0]).clicked()
                                            {
                                                self.current_setup.setup.profile_override = Some(profile);
                                                self.refresh_previews();
                                            }
                                        }
                                    });

                                    ui.add_space(12.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.checkbox(
                                            &mut self.current_setup.setup.hardware.with_nvidia,
                                            "NVIDIA path",
                                        );
                                        ui.checkbox(
                                            &mut self.current_setup.setup.hardware.with_lts,
                                            "LTS kernel",
                                        );
                                        if ui.button("Refresh preview").clicked() {
                                            self.refresh_previews();
                                        }
                                    });

                                    ui.add_space(12.0);
                                    egui::Grid::new("build-path-grid")
                                        .num_columns(2)
                                        .spacing([16.0, 12.0])
                                        .show(ui, |ui| {
                                            ui.label("Artifacts");
                                            ui.add_sized(
                                                [300.0, 34.0],
                                                TextEdit::singleline(&mut self.artifacts_dir),
                                            );
                                            ui.end_row();

                                            ui.label("Repo override");
                                            ui.add_sized(
                                                [300.0, 34.0],
                                                TextEdit::singleline(&mut self.repo_root),
                                            );
                                            ui.end_row();
                                        });
                                },
                            );

                            studio_section(
                                ui,
                                "Commit",
                                "Save the setup or run the next honest action for this machine.",
                                |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        if ui
                                            .add_sized([140.0, 42.0], egui::Button::new("Save setup"))
                                            .clicked()
                                        {
                                            self.save_current_setup();
                                        }

                                        if ui
                                            .add_enabled(
                                                !self.build_running,
                                                primary_button("Build / export")
                                                    .min_size(Vec2::new(160.0, 42.0)),
                                            )
                                            .clicked()
                                        {
                                            self.start_build();
                                        }

                                        ui.label(
                                            RichText::new("Build output lives in the right rail.")
                                                .color(SHELL_MUTED),
                                        );
                                    });

                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(&self.build_status)
                                            .font(FontId::proportional(18.0))
                                            .color(SHELL_BLUE),
                                    );
                                },
                            );
                                });
                        },
                    );
                });
            });
        });
    }
}

impl eframe::App for MakerGui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_build_channel();
        let ctx = ui.ctx().clone();
        self.render_root(&ctx);
    }
}

fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.override_text_color = Some(SHELL_TEXT);
    visuals.panel_fill = Color32::from_rgb(228, 236, 241);
    visuals.window_fill = Color32::from_rgb(224, 233, 238);
    visuals.extreme_bg_color = SHELL_PANEL;
    visuals.code_bg_color = SHELL_PANEL_ALT;
    visuals.faint_bg_color = SHELL_RAIL;
    visuals.widgets.noninteractive.bg_fill = SHELL_PANEL_ALT;
    visuals.widgets.noninteractive.bg_stroke.color = SHELL_LINE;
    visuals.widgets.inactive.bg_fill = SHELL_PANEL;
    visuals.widgets.inactive.bg_stroke.color = SHELL_LINE;
    visuals.widgets.hovered.bg_fill = SHELL_BLUE_SOFT;
    visuals.widgets.hovered.bg_stroke.color = Color32::from_rgb(133, 165, 240);
    visuals.widgets.active.bg_fill = SHELL_BLUE;
    visuals.widgets.active.bg_stroke.color = SHELL_BLUE;
    visuals.selection.bg_fill = Color32::from_rgb(211, 225, 255);
    visuals.selection.stroke.color = SHELL_BLUE;
    visuals.widgets.inactive.corner_radius = 10.0.into();
    visuals.widgets.hovered.corner_radius = 10.0.into();
    visuals.widgets.active.corner_radius = 10.0.into();
    visuals.widgets.noninteractive.corner_radius = 12.0.into();
    ctx.set_visuals(visuals);

    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = Vec2::new(12.0, 12.0);
    style.spacing.button_padding = Vec2::new(12.0, 9.0);
    style.spacing.interact_size = Vec2::new(44.0, 32.0);
    style.spacing.text_edit_width = 260.0;
    ctx.set_global_style(style);
}

fn segmented_chip(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    size: [f32; 2],
) -> egui::Response {
    Frame::new()
        .fill(if selected {
            SHELL_BLUE
        } else {
            SHELL_PANEL_ALT
        })
        .stroke(Stroke::new(
            1.0,
            if selected { SHELL_BLUE } else { SHELL_LINE },
        ))
        .corner_radius(12.0)
        .inner_margin(Margin::same(0))
        .show(ui, |ui| {
            ui.add_sized(
                size,
                egui::Button::new(RichText::new(label).color(if selected {
                    Color32::WHITE
                } else {
                    SHELL_MUTED
                })),
            )
        })
        .inner
}

fn rail_card_button(ui: &mut egui::Ui, title: &str, selected: bool, width: f32) -> egui::Response {
    Frame::new()
        .fill(if selected {
            SHELL_BLUE_SOFT
        } else {
            SHELL_PANEL_ALT
        })
        .stroke(Stroke::new(
            1.0,
            if selected {
                Color32::from_rgb(128, 160, 239)
            } else {
                SHELL_LINE
            },
        ))
        .corner_radius(14.0)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.add_sized(
                [width, 52.0],
                egui::Button::new(
                    RichText::new(title)
                        .font(FontId::proportional(18.0))
                        .color(SHELL_TEXT),
                ),
            )
        })
        .inner
}

fn preset_card(
    ui: &mut egui::Ui,
    title: &str,
    summary: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    Frame::new()
        .fill(if selected {
            SHELL_BLUE_SOFT
        } else {
            SHELL_PANEL_ALT
        })
        .stroke(Stroke::new(
            1.0,
            if selected {
                Color32::from_rgb(128, 160, 239)
            } else {
                SHELL_LINE
            },
        ))
        .corner_radius(16.0)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.add_sized(
                [width, 104.0],
                egui::Button::new(
                    RichText::new(format!("{title}\n{summary}"))
                        .color(SHELL_TEXT)
                        .font(FontId::proportional(18.0)),
                ),
            )
        })
        .inner
}

fn primary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(Color32::WHITE))
}

fn studio_section(
    ui: &mut egui::Ui,
    title: &str,
    body: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.label(
        RichText::new(title)
            .font(FontId::proportional(24.0))
            .color(SHELL_TEXT),
    );
    ui.label(RichText::new(body).color(SHELL_MUTED));
    ui.add_space(14.0);
    add_contents(ui);
    ui.add_space(18.0);
    ui.separator();
    ui.add_space(8.0);
}
