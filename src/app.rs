use crate::{
    command::{self, Evidence},
    connections::{self, Mac},
    report,
    storage::{self, Preferences},
    system::{self, Snapshot, State},
};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

const INK: Color32 = Color32::from_rgb(26, 42, 53);
const MUTED: Color32 = Color32::from_rgb(107, 122, 132);
const TEAL: Color32 = Color32::from_rgb(0, 119, 109);
const BORDER: Color32 = Color32::from_rgb(222, 230, 230);
const BG: Color32 = Color32::from_rgb(246, 248, 248);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Host,
    Connect,
    Checks,
    Guide,
}
enum ResultData {
    Scan(Box<Snapshot>),
    Actions(Vec<Evidence>),
    Power(Preferences, Box<Snapshot>, Evidence),
    Error(String),
}

pub struct MacLink {
    page: Page,
    snapshot: Option<Snapshot>,
    prefs: Preferences,
    save_blocked: bool,
    job: Option<Receiver<ResultData>>,
    busy: String,
    notice: String,
    notice_error: bool,
    draft: Mac,
    filter: String,
    report_text: String,
}

impl MacLink {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        // Use the system font when available; retain bundled fallbacks for symbols.
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "mac-system".into(),
                egui::FontData::from_owned(bytes).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "mac-system".into());
            ctx.set_fonts(fonts);
        }
        ctx.set_visuals(egui::Visuals::light());
        let mut style = (*ctx.style()).clone();
        style.visuals.panel_fill = BG;
        style.visuals.override_text_color = Some(INK);
        style.visuals.widgets.inactive.bg_fill = Color32::WHITE;
        style.visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(229, 241, 239);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(229, 241, 239);
        style.visuals.selection.bg_fill = Color32::from_rgb(210, 234, 229);
        style.visuals.selection.stroke = Stroke::new(1.0_f32, TEAL);
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.spacing.button_padding = Vec2::new(13.0, 9.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(25.0));
        ctx.set_style(style);
        let loaded = storage::load();
        let (prefs, notice, save_blocked) = match loaded {
            Ok(p) => (
                p,
                "Discovery runs locally. No settings change during a scan.".into(),
                false,
            ),
            Err(e) => (Preferences::default(), e, true),
        };
        let mut app = Self {
            page: Page::Host,
            snapshot: None,
            prefs,
            save_blocked,
            job: None,
            busy: String::new(),
            notice,
            notice_error: save_blocked,
            draft: Mac::default(),
            filter: String::new(),
            report_text: String::new(),
        };
        app.scan(ctx);
        app
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        work: impl FnOnce() -> ResultData + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.job = Some(rx);
        self.busy = label.into();
        thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }
    fn scan(&mut self, ctx: &egui::Context) {
        self.start(ctx, "Inspecting this Mac…", || {
            ResultData::Scan(Box::new(Snapshot::discover()))
        });
    }
    fn say(&mut self, message: impl Into<String>, error: bool) {
        self.notice = message.into();
        self.notice_error = error;
    }
    fn persist(&mut self) {
        if self.save_blocked {
            self.say("Saved settings are unreadable; original file preserved. Resolve the file error before saving changes.",true);
            return;
        }
        if let Err(e) = storage::save(&self.prefs) {
            self.say(e, true);
        }
    }
    fn poll(&mut self) {
        let result = self.job.as_ref().map(|r| r.try_recv());
        match result {
            Some(Ok(r)) => {
                self.job = None;
                match r {
                    ResultData::Scan(s) => {
                        self.snapshot = Some(*s);
                        self.say(
                            "Discovery complete. Review pending setup and client tests below.",
                            false,
                        );
                    }
                    ResultData::Actions(actions) => {
                        let failed = actions.iter().any(|e| !e.success);
                        self.say(
                            actions
                                .iter()
                                .map(|e| format!("{}: {}", e.command, e.output))
                                .collect::<Vec<_>>()
                                .join("\n"),
                            failed,
                        );
                        self.prefs.actions.extend(actions);
                        self.persist();
                    }
                    ResultData::Power(p, s, e) => {
                        self.prefs = p;
                        self.snapshot = Some(*s);
                        self.say(e.output, !e.success);
                    }
                    ResultData::Error(e) => self.say(e, true),
                }
                self.update_report();
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.job = None;
                self.say(
                    "The operation ended unexpectedly. Refresh to verify the current state.",
                    true,
                );
            }
            _ => {}
        }
    }
    fn update_report(&mut self) {
        if let Some(s) = &self.snapshot {
            self.report_text = report::full(s, &self.prefs);
        }
    }
    fn open_url(&mut self, ctx: &egui::Context, url: &'static str) {
        self.start(ctx,"Opening macOS…",move|| {
            let mut e=command::run("/usr/bin/open",&[url],8);
            if e.success {e.output="Opened. Complete the requested step there, then refresh Mac Link. Configuration remains unverified.".into();}
            ResultData::Actions(vec![e])
        });
    }
    fn sharing(&mut self, ctx: &egui::Context) {
        self.open_url(
            ctx,
            "x-apple.systempreferences:com.apple.Sharing-Settings.extension",
        );
    }
    fn connect(&mut self, ctx: &egui::Context, mac: Mac, ssh: bool) {
        if let Err(e) = connections::host(&mac.host) {
            self.say(e, true);
            return;
        }
        if ssh && let Err(e) = connections::username(&mac.user) {
            self.say(e, true);
            return;
        }
        self.start(ctx,"Opening the native client…",move|| {
            match if ssh {connections::open_ssh(&mac)} else {connections::open_screen(&mac)} {
                Ok(mut e)=>{if e.success {e.output="Native client opened. Sign in there; authentication has NOT been verified by Mac Link.".into();}ResultData::Actions(vec![e])},
                Err(e)=>ResultData::Error(e),
            }
        });
    }
    fn power(&mut self, ctx: &egui::Context, restore: bool) {
        if self.save_blocked {
            self.say(
                "Resolve the saved settings error before changing power settings.",
                true,
            );
            return;
        }
        let mut prefs = self.prefs.clone();
        self.start(ctx,if restore {"Restoring AC settings…"} else {"Waiting for macOS administrator authorization…"},move|| {
            let current=Snapshot::discover();
            let backup=if restore {
                match &prefs.power_backup {
                    Some(b)=>b.clone(),None=>return ResultData::Error("No recorded power change to restore.".into()),
                }
            } else {
                match (&prefs.power_backup,system::power_plan(&current)) {
                    (_,Err(e))=>return ResultData::Error(e),
                    (Some(b),Ok(_))=>b.clone(),
                    (None,Ok(b))=>b,
                }
            };
            if backup.applied.is_empty() {return ResultData::Error("This Mac already has the requested AC settings.".into());}
            if backup.applied.iter().any(|(k,v)|current.power.get(k)!=Some(v) && current.power.get(k)!=backup.previous.get(k)) {
                return ResultData::Error("Power settings have changed outside Mac Link. Review the recorded previous values in the report before restoring manually.".into());
            }
            if !restore {
                prefs.power_backup=Some(backup.clone());
                if let Err(e)=storage::save(&prefs) {return ResultData::Error(format!("Could not save rollback values; no changes made: {e}"));}
            }
            let e=match system::apply_power(if restore {&backup.previous} else {&backup.applied}) {Ok(e)=>e,Err(e)=>return ResultData::Error(e)};
            if restore && e.success {prefs.power_backup=None;}
            prefs.actions.push(e.clone());
            if let Err(err)=storage::save(&prefs) {return ResultData::Error(format!("Power operation ran, but saving its result failed: {err}. Inspect pmset and the saved rollback record."));}
            ResultData::Power(prefs,Box::new(Snapshot::discover()),e)
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(208.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(235, 241, 240))
                    .inner_margin(20),
            )
            .show(ctx, |ui| {
                ui.add_space(15.0);
                ui.horizontal(|ui| {
                    logo(ui, 34.0);
                    ui.label(RichText::new("Mac Link").size(23.0).strong());
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Your Macs. Within reach.")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.add_space(35.0);
                eyebrow(ui, "WORKSPACE");
                ui.add_space(8.0);
                for (page, label) in [
                    (Page::Host, "Set up this Mac"),
                    (Page::Connect, "Connect to a Mac"),
                    (Page::Checks, "Verification"),
                    (Page::Guide, "Connection guide"),
                ] {
                    let selected = self.page == page;
                    let button = egui::Button::new(
                        RichText::new(label)
                            .color(if selected { TEAL } else { INK })
                            .strong(),
                    )
                    .fill(if selected {
                        Color32::from_rgb(213, 232, 227)
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE)
                    .min_size(Vec2::new(168.0, 42.0));
                    if ui.add(button).clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("MAC LINK  /  0.1.0").size(10.0).color(MUTED));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Apple Screen Sharing\n+ your private Tailscale network")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.add_space(16.0);
                    ui.separator();
                    if let Some(s) = &self.snapshot {
                        ui.label(RichText::new(&s.name).size(13.0).strong());
                        ui.horizontal(|ui| {
                            dot(ui, s.tailnet.state == "Running");
                            ui.label(
                                RichText::new(if s.tailnet.state == "Running" {
                                    "Tailscale connected"
                                } else {
                                    "Tailscale needs attention"
                                })
                                .size(12.0),
                            );
                        });
                    }
                });
            });
    }
    fn header(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        kicker: &str,
        title: &str,
        subtitle: &str,
    ) {
        eyebrow(ui, kicker);
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(31.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("Refresh discovery"))
                    .clicked()
                {
                    self.scan(ctx);
                }
            });
        });
        ui.label(RichText::new(subtitle).color(MUTED));
        ui.add_space(18.0);
    }
    fn host_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.header(
            ui,
            ctx,
            "HOST WORKSPACE",
            "Make this Mac reachable.",
            "Set up once here. Connect from your other Mac, wherever you are.",
        );
        let Some(s) = self.snapshot.clone() else {
            ui.spinner();
            ui.label("Reading hardware, network, sharing, and security settings…");
            return;
        };
        let enabled = self.job.is_none();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                computer_art(ui);
                ui.vertical(|ui| {
                    eyebrow(ui, "THIS MAC");
                    ui.label(RichText::new(&s.name).size(23.0).strong());
                    ui.label(
                        RichText::new(format!("{}  ·  macOS {}  ·  {}", s.chip, s.os, s.model))
                            .color(MUTED),
                    );
                    ui.add_space(5.0);
                    ui.label(format!("Local account   {}", s.user));
                });
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(5.0);
            ui.columns(3, |cols| {
                mini_stat(
                    &mut cols[0],
                    "LOCAL NETWORK",
                    s.lan
                        .first()
                        .map(|(_, a)| a.as_str())
                        .unwrap_or("No LAN address"),
                    "Connect on the same network",
                );
                mini_stat(
                    &mut cols[1],
                    "TAILSCALE",
                    if s.tailnet.ip.is_empty() {
                        "Not connected"
                    } else {
                        &s.tailnet.ip
                    },
                    "Connect through your tailnet",
                );
                mini_stat(
                    &mut cols[2],
                    "AC SYSTEM SLEEP",
                    if s.power.get("sleep") == Some(&0) {
                        "Never"
                    } else {
                        "Needs setup"
                    },
                    "Display sleep stays independent",
                );
            });
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Your setup checklist").size(20.0).strong());
            ui.label(
                RichText::new("Changes stay in your control")
                    .color(MUTED)
                    .size(12.0),
            );
        });
        card(ui, |ui| {
            step(
                ui,
                "01",
                "Enable native Screen Sharing",
                "Choose the account allowed to control this Mac.",
                |ui| {
                    if ui
                        .add_enabled(enabled, primary("Open Sharing settings"))
                        .clicked()
                    {
                        self.sharing(ctx);
                    }
                },
            );
            egui::CollapsingHeader::new("Screen Sharing instructions").show(ui, |ui| {
                ui.label(report::SCREEN_STEPS);
            });
            ui.separator();
            step(
                ui,
                "02",
                "Connect your private network",
                if s.tailnet.state == "Running" {
                    "Existing Tailscale connection detected. Identity preserved."
                } else {
                    "Sign in to the official Tailscale app with your existing account."
                },
                |ui| {
                    if ui
                        .add_enabled(enabled, egui::Button::new("Open Tailscale"))
                        .clicked()
                    {
                        self.start(ctx, "Opening Tailscale…", || {
                            let e = command::run("/usr/bin/open", &["-a", "Tailscale"], 8);
                            ResultData::Actions(vec![e])
                        });
                    }
                },
            );
            egui::CollapsingHeader::new("Tailscale installation & access").show(ui,|ui| {
                ui.label(format!("Tailnet: {}\nMagicDNS: {}",s.tailnet.identity,if s.tailnet.magic_dns{&s.tailnet.dns}else{"Not confirmed enabled"}));
                ui.label("If the app is absent, install the official macOS app and sign in. Keep the existing variant if already installed. Restrict access with tailnet policy; no exit node, subnet route, public port forwarding, or Funnel is needed.");
                ui.hyperlink_to("Official Tailscale download","https://tailscale.com/download/mac");
            });
            ui.separator();
            step(
                ui,
                "03",
                "Enable secure terminal access",
                "Use macOS Remote Login with only your designated users.",
                |ui| {
                    if ui
                        .add_enabled(enabled, egui::Button::new("Open Remote Login"))
                        .clicked()
                    {
                        self.sharing(ctx);
                    }
                },
            );
            egui::CollapsingHeader::new("SSH instructions").show(ui, |ui| {
                ui.label(report::SSH_STEPS);
            });
            ui.separator();
            step(
                ui,
                "04",
                "Keep this desktop available",
                "Prevent AC system sleep while allowing the display to sleep.",
                |ui| {
                    if self.prefs.power_backup.is_some() {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Restore AC settings"))
                            .clicked()
                        {
                            self.power(ctx, true);
                        }
                    } else if ui
                        .add_enabled(
                            enabled
                                && (s.power.get("sleep") != Some(&0)
                                    || (s.power_caps.iter().any(|c| c == "womp")
                                        && s.power.get("womp") != Some(&1))),
                            egui::Button::new("Keep awake on AC…"),
                        )
                        .clicked()
                    {
                        self.power(ctx, false);
                    }
                },
            );
            ui.label(RichText::new("Uses macOS administrator authorization. Records previous values; changes only supported AC sleep / network wake. Higher power use while idle.").size(12.0).color(MUTED));
            if self
                .prefs
                .power_backup
                .as_ref()
                .is_some_and(|b| b.applied.iter().any(|(k, v)| s.power.get(k) != Some(v)))
                && ui
                    .add_enabled(
                        enabled,
                        egui::Button::new("Retry keep-awake authorization…"),
                    )
                    .clicked()
            {
                self.power(ctx, false);
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Ready for the other Mac?")
                            .size(18.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Copy the connection guide, then test a real login.")
                            .color(MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(primary("Copy connection guide")).clicked() {
                        ctx.copy_text(report::guide(&s));
                        self.say(
                            "Connection guide copied. Paste it on your client Mac.",
                            false,
                        );
                    }
                });
            });
        });
    }

    fn connect_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.header(
            ui,
            ctx,
            "CLIENT WORKSPACE",
            "Your next Mac is a click away.",
            "Use this view on the Mac you want to connect from.",
        );
        let enabled = self.job.is_none();
        card(ui, |ui| {
            ui.label(RichText::new("Quick connect").size(20.0).strong());
            ui.label(
                RichText::new("Enter the host’s LAN IP, Tailscale IP, or MagicDNS name.")
                    .color(MUTED),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.draft.host)
                    .hint_text("192.168.1.25 or mac-mini.tailnet.ts.net")
                    .desired_width(f32::INFINITY)
                    .margin(Vec2::new(12.0, 12.0)),
            );
            ui.columns(2, |cols| {
                cols[0].label("Mac name (optional)");
                cols[0].add(
                    egui::TextEdit::singleline(&mut self.draft.name)
                        .hint_text("Studio Mac mini")
                        .desired_width(f32::INFINITY),
                );
                cols[1].label("Remote username (for SSH)");
                cols[1].add(
                    egui::TextEdit::singleline(&mut self.draft.user)
                        .hint_text("alex")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(enabled, primary("Open Screen Sharing"))
                    .clicked()
                {
                    self.connect(ctx, self.draft.clone(), false);
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Open SSH"))
                    .clicked()
                {
                    self.connect(ctx, self.draft.clone(), true);
                }
                if ui
                    .add_enabled(enabled, egui::Button::new("Test ports"))
                    .clicked()
                {
                    match connections::host(&self.draft.host) {
                        Ok(h) => self.start(ctx, "Checking TCP 5900 and 22…", move || {
                            ResultData::Actions(connections::test_ports(&h))
                        }),
                        Err(e) => self.say(e, true),
                    }
                }
                if ui
                    .add_enabled(enabled && !self.save_blocked, egui::Button::new("Save Mac"))
                    .clicked()
                {
                    match connections::host(&self.draft.host) {
                        Ok(h) => {
                            let mut mac = self.draft.clone();
                            mac.host = h;
                            if mac.name.trim().is_empty() {
                                mac.name = mac.host.clone();
                            }
                            if let Some(existing) =
                                self.prefs.macs.iter_mut().find(|m| m.host == mac.host)
                            {
                                *existing = mac;
                            } else {
                                self.prefs.macs.push(mac);
                            }
                            self.say("Mac saved on this device. No password is stored.", false);
                            self.persist();
                        }
                        Err(e) => self.say(e, true),
                    }
                }
            });
            ui.label(RichText::new("Your macOS account signs in inside Apple’s client. A port test does not check credentials.").color(MUTED).size(12.0));
        });
        ui.add_space(12.0);
        ui.label(RichText::new("Saved Macs").size(20.0).strong());
        if self.prefs.macs.is_empty() {
            card(ui, |ui| {
                ui.label("A familiar Mac, one click away.");
                ui.label(RichText::new("Save an address above to keep it here.").color(MUTED));
            });
        }
        for (i, mac) in self.prefs.macs.clone().into_iter().enumerate() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&mac.name).strong());
                        ui.label(RichText::new(&mac.host).color(MUTED));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(enabled && !self.save_blocked, egui::Button::new("Remove"))
                            .clicked()
                        {
                            self.prefs.macs.remove(i);
                            self.persist();
                        }
                        if ui.button("Edit").clicked() {
                            self.draft = mac.clone();
                        }
                        if ui.add_enabled(enabled, primary("Connect")).clicked() {
                            self.connect(ctx, mac.clone(), false);
                        }
                    });
                });
            });
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Macs on your tailnet").size(20.0).strong());
            ui.label(
                RichText::new("Presence reported by Tailscale")
                    .color(MUTED)
                    .size(12.0),
            );
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Find a Mac…")
                .desired_width(f32::INFINITY),
        );
        let peers = self
            .snapshot
            .as_ref()
            .map(|s| s.tailnet.peers.clone())
            .unwrap_or_default();
        if peers.is_empty() {
            ui.label("No Mac peers found. Check that both Macs are signed into the same tailnet.");
        }
        for peer in peers
            .into_iter()
            .filter(|p| p.name.to_lowercase().contains(&self.filter.to_lowercase()))
            .collect::<Vec<_>>()
        {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    dot(ui, peer.online);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&peer.name).strong());
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                peer.ip,
                                if peer.online {
                                    "Online; sharing not tested"
                                } else {
                                    "Offline or presence unavailable"
                                }
                            ))
                            .color(MUTED)
                            .size(12.0),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Use address").clicked() {
                            self.draft = Mac {
                                name: peer.name.clone(),
                                host: peer.host.clone(),
                                user: String::new(),
                            };
                            self.say(
                                "Address selected above. Add the remote account name for SSH.",
                                false,
                            );
                        }
                        if ui
                            .add_enabled(enabled, egui::Button::new("Ping path"))
                            .clicked()
                        {
                            if let Some(binary) = self
                                .snapshot
                                .as_ref()
                                .and_then(|s| s.tailnet.binary.clone())
                            {
                                let h = peer.host.clone();
                                self.start(ctx, "Testing the Tailscale peer path…", move || {
                                    if let Err(e) = connections::host(&h) {
                                        return ResultData::Error(e);
                                    }
                                    ResultData::Actions(vec![command::run(
                                        &binary,
                                        &["ping", "--c=3", "--timeout=3s", &h],
                                        12,
                                    )])
                                });
                            } else {
                                self.say("Tailscale CLI unavailable.", true);
                            }
                        }
                    });
                });
            });
        }
    }

    fn checks_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.header(ui,ctx,"EVIDENCE, NOT ASSUMPTIONS","Know what actually works.","Local configuration, network reachability, and a successful sign-in are separate checks.");
        let Some(s) = self.snapshot.clone() else {
            ui.spinner();
            return;
        };
        card(ui, |ui| {
            ui.label(RichText::new("Test the next layer").size(19.0).strong());
            ui.label("Run a local SSH key test without prompting for a password or accepting an unknown host key. For desktop access, use the connection guide from another Mac.");
            ui.horizontal(|ui|{
                if ui.add_enabled(self.job.is_none(),egui::Button::new("Test localhost SSH key")).clicked(){
                    let user=s.user.clone();
                    self.start(ctx,"Testing local SSH authentication…",move||{
                        if let Err(e)=connections::username(&user){return ResultData::Error(e);}
                        let e=command::run("/usr/bin/ssh",&["-o","BatchMode=yes","-o","StrictHostKeyChecking=yes","-o","ConnectTimeout=4","-o","ConnectionAttempts=1","-o","PasswordAuthentication=no","-o","KbdInteractiveAuthentication=no","-l",&user,"localhost","/usr/bin/true"],8);
                        ResultData::Actions(vec![Evidence{output:if e.success{"VERIFIED: SSH authentication to localhost succeeded and /usr/bin/true completed. Remote SSH and GUI access are still NOT TESTED.".into()}else{format!("NOT TESTED: SSH key login did not complete. {}\nUse Terminal to review the host key and account access; passwords and keys are unchanged.",e.output)},..e}])
                    });
                }
                if ui.button("Copy client commands").clicked(){ctx.copy_text(report::guide(&s));self.say("Client commands copied.",false);}
            });
        });
        ui.add_space(12.0);
        for c in &s.checks {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&c.name).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        badge(ui, c.state);
                    });
                });
                ui.label(RichText::new(&c.detail).color(MUTED));
            });
        }
        egui::CollapsingHeader::new("Headless & High Performance requirements").show(ui, |ui| {
            ui.label(report::PERFORMANCE);
            ui.hyperlink_to(
                "Apple’s requirements",
                "https://support.apple.com/guide/remote-desktop/apdf8e09f5a9/mac",
            );
        });
        egui::CollapsingHeader::new("Recovery test plan").show(ui, |ui| {
            ui.label(report::RECOVERY);
        });
        egui::CollapsingHeader::new("Command history & observations").show(ui, |ui| {
            for (i, e) in s
                .evidence
                .iter()
                .chain(self.prefs.actions.iter())
                .enumerate()
            {
                egui::CollapsingHeader::new(format!(
                    "{}  {}",
                    if e.success { "OK" } else { "CHECK" },
                    e.command
                ))
                .id_salt(i)
                .show(ui, |ui| {
                    ui.monospace(&e.output);
                });
            }
        });
    }

    fn guide_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.header(ui,ctx,"TAKE IT WITH YOU","A clear path back to this Mac.","Connection details, observations, security checks, recovery limits, and rollback steps.");
        if self.snapshot.is_none() {
            ui.spinner();
            return;
        }
        ui.horizontal(|ui|{
            if ui.add(primary("Copy connection guide")).clicked(){ctx.copy_text(report::guide(self.snapshot.as_ref().unwrap()));self.say("Connection guide copied.",false);}
            if ui.button("Copy full report").clicked(){self.update_report();ctx.copy_text(self.report_text.clone());self.say("Full report copied. It includes host addresses and local configuration; share only with trusted people.",false);}
            if ui.button("Export report…").clicked(){
                self.update_report();
                match storage::export(&self.report_text){
                    Ok(path)=>{
                        self.say(format!("Report saved: {}",path.display()),false);
                        let p=path.to_string_lossy().to_string();
                        self.start(ctx,"Revealing the report…",move||ResultData::Actions(vec![command::run("/usr/bin/open",&["-R",&p],8)]));
                    },Err(e)=>self.say(e,true),
                }
            }
        });
        ui.add_space(10.0);
        card(ui, |ui| {
            ui.label(
                RichText::new("Start with a real connection")
                    .size(18.0)
                    .strong(),
            );
            ui.label("On your other Mac, open Screen Sharing and use a host address below. Authenticate with this Mac’s authorized account. Keep a local recovery option before testing logout, network loss, or restart.");
        });
        ui.add_space(10.0);
        let mut text = self.report_text.as_str();
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .desired_rows(24)
                .interactive(true),
        );
    }
}

impl eframe::App for MacLink {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        egui::TopBottomPanel::bottom("status")
            .resizable(false)
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(12))
            .show(ctx, |ui| {
                if self.job.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(&self.busy);
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(64.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(&self.notice).size(12.0).color(
                                if self.notice_error {
                                    Color32::from_rgb(155, 78, 30)
                                } else {
                                    MUTED
                                },
                            ));
                        });
                }
            });
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(28))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("main_scroll")
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match self.page {
                            Page::Host => self.host_page(ui, ctx),
                            Page::Connect => self.connect_page(ui, ctx),
                            Page::Checks => self.checks_page(ui, ctx),
                            Page::Guide => self.guide_page(ui, ctx),
                        }
                    });
            });
    }
}

fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
        .fill(TEAL)
        .stroke(Stroke::NONE)
}
fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(10.0).strong().color(MUTED));
}
fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(12)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}
fn mini_stat(ui: &mut egui::Ui, label: &str, value: &str, detail: &str) {
    eyebrow(ui, label);
    ui.label(RichText::new(value).size(19.0).strong());
    ui.label(RichText::new(detail).size(11.0).color(MUTED));
}
fn step(
    ui: &mut egui::Ui,
    number: &str,
    title: &str,
    detail: &str,
    action: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(number).color(TEAL).size(14.0).strong());
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(16.0).strong());
            ui.label(RichText::new(detail).size(12.0).color(MUTED));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), action);
    });
}
fn badge(ui: &mut egui::Ui, state: State) {
    let color = match state {
        State::Verified => TEAL,
        State::Action => Color32::from_rgb(161, 101, 31),
        _ => MUTED,
    };
    egui::Frame::new()
        .fill(color.gamma_multiply(0.09))
        .corner_radius(5)
        .inner_margin(5)
        .show(ui, |ui| {
            ui.label(
                RichText::new(state.label())
                    .size(10.0)
                    .strong()
                    .color(color),
            );
        });
}
fn dot(ui: &mut egui::Ui, on: bool) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(9.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(r.center(), 3.5, if on { TEAL } else { MUTED });
}
fn logo(ui: &mut egui::Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 8, TEAL);
    p.rect_stroke(
        r.shrink(9.0),
        3,
        Stroke::new(1.7_f32, Color32::WHITE),
        egui::StrokeKind::Inside,
    );
    p.line_segment(
        [
            r.left_bottom() + Vec2::new(10.0, -7.0),
            r.right_bottom() + Vec2::new(-10.0, -7.0),
        ],
        Stroke::new(1.7_f32, Color32::WHITE),
    );
}
fn computer_art(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(124.0, 100.0), egui::Sense::hover());
    let p = ui.painter();
    p.circle_filled(r.center(), 45.0, Color32::from_rgb(234, 244, 240));
    let screen = egui::Rect::from_min_size(r.min + Vec2::new(20.0, 13.0), Vec2::new(83.0, 56.0));
    p.rect_filled(screen, 5, INK);
    p.rect_filled(screen.shrink(4.0), 2, Color32::from_rgb(154, 205, 188));
    p.line_segment(
        [
            screen.left_bottom() + Vec2::new(7.0, -10.0),
            screen.right_top() + Vec2::new(-8.0, 11.0),
        ],
        Stroke::new(12.0_f32, Color32::from_rgb(195, 223, 209)),
    );
    p.rect_filled(
        egui::Rect::from_min_size(r.min + Vec2::new(57.0, 69.0), Vec2::new(10.0, 10.0)),
        0,
        INK,
    );
    p.rect_filled(
        egui::Rect::from_min_size(r.min + Vec2::new(39.0, 79.0), Vec2::new(47.0, 4.0)),
        2,
        INK,
    );
    p.rect_filled(
        egui::Rect::from_min_size(r.min + Vec2::new(84.0, 69.0), Vec2::new(28.0, 20.0)),
        5,
        Color32::from_rgb(195, 207, 207),
    );
    p.circle_filled(r.min + Vec2::new(105.0, 83.0), 1.3, TEAL);
}
