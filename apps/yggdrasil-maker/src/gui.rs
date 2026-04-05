use anyhow::Result;
use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, TextEdit};
use maker_app::{BuildInputs, MakerApp, StoredSetupSummary};
use maker_copy::preset_cards;
use maker_model::{BuildProfile, JourneyStage, SetupDocument};
use std::sync::mpsc::{self, Receiver};
use std::thread;

pub fn launch() -> Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "yggdrasil-maker",
        options,
        Box::new(|_cc| Ok(Box::new(MakerGui::bootstrap()?))),
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
}

enum GuiBuildMessage {
    Event(String),
    Finished(String),
    Failed(String),
}

impl MakerGui {
    fn bootstrap() -> Result<Self> {
        let app = MakerApp::new_for_current_platform()?;
        let mut saved_setups = app.setup_store().list()?;
        let current_setup = if let Some(first) = saved_setups.first() {
            app.setup_store().load(&first.setup_id)?
        } else {
            app.create_setup_document("Lab NAS".to_owned(), maker_model::PresetId::Nas, None, None)
        };
        let config_preview = app.emit_config_toml(&current_setup)?;
        saved_setups.sort_by(|left, right| right.modified_unix_secs.cmp(&left.modified_unix_secs));
        Ok(Self {
            app,
            saved_setups,
            current_setup,
            artifacts_dir: "./artifacts".to_owned(),
            repo_root: String::new(),
            plan_preview: String::new(),
            config_preview,
            build_log: Vec::new(),
            build_status: "Ready".to_owned(),
            build_result: String::new(),
            build_rx: None,
            build_running: false,
        })
    }

    fn refresh_saved_setups(&mut self) {
        if let Ok(saved) = self.app.setup_store().list() {
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
                self.refresh_saved_setups();
            }
            Err(error) => {
                self.build_status = format!("Save failed: {error}");
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
    fn render_root(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("saved_setups")
            .resizable(true)
            .min_width(220.0)
            .show(ctx, |ui| {
                ui.heading(RichText::new("Your Yggdrasils").font(FontId::proportional(24.0)));
                ui.add_space(8.0);
                if ui.button("New Setup").clicked() {
                    self.current_setup = self.app.create_setup_document(
                        "New Yggdrasil".to_owned(),
                        maker_model::PresetId::Nas,
                        None,
                        None,
                    );
                    self.refresh_previews();
                }
                ui.separator();
                let summaries = self.saved_setups.clone();
                for summary in summaries {
                    let label = format!("{}\n{}", summary.name, summary.journey_stage.label());
                    if ui
                        .selectable_label(summary.setup_id == self.current_setup.setup_id, label)
                        .clicked()
                    {
                        if let Ok(document) = self.app.setup_store().load(&summary.setup_id) {
                            self.current_setup = document;
                            self.refresh_previews();
                        }
                    }
                }
            });

        egui::SidePanel::right("utility_pane")
            .resizable(true)
            .min_width(340.0)
            .show(ctx, |ui| {
                ui.heading("Native Config");
                ui.add(
                    TextEdit::multiline(&mut self.config_preview)
                        .font(FontId::monospace(13.0))
                        .desired_rows(18),
                );
                ui.separator();
                ui.heading("Build Plan");
                ui.add(
                    TextEdit::multiline(&mut self.plan_preview)
                        .font(FontId::monospace(13.0))
                        .desired_rows(18),
                );
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                ui.heading(
                    RichText::new("What are you making?")
                        .font(FontId::proportional(34.0))
                        .color(Color32::from_rgb(240, 224, 196)),
                );
                ui.label("A guided studio for shaping a real Yggdrasil ISO without hiding the native config.");
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label("Name");
                    if ui.text_edit_singleline(&mut self.current_setup.setup.name).changed() {
                        self.refresh_previews();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Hostname");
                    if ui
                        .text_edit_singleline(&mut self.current_setup.setup.personalization.hostname)
                        .changed()
                    {
                        self.refresh_previews();
                    }
                });

                egui::ComboBox::from_label("Journey stage")
                    .selected_text(self.current_setup.journey_stage.label())
                    .show_ui(ui, |ui| {
                        for stage in [
                            JourneyStage::Outcome,
                            JourneyStage::Profile,
                            JourneyStage::Personalize,
                            JourneyStage::Review,
                            JourneyStage::Build,
                            JourneyStage::Boot,
                        ] {
                            if ui
                                .selectable_label(self.current_setup.journey_stage == stage, stage.label())
                                .clicked()
                            {
                                self.current_setup.journey_stage = stage;
                            }
                        }
                    });

                egui::ComboBox::from_label("Outcome preset")
                    .selected_text(self.current_setup.setup.preset.slug())
                    .show_ui(ui, |ui| {
                        for card in preset_cards() {
                            if ui
                                .selectable_label(self.current_setup.setup.preset == card.id, card.title)
                                .clicked()
                            {
                                self.current_setup.setup.preset = card.id;
                                self.refresh_previews();
                            }
                        }
                    });

                egui::ComboBox::from_label("Profile")
                    .selected_text(
                        self.current_setup
                            .setup
                            .profile_override
                            .unwrap_or_else(|| self.current_setup.setup.preset.recommended_profile())
                            .slug(),
                    )
                    .show_ui(ui, |ui| {
                        for profile in [BuildProfile::Server, BuildProfile::Kde, BuildProfile::Both] {
                            if ui.selectable_label(
                                self.current_setup.setup.profile_override == Some(profile),
                                profile.slug(),
                            ).clicked() {
                                self.current_setup.setup.profile_override = Some(profile);
                                self.refresh_previews();
                            }
                        }
                    });

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.current_setup.setup.hardware.with_nvidia, "NVIDIA");
                    ui.checkbox(&mut self.current_setup.setup.hardware.with_lts, "LTS kernel");
                    if ui.button("Refresh Preview").clicked() {
                        self.refresh_previews();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Artifacts");
                    ui.text_edit_singleline(&mut self.artifacts_dir);
                });
                ui.horizontal(|ui| {
                    ui.label("Repo override");
                    ui.text_edit_singleline(&mut self.repo_root);
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Save Setup").clicked() {
                        self.save_current_setup();
                    }
                    if ui
                        .add_enabled(!self.build_running, egui::Button::new("Build / Export"))
                        .clicked()
                    {
                        self.start_build();
                    }
                });

                ui.add_space(8.0);
                ui.label(RichText::new(&self.build_status).color(Color32::from_rgb(222, 194, 140)));
                if !self.build_result.is_empty() {
                    ui.separator();
                    ui.label("Artifact manifest");
                    ui.add(
                        TextEdit::multiline(&mut self.build_result)
                            .font(FontId::monospace(13.0))
                            .desired_rows(10),
                    );
                }
                if !self.build_log.is_empty() {
                    ui.separator();
                    ui.label("Build stream");
                    let mut joined = self.build_log.join("\n");
                    ui.add(
                        TextEdit::multiline(&mut joined)
                            .font(FontId::monospace(12.0))
                            .desired_rows(12),
                    );
                }
            });
        });
    }
}

impl eframe::App for MakerGui {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.poll_build_channel();
        let ctx = ui.ctx().clone();
        self.render_root(&ctx, frame);
    }
}
