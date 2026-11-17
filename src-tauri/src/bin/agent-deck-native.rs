#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "native/backend.rs"]
mod backend;
#[path = "native/screen.rs"]
mod screen;

use agent_deck::{
    diagnostics, gjc_history,
    model::{self, Agent, Candidate, Result, Router, Runtime, Session},
    native_gjc::{self, HistoryLocation, Preset, PRESETS},
    registry, router,
    storage::Table,
};
use backend::{Backend, Mode};
use eframe::egui::{self, Color32};
use screen::Screen;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{mpsc, Arc},
};

fn main() {
    let data = data_directory();
    let log = dirs::data_local_dir().map(|root| root.join("local.agentdeck.desktop/logs"));
    diagnostics::init(log.as_deref());
    let result = data.and_then(Backend::open).map(Arc::new);
    let result = eframe::run_native(
        "Agent Deck",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1250.0, 820.0])
                .with_min_inner_size([760.0, 520.0]),
            ..Default::default()
        },
        Box::new(move |context| {
            install_fonts(&context.egui_ctx);
            configure_style(&context.egui_ctx);
            match result {
                Ok(backend) => Ok(Box::new(Deck::new(backend))),
                Err(error) => Ok(Box::new(StartupError(error))),
            }
        }),
    );
    if result.is_err() {
        diagnostics::warn("native.window_start_failed", None);
    }
}

fn configure_style(ctx: &egui::Context) {
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(9.0, 5.0);
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(21, 24, 31);
        style.visuals.window_fill = Color32::from_rgb(28, 32, 41);
        style.visuals.selection.bg_fill = Color32::from_rgb(42, 77, 117);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(21.0));
    });
}

fn data_directory() -> Result<PathBuf> {
    #[cfg(feature = "test-fixture")]
    if let Some(path) = std::env::var_os("AGENT_DECK_TEST_DATA_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(path);
    }
    dirs::config_dir()
        .map(|dir| dir.join("local.agentdeck.desktop"))
        .ok_or("Agent Deck app data is unavailable".into())
}

fn install_fonts(ctx: &egui::Context) {
    if let Some(root) = std::env::var_os("SystemRoot") {
        let font = PathBuf::from(root).join("Fonts/malgun.ttf");
        if let Ok(data) = std::fs::read(font) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "windows-korean".into(),
                Arc::new(egui::FontData::from_owned(data)),
            );
            for family in fonts.families.values_mut() {
                family.push("windows-korean".into());
            }
            ctx.set_fonts(fonts);
        }
    }
}

struct StartupError(String);
impl eframe::App for StartupError {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Agent Deck could not start");
            ui.label(&self.0);
        });
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Default)]
enum Page {
    Terminal,
    #[default]
    New,
    History,
    Settings,
    Help,
}

enum Response {
    Launched(Box<Session>),
    Runtime,
    Inspected(Candidate),
    Catalog(Vec<String>),
    History(gjc_history::Catalog),
    HistoryFile(gjc_history::Entry),
    Path(BrowseTarget, Option<PathBuf>),
    Closed(String),
    Quit,
}

#[derive(Clone, Copy)]
enum BrowseTarget {
    Working,
    Models,
    Config,
    HistoryRoot,
    AgentDirectory,
    Conversation,
    Executable,
}

struct Deck {
    backend: Arc<Backend>,
    responses: mpsc::Receiver<Result<Response>>,
    send: mpsc::Sender<Result<Response>>,
    sessions: Vec<Session>,
    tabs: Vec<Screen>,
    active: String,
    focus: bool,
    sidebar: bool,
    page: Page,
    error: String,
    busy: bool,
    close_requested: bool,
    allow_close: bool,
    closing_tab: Option<String>,
    runtimes: Vec<Runtime>,
    routers: Vec<Router>,
    presets: BTreeMap<String, Preset>,
    preset: Preset,
    catalog: Vec<String>,
    template_launch: bool,
    cwd: String,
    name: String,
    token: String,
    runtime_path: String,
    candidate: Option<Candidate>,
    register_agent: Agent,
    router_draft: Router,
    port_text: String,
    openai: bool,
    anthropic: bool,
    history_agent: String,
    history_file: String,
    history: Vec<gjc_history::Entry>,
    history_limited: bool,
    selected_history: Option<usize>,
    replacement: String,
}

impl Deck {
    fn new(backend: Arc<Backend>) -> Self {
        let (send, responses) = mpsc::channel();
        let mut app = Self {
            backend,
            responses,
            send,
            sessions: vec![],
            tabs: vec![],
            active: String::new(),
            focus: false,
            sidebar: true,
            page: Page::New,
            error: String::new(),
            busy: false,
            close_requested: false,
            allow_close: false,
            closing_tab: None,
            runtimes: vec![],
            routers: vec![],
            presets: BTreeMap::new(),
            preset: Preset::default(),
            catalog: vec![],
            template_launch: false,
            cwd: String::new(),
            name: String::new(),
            token: String::new(),
            runtime_path: String::new(),
            candidate: None,
            register_agent: Agent::Gajae,
            router_draft: new_router(),
            port_text: String::new(),
            openai: true,
            anthropic: false,
            history_agent: String::new(),
            history_file: String::new(),
            history: vec![],
            history_limited: false,
            selected_history: None,
            replacement: String::new(),
        };
        app.refresh();
        if let Ok(Some(preferences)) = app
            .backend
            .store
            .get::<model::Preferences>(Table::Settings, "terminal")
        {
            app.cwd = preferences.last_cwd;
            app.preset.runtime_id = preferences.last_runtime;
        }
        app
    }

    fn refresh(&mut self) {
        let result = (|| -> Result<()> {
            self.sessions = self.backend.store.list(Table::Sessions)?;
            self.runtimes = self.backend.store.list(Table::Runtimes)?;
            self.routers = self.backend.store.list(Table::Routers)?;
            self.presets = self
                .backend
                .store
                .get(Table::Settings, PRESETS)?
                .unwrap_or_default();
            Ok(())
        })();
        if let Err(error) = result {
            self.error = error;
        }
    }

    fn work(
        &mut self,
        ctx: &egui::Context,
        f: impl FnOnce(Arc<Backend>) -> Result<Response> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error.clear();
        let backend = self.backend.clone();
        let sender = self.send.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = f(backend);
            if result.is_err() {
                diagnostics::warn("native.operation_failed", None);
            }
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(result) = self.responses.try_recv() {
            self.busy = false;
            match result {
                Ok(Response::Launched(session)) => {
                    self.active.clone_from(&session.id);
                    self.tabs.push(Screen::new(*session));
                    self.page = Page::Terminal;
                    self.focus = true;
                    self.refresh();
                }
                Ok(Response::Inspected(candidate)) => self.candidate = Some(candidate),
                Ok(Response::Catalog(models)) => self.catalog = models,
                Ok(Response::History(catalog)) => {
                    self.history = catalog.entries;
                    self.history_limited = catalog.limited;
                    self.selected_history = None;
                }
                Ok(Response::HistoryFile(entry)) => {
                    self.history = vec![entry];
                    self.history_limited = false;
                    self.selected_history = Some(0);
                }
                Ok(Response::Path(target, Some(path))) => {
                    let value = path.to_string_lossy().into_owned();
                    match target {
                        BrowseTarget::Working => self.cwd = value,
                        BrowseTarget::Models => self.preset.models_template = value,
                        BrowseTarget::Config => self.preset.config_template = value,
                        BrowseTarget::HistoryRoot => self.preset.history_root = value,
                        BrowseTarget::AgentDirectory => {
                            self.history_agent = value;
                            self.history.clear();
                            self.selected_history = None;
                        }
                        BrowseTarget::Conversation => self.history_file = value,
                        BrowseTarget::Executable => {
                            self.runtime_path = value;
                            self.candidate = None;
                        }
                    }
                }
                Ok(Response::Path(_, None)) => {}
                Ok(Response::Runtime) => {
                    self.candidate = None;
                    self.refresh();
                }
                Ok(Response::Closed(id)) => {
                    self.tabs.retain(|tab| tab.session.id != id);
                    if self.active == id {
                        self.active = self
                            .tabs
                            .last()
                            .map(|tab| tab.session.id.clone())
                            .unwrap_or_default();
                    }
                    self.closing_tab = None;
                    if self.tabs.is_empty() {
                        self.page = Page::New;
                    }
                }
                Ok(Response::Quit) => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(error) => self.error = error,
            }
        }
        for tab in &mut self.tabs {
            tab.drain(&self.backend.terminal);
        }
    }

    fn select_page(&mut self, page: Page, ctx: &egui::Context) {
        if self.page != page {
            self.token.clear();
        }
        self.page = page;
        self.error.clear();
        ctx.request_repaint();
    }

    fn browse(&mut self, ctx: &egui::Context, target: BrowseTarget) {
        self.work(ctx, move |_| {
            let dialog = rfd::FileDialog::new();
            let selected = match target {
                BrowseTarget::Models | BrowseTarget::Config => {
                    dialog.add_filter("YAML", &["yml", "yaml"]).pick_file()
                }
                BrowseTarget::Conversation => {
                    dialog.add_filter("GJC session", &["jsonl"]).pick_file()
                }
                BrowseTarget::Executable => dialog
                    .add_filter("Windows executable", &["exe"])
                    .pick_file(),
                _ => dialog.pick_folder(),
            };
            Ok(Response::Path(target, selected))
        });
    }

    fn start(&mut self, ui: &egui::Ui, mode: Mode) {
        let runtime = self.preset.runtime_id.clone();
        let name = if self.name.trim().is_empty() {
            "Agent session".into()
        } else {
            self.name.trim().into()
        };
        let ctx = ui.ctx().clone();
        let wake = ctx.clone();
        self.work(&ctx, move |backend| {
            backend
                .launch(name, &runtime, mode, move || wake.request_repaint())
                .map(Box::new)
                .map(Response::Launched)
        });
    }

    fn runtime_picker(&mut self, ui: &mut egui::Ui) {
        let current = self
            .runtimes
            .iter()
            .find(|item| item.id == self.preset.runtime_id);
        egui::ComboBox::from_label("Runtime")
            .selected_text(
                current
                    .map(runtime_name)
                    .unwrap_or_else(|| "Select a runtime".into()),
            )
            .show_ui(ui, |ui| {
                for item in self.runtimes.iter().filter(|item| item.trusted) {
                    ui.selectable_value(
                        &mut self.preset.runtime_id,
                        item.id.clone(),
                        runtime_name(item),
                    );
                }
            });
    }

    fn new_session(&mut self, ui: &mut egui::Ui) {
        ui.heading("New session");
        text_input(ui, "Name", &mut self.name);
        self.runtime_picker(ui);
        if path_input(ui, "Working directory", &mut self.cwd) {
            self.browse(ui.ctx(), BrowseTarget::Working);
        }
        ui.checkbox(
            &mut self.template_launch,
            "Generate GJC YAML from a Router preset",
        );
        if self.template_launch {
            self.preset_controls(ui);
        }
        if ui
            .add_enabled(
                !self.preset.runtime_id.is_empty()
                    && !self.cwd.trim().is_empty()
                    && (!self.template_launch || !self.catalog.is_empty()),
                egui::Button::new("Start session"),
            )
            .clicked()
        {
            let cwd = self.cwd.trim().into();
            let mode = if self.template_launch {
                Mode::Preset {
                    cwd,
                    preset: Box::new(self.preset.clone()),
                    token: std::mem::take(&mut self.token),
                    catalog: self.catalog.clone(),
                }
            } else {
                Mode::Direct { cwd }
            };
            self.start(ui, mode);
        }
    }

    fn preset_controls(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_label("Saved preset")
            .selected_text(if self.preset.name.is_empty() {
                "Select a preset"
            } else {
                &self.preset.name
            })
            .show_ui(ui, |ui| {
                for (name, preset) in &self.presets {
                    if ui.selectable_label(false, name).clicked() {
                        self.preset = preset.clone();
                        self.catalog.clear();
                        self.token.clear();
                    }
                }
            });
        text_input(ui, "Preset name", &mut self.preset.name);
        let current = self
            .routers
            .iter()
            .find(|router| router.id == self.preset.router_id);
        egui::ComboBox::from_label("Router")
            .selected_text(
                current
                    .map(|router| router.name.as_str())
                    .unwrap_or("Select a Router"),
            )
            .show_ui(ui, |ui| {
                for profile in self.routers.iter().filter(|router| router.enabled) {
                    if ui
                        .selectable_value(
                            &mut self.preset.router_id,
                            profile.id.clone(),
                            &profile.name,
                        )
                        .changed()
                    {
                        self.catalog.clear();
                        self.token.clear();
                    }
                }
            });
        if ui
            .checkbox(&mut self.preset.anthropic, "Use Anthropic Messages")
            .changed()
        {
            self.catalog.clear();
        }
        if path_input(ui, "models.yml template", &mut self.preset.models_template) {
            self.browse(ui.ctx(), BrowseTarget::Models);
        }
        if path_input(ui, "config.yml template", &mut self.preset.config_template) {
            self.browse(ui.ctx(), BrowseTarget::Config);
        }
        text_input(ui, "Provider ID", &mut self.preset.provider);
        ui.horizontal(|ui| {
            ui.label("Fallback context tokens");
            ui.add(egui::DragValue::new(&mut self.preset.context).speed(256));
            ui.label("Fallback output tokens");
            ui.add(egui::DragValue::new(&mut self.preset.max_output).speed(128));
        });
        ui.small("Enter verified limits for models without token limits in the template.");
        if self
            .routers
            .iter()
            .any(|router| router.id == self.preset.router_id && router.authentication == "bearer")
        {
            ui.add(
                egui::TextEdit::singleline(&mut self.token)
                    .password(true)
                    .hint_text("Temporary Router credential"),
            );
        }
        if ui.button("Refresh Router models").clicked() {
            let profile = self
                .routers
                .iter()
                .find(|router| router.id == self.preset.router_id)
                .cloned();
            let protocol = native_gjc::protocol(&self.preset);
            let token = self.token.clone();
            self.catalog.clear();
            self.work(ui.ctx(), move |_| {
                let profile = profile.ok_or("Select a Router profile")?;
                let catalog = router::list_models(&profile, &token, protocol)?;
                if catalog.complete {
                    Ok(Response::Catalog(catalog.models))
                } else {
                    Err(catalog.message.into())
                }
            });
        }
        if !self.catalog.is_empty() {
            egui::ComboBox::from_label("Default model")
                .selected_text(&self.preset.default_model)
                .show_ui(ui, |ui| {
                    for model in &self.catalog {
                        ui.selectable_value(&mut self.preset.default_model, model.clone(), model);
                    }
                });
            ui.collapsing(format!("Served models ({})", self.catalog.len()), |ui| {
                ui.small("An empty selection includes all served models.");
                ui.horizontal(|ui| {
                    if ui.button("Include all").clicked() {
                        self.preset.models.clear();
                    }
                    self.preset.models.retain(|model| {
                        self.catalog.contains(model)
                            || !ui.small_button(format!("Remove missing {model}")).clicked()
                    });
                });
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for model in &self.catalog {
                            let mut selected = self.preset.models.contains(model);
                            if ui.checkbox(&mut selected, model).changed() {
                                if selected {
                                    self.preset.models.push(model.clone());
                                } else {
                                    self.preset.models.retain(|id| id != model);
                                }
                            }
                        }
                    });
            });
        }
        egui::ComboBox::from_label("Conversation storage")
            .selected_text(history_label(self.preset.location))
            .show_ui(ui, |ui| {
                for mode in [
                    HistoryLocation::AppData,
                    HistoryLocation::Custom,
                    HistoryLocation::Workspace,
                ] {
                    ui.selectable_value(&mut self.preset.location, mode, history_label(mode));
                }
            });
        if self.preset.location == HistoryLocation::Custom
            && path_input(ui, "Existing history root", &mut self.preset.history_root)
        {
            self.browse(ui.ctx(), BrowseTarget::HistoryRoot);
        }
        if self.preset.location == HistoryLocation::Workspace {
            ui.small("Uses .gjc/deck-sessions under the working directory. Check Git exclusions before storing conversations there.");
        }
        if ui
            .add_enabled(
                !self.preset.name.trim().is_empty(),
                egui::Button::new("Save preset"),
            )
            .clicked()
        {
            self.preset.name = self.preset.name.trim().into();
            self.presets
                .insert(self.preset.name.clone(), self.preset.clone());
            if let Err(error) = self
                .backend
                .store
                .put(Table::Settings, PRESETS, &self.presets)
            {
                self.error = error;
            }
        }
    }

    fn history_page(&mut self, ui: &mut egui::Ui) {
        ui.heading("GJC history");
        ui.small(
            "Close the conversation in its previous terminal. GJC continues in the original file.",
        );
        self.runtime_picker(ui);
        if path_input(
            ui,
            "Original agent directory (blank for default)",
            &mut self.history_agent,
        ) {
            self.browse(ui.ctx(), BrowseTarget::AgentDirectory);
        }
        if path_input(ui, "GJC .jsonl path", &mut self.history_file) {
            self.browse(ui.ctx(), BrowseTarget::Conversation);
        }
        ui.horizontal(|ui| {
            let agent = self.history_agent.trim().to_owned();
            if ui.button("Find recent sessions").clicked() {
                let agent = agent.clone();
                self.work(ui.ctx(), move |_| {
                    gjc_history::list(gjc_history::agent_directory(Some(&agent))?)
                        .map(Response::History)
                });
            }
            if ui.button("Inspect file").clicked() {
                let path = PathBuf::from(self.history_file.trim());
                self.work(ui.ctx(), move |_| {
                    gjc_history::inspect(&path, gjc_history::agent_directory(Some(&agent))?)
                        .map(Response::HistoryFile)
                });
            }
        });
        if self.history_limited {
            ui.small("Scan limit reached. Open an unlisted record by its full path.");
        }
        for (index, entry) in self.history.iter().enumerate() {
            let title = if entry.title.is_empty() {
                &entry.source.session_id
            } else {
                &entry.title
            };
            if ui
                .selectable_label(
                    self.selected_history == Some(index),
                    format!("{title} · {}", entry.cwd.display()),
                )
                .clicked()
            {
                self.selected_history = Some(index);
            }
        }
        text_input(ui, "Replacement model (optional)", &mut self.replacement);
        ui.add(
            egui::TextEdit::singleline(&mut self.token)
                .password(true)
                .hint_text("Router credential for Deck-generated YAML"),
        );
        if let Some(entry) = self
            .selected_history
            .and_then(|index| self.history.get(index))
        {
            ui.small(entry.source.path.display().to_string());
            if ui.button("Resume original conversation").clicked() {
                self.name = if entry.title.is_empty() {
                    format!(
                        "GJC · {}",
                        entry.source.session_id.chars().take(8).collect::<String>()
                    )
                } else {
                    entry.title.clone()
                };
                let token = std::mem::take(&mut self.token);
                self.start(
                    ui,
                    Mode::Resume {
                        source: entry.source.clone(),
                        model: self.replacement.trim().into(),
                        token,
                    },
                );
            }
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.collapsing("Agent runtimes", |ui| {
            for runtime in &self.runtimes {
                ui.label(runtime_name(runtime));
            }
            ui.label("Enter the full path to a native executable.");
            if ui.text_edit_singleline(&mut self.runtime_path).changed() {
                self.candidate = None;
            }
            if ui.button("Browse executable").clicked() {
                self.browse(ui.ctx(), BrowseTarget::Executable);
            }
            egui::ComboBox::from_label("Agent")
                .selected_text(format!("{:?}", self.register_agent))
                .show_ui(ui, |ui| {
                    for agent in [Agent::Gajae, Agent::Claude, Agent::Hermes, Agent::Generic] {
                        ui.selectable_value(&mut self.register_agent, agent, format!("{agent:?}"));
                    }
                });
            if ui.button("Inspect executable").clicked() {
                let path = self.runtime_path.trim().to_owned();
                self.work(ui.ctx(), move |_| {
                    registry::inspect(&path).map(Response::Inspected)
                });
            }
            if let Some(candidate) = self.candidate.clone() {
                ui.monospace(format!("{}\n{}", candidate.path, candidate.sha256));
                if ui.button("Trust, verify version and register").clicked() {
                    let agent = self.register_agent;
                    self.work(ui.ctx(), move |backend| {
                        backend
                            .register(&candidate.path, &candidate.sha256, agent)
                            .map(|_| Response::Runtime)
                    });
                }
            }
        });
        ui.collapsing("Local LLM Router", |ui| {
            for profile in &self.routers {
                if ui.button(&profile.name).clicked() {
                    self.router_draft = profile.clone();
                    self.port_text = profile.port.to_string();
                    self.openai = profile.openai_path.is_some();
                    self.anthropic = profile.anthropic_path.is_some();
                }
            }
            if ui.button("New Router profile").clicked() {
                self.router_draft = new_router();
                self.port_text.clear();
                self.openai = true;
                self.anthropic = false;
            }
            text_input(ui, "Profile name", &mut self.router_draft.name);
            text_input(ui, "Scheme", &mut self.router_draft.scheme);
            text_input(ui, "Host", &mut self.router_draft.host);
            text_input(ui, "Port", &mut self.port_text);
            ui.checkbox(
                &mut self.router_draft.allow_remote,
                "Allow this non-loopback host",
            );
            ui.checkbox(&mut self.router_draft.enabled, "Enabled");
            ui.checkbox(&mut self.openai, "OpenAI Chat Completions");
            if self.openai {
                text_input(
                    ui,
                    "OpenAI base path",
                    self.router_draft.openai_path.get_or_insert("/v1".into()),
                );
            }
            ui.checkbox(&mut self.anthropic, "Anthropic Messages");
            if self.anthropic {
                text_input(
                    ui,
                    "Anthropic base path",
                    self.router_draft
                        .anthropic_path
                        .get_or_insert_with(String::new),
                );
            }
            let mut bearer = self.router_draft.authentication == "bearer";
            if ui.checkbox(&mut bearer, "Bearer authentication").changed() {
                self.router_draft.authentication = if bearer { "bearer" } else { "none" }.into();
            }
            if ui.button("Save Router profile").clicked() {
                let result = (|| -> Result<()> {
                    let mut profile = self.router_draft.clone();
                    profile.port = self
                        .port_text
                        .parse()
                        .map_err(|_| "Enter a port from 1 to 65535")?;
                    profile.openai_path = profile.openai_path.filter(|_| self.openai);
                    profile.anthropic_path = profile.anthropic_path.filter(|_| self.anthropic);
                    profile.updated_at = model::now();
                    router::validate(&profile)?;
                    self.backend
                        .store
                        .put(Table::Routers, &profile.id, &profile)
                })();
                match result {
                    Ok(()) => {
                        self.catalog.clear();
                        self.refresh();
                    }
                    Err(error) => self.error = error,
                }
            }
        });
        ui.label(format!("App data · {}", self.backend.data.display()));
        if let Some(path) = diagnostics::path() {
            ui.label(format!("Log · {path}"));
        }
    }
}

impl eframe::App for Deck {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll(ctx);
        if ctx.input(|input| input.viewport().close_requested())
            && !self.allow_close
            && (self.busy || !self.tabs.is_empty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_requested = true;
        }
        egui::TopBottomPanel::top("deck-toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Agent Deck");
                ui.separator();
                for (title, page) in [
                    ("New session", Page::New),
                    ("GJC history", Page::History),
                    ("Settings", Page::Settings),
                    ("?", Page::Help),
                ] {
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new(title).selected(self.page == page),
                        )
                        .clicked()
                    {
                        self.select_page(page, ctx);
                    }
                }
                if self.busy {
                    ui.spinner();
                }
                if ui
                    .button(if self.sidebar { "<" } else { ">" })
                    .on_hover_text("Toggle sessions")
                    .clicked()
                {
                    self.sidebar = !self.sidebar;
                }
            });
            if !self.tabs.is_empty() {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for tab in &self.tabs {
                            if ui
                                .selectable_label(
                                    self.page == Page::Terminal && self.active == tab.session.id,
                                    &tab.session.name,
                                )
                                .clicked()
                            {
                                self.active.clone_from(&tab.session.id);
                                self.page = Page::Terminal;
                                self.focus = true;
                            }
                            if ui
                                .small_button("×")
                                .on_hover_text("Close terminal")
                                .clicked()
                            {
                                self.closing_tab = Some(tab.session.id.clone());
                            }
                        }
                    });
                });
            }
        });
        egui::TopBottomPanel::bottom("deck-status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Agent Deck v{} · Native",
                    env!("CARGO_PKG_VERSION")
                ));
                if let Some(tab) = self.tabs.iter().find(|tab| tab.session.id == self.active) {
                    ui.separator();
                    ui.label(&tab.session.version);
                    ui.label(&tab.status);
                    ui.label(&tab.session.cwd);
                }
            });
        });
        let mut history_lookup = None;
        if self.sidebar {
            egui::SidePanel::left("deck-sessions")
                .resizable(true)
                .default_width(235.0)
                .min_width(190.0)
                .show(ctx, |ui| {
                    ui.heading("Sessions");
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for session in &self.sessions {
                            let running = self.tabs.iter().find(|tab| {
                                tab.session.id == session.id
                                    || tab.session.resume_source_id.as_deref() == Some(&session.id)
                            });
                            let status = running.map(|tab| tab.status.as_str()).unwrap_or("Saved");
                            if ui
                                .selectable_label(
                                    running.is_some_and(|tab| tab.session.id == self.active),
                                    format!("{}\n{} · {status}", session.name, session.version),
                                )
                                .clicked()
                            {
                                if let Some(tab) = running {
                                    self.active.clone_from(&tab.session.id);
                                    self.page = Page::Terminal;
                                    self.focus = true;
                                } else if let Some(source) = &session.external_history {
                                    self.preset.runtime_id.clone_from(&session.runtime_id);
                                    self.history_agent =
                                        source.agent_directory.display().to_string();
                                    self.history_file = source.path.display().to_string();
                                    let source = source.clone();
                                    self.page = Page::History;
                                    self.history.clear();
                                    self.selected_history = None;
                                    history_lookup =
                                        Some((source.path, source.agent_directory, true));
                                } else if let Some(paths) = &session.native_gjc {
                                    self.preset.runtime_id.clone_from(&session.runtime_id);
                                    self.history_agent =
                                        paths.agent_directory.display().to_string();
                                    self.history_file.clear();
                                    self.page = Page::History;
                                    self.history.clear();
                                    self.selected_history = None;
                                    history_lookup = Some((
                                        paths.session_directory.clone(),
                                        paths.agent_directory.clone(),
                                        false,
                                    ));
                                } else if let Some(history) = session
                                    .gjc
                                    .as_ref()
                                    .filter(|history| !history.history_disabled)
                                {
                                    self.preset.runtime_id.clone_from(&session.runtime_id);
                                    self.history_agent = history
                                        .agent_directory
                                        .as_ref()
                                        .map(|path| path.display().to_string())
                                        .unwrap_or_default();
                                    self.history_file.clear();
                                    self.page = Page::History;
                                    self.history.clear();
                                    self.selected_history = None;
                                    if let Some(agent) = &history.agent_directory {
                                        history_lookup = Some((
                                            history
                                                .session_directory
                                                .clone()
                                                .unwrap_or_else(|| agent.join("sessions")),
                                            agent.clone(),
                                            false,
                                        ));
                                    }
                                } else {
                                    self.cwd.clone_from(&session.cwd);
                                    self.preset.runtime_id.clone_from(&session.runtime_id);
                                    self.name.clone_from(&session.name);
                                    self.template_launch = false;
                                    self.page = Page::New;
                                }
                            }
                        }
                    });
                });
        }
        if let Some((path, agent, file)) = history_lookup {
            self.work(ctx, move |_| {
                if file {
                    gjc_history::inspect(&path, agent).map(Response::HistoryFile)
                } else {
                    gjc_history::list_in(path, agent).map(Response::History)
                }
            });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.error.is_empty() { ui.colored_label(Color32::LIGHT_RED, &self.error); }
            if self.page == Page::Terminal {
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.session.id == self.active) {
                    tab.ui(ui, &self.backend.terminal, std::mem::take(&mut self.focus));
                }
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_max_width(760.0);
                    ui.add_enabled_ui(!self.busy, |ui| match self.page {
                        Page::New => self.new_session(ui),
                        Page::History => self.history_page(ui),
                        Page::Settings => self.settings(ui),
                        Page::Help => {
                            ui.heading("Terminal controls");
                            ui.label("Alt+↑ passes through to GJC. Drag to select text, then press Ctrl+Shift+C to copy. Ctrl+C interrupts. Ctrl+V or Ctrl+Shift+V pastes.");
                        }
                        Page::Terminal => {}
                    });
                });
            }
        });
        if self.close_requested || self.closing_tab.is_some() {
            let quit = self.close_requested;
            egui::Window::new(if quit {
                "Close Agent Deck"
            } else {
                "Close terminal"
            })
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(if quit {
                    "Stop the managed terminal processes and exit?"
                } else {
                    "Stop this terminal process and close the tab?"
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Stop and close"))
                        .clicked()
                    {
                        if quit {
                            self.work(ctx, |backend| {
                                backend.terminal.shutdown().map(|_| Response::Quit)
                            });
                        } else if let Some(id) = self.closing_tab.clone() {
                            self.work(ctx, move |backend| {
                                backend.terminal.close(&id).map(|_| Response::Closed(id))
                            });
                        }
                    }
                    if !self.busy && ui.button("Cancel").clicked() {
                        self.close_requested = false;
                        self.closing_tab = None;
                    }
                });
            });
        }
    }
}

fn text_input(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}

fn path_input(ui: &mut egui::Ui, label: &str, value: &mut String) -> bool {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width((ui.available_width() - 93.0).max(80.0)),
        );
        ui.button("Browse…").clicked()
    })
    .inner
}

fn runtime_name(runtime: &Runtime) -> String {
    format!("{} · {}", runtime.name, runtime.version)
}

fn history_label(location: HistoryLocation) -> &'static str {
    match location {
        HistoryLocation::AppData => "Deck app data",
        HistoryLocation::Custom => "Custom root per session",
        HistoryLocation::Workspace => "Working directory per session",
    }
}

fn new_router() -> Router {
    let now = model::now();
    Router {
        id: model::id(),
        name: String::new(),
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: 0,
        openai_path: Some("/v1".into()),
        anthropic_path: None,
        model: String::new(),
        authentication: "none".into(),
        enabled: true,
        allow_remote: false,
        created_at: now,
        updated_at: now,
    }
}
