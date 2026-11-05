//! Explicit SDK pinning. Background work is scoped to the chooser's workspace snapshot.

use egui::Ui;
use rc_core::{Inbox, Waker};
use rc_dotnet::{OpHandle, SdkInfo, global_json::GlobalJson};
use rc_kit::i18n::{tr, trf};
use std::path::PathBuf;

pub struct SdkSelection {
    root: PathBuf,
    load: Inbox<Result<(GlobalJson, Vec<SdkInfo>), String>>,
    saved: Inbox<Result<(), String>>,
    _scan: OpHandle,
    file: Option<GlobalJson>,
    versions: Vec<SdkInfo>,
    selected: String,
    loading: bool,
    saving: bool,
    error: Option<String>,
}

impl SdkSelection {
    pub fn new(root: PathBuf, dotnet: PathBuf, waker: Waker) -> Self {
        let load = Inbox::new(waker.clone());
        let scan = rc_dotnet::global_json::start_load(root.clone(), dotnet, load.sender());
        Self {
            root,
            load,
            saved: Inbox::new(waker),
            _scan: scan,
            file: None,
            versions: Vec::new(),
            selected: String::new(),
            loading: true,
            saving: false,
            error: None,
        }
    }

    /// Returns true once the accepted write has completed; closing never blocks on SDK discovery.
    pub fn contents(&mut self, ui: &mut Ui) -> (bool, bool) {
        for result in self.load.drain() {
            self.loading = false;
            match result {
                Ok((file, versions)) => {
                    self.selected = file.version().unwrap_or_default().to_owned();
                    self.file = Some(file);
                    self.versions = versions;
                }
                Err(error) => self.error = Some(error),
            }
        }
        for result in self.saved.drain() {
            self.saving = false;
            match result {
                Ok(()) => return (true, true),
                Err(error) => self.error = Some(error),
            }
        }
        ui.label(tr("Writes global.json in the project root. Ancestor files are never changed."));
        ui.label(self.root.join("global.json").display().to_string());
        if self.loading || self.saving {
            ui.spinner();
        }
        if let Some(file) = &self.file {
            ui.label(trf("Current SDK pin: {version}", &[("version", &file.version().unwrap_or(tr("None")))]));
        }
        ui.add_enabled_ui(!self.loading && !self.saving, |ui| {
            ui.label(tr("Installed SDK"));
            egui::ComboBox::from_id_salt("workspace_sdk_version")
                .selected_text(if self.selected.is_empty() { tr("Select an SDK") } else { &self.selected })
                .show_ui(ui, |ui| {
                    for sdk in &self.versions {
                        ui.selectable_value(&mut self.selected, sdk.version.clone(), &sdk.version);
                    }
                });
            if !self.loading && self.versions.is_empty() {
                ui.label(tr("No installed SDKs were found."));
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(rc_kit::pal(ui).error, trf("Could not update SDK selection: {error}", &[("error", error)]));
        }
        let can_save = !self.loading && !self.saving && self.versions.iter().any(|sdk| sdk.version == self.selected) && self.file.is_some();
        if ui.add_enabled(can_save, egui::Button::new(tr("Save SDK selection"))).clicked()
            && let Some(file) = self.file.clone()
        {
            let version = self.selected.clone();
            let out = self.saved.sender();
            self.saving = true;
            self.error = None;
            std::thread::spawn(move || {
                out.send(file.save_version(&version).map_err(|e| format!("{e:#}")));
            });
        }
        let close = ui.add_enabled(!self.saving, egui::Button::new(tr("Close"))).clicked();
        (close, !self.saving)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    #[test]
    fn sdk_chooser_only_writes_after_explicit_save() {
        rc_kit::i18n::set_lang(rc_kit::i18n::Lang::En);
        let root = tempfile::tempdir().unwrap();
        let host = root.path().join("dotnet");
        std::fs::write(&host, "#!/bin/sh\nprintf '10.0.100 [/sdk]\\n'\n").unwrap();
        std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o755)).unwrap();
        let state = SdkSelection::new(root.path().to_path_buf(), host, Waker::noop());
        let mut h = Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
            |ui, state: &mut SdkSelection| {
                state.contents(ui);
            },
            state,
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.state().loading && Instant::now() < deadline {
            h.run_steps(1);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!h.state().loading);
        assert!(h.state().error.is_none(), "{:?}", h.state().error);
        assert!(!root.path().join("global.json").exists());
        h.state_mut().selected = "10.0.100".into();
        h.run_steps(2);
        h.get_by_label("Save SDK selection").click();
        h.run_steps(2);
        while h.state().saving && Instant::now() < deadline {
            h.run_steps(1);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!h.state().saving);
        assert_eq!(GlobalJson::load(root.path()).unwrap().version(), Some("10.0.100"));
    }
}
