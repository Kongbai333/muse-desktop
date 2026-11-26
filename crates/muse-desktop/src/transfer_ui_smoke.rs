//! Debug-only synthetic end-to-end UI driver. Injects file-picker results; never uses user data.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn passphrase() -> Zeroizing<String> {
    let source = include_str!("../../../checks/fixtures/account-transfer-v1/unicode.properties");
    let encoded = source
        .lines()
        .find_map(|l| l.strip_prefix("passphrase_utf8_base64="))
        .expect("synthetic property");
    Zeroizing::new(
        String::from_utf8(STANDARD.decode(encoded).expect("synthetic Base64"))
            .expect("synthetic UTF-8"),
    )
}
impl MuseWindow {
    pub(in crate::ui) fn advance_transfer_smoke(
        &mut self,
        smoke: &mut Smoke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.transfer_smoke_step(smoke, window, cx);
        if let Err(error) = result {
            let report = serde_json::json!({"passed":false,"stage":smoke.stage,"error":error});
            let _ = std::fs::write(self.data_dir.join("smoke-result.json"), report.to_string());
            cx.quit();
        }
    }
    fn transfer_smoke_step(
        &mut self,
        smoke: &mut Smoke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.diagnostic_origin.is_none() {
            return Err("encrypted smoke requires diagnostic fixture".into());
        }
        if smoke.started.elapsed() > Duration::from_secs(65) {
            return Err(format!("encrypted UI timeout at phase {}", smoke.stage));
        }
        let package = self.data_dir.join("synthetic.muse-account");
        let password = passphrase();
        match smoke.stage {
            0 => {
                let Some(identity) = self
                    .workspace
                    .active_browser_identity()
                    .filter(|i| self.browsers.page_ready(*i))
                else {
                    return Ok(());
                };
                // Exercise real secure inputs and validation before bypassing only the OS picker.
                self.open_transfer(true, identity.account_id, window, cx);
                let d = self.transfer_dialog.as_mut().ok_or("export form missing")?;
                let input = d.password.clone();
                d.password.update(cx, |i, cx| i.set_text(&password, cx));
                d.confirmation
                    .update(cx, |i, cx| i.set_text("SYNTHETIC_MISMATCH", cx));
                self.submit_transfer(window, cx);
                if self
                    .transfer_dialog
                    .as_ref()
                    .is_none_or(|d| d.phase != Phase::Form)
                {
                    return Err("mismatched password opened picker".into());
                }
                self.close_transfer(window, cx);
                if !input.read(cx).text().is_empty() {
                    return Err("cancel did not clear secure input".into());
                }
                super::super::transfer_smoke::check(self, window)?;
                smoke.first = identity.account_id;
                smoke.edit_before = Some(identity);
                smoke.edit_count = self.workspace.accounts.len();
                let mut set = crate::account_transfer::decode_payload(include_bytes!(
                    "../../../checks/fixtures/account-transfer-v1/payload.json"
                ))
                .map_err(|_| "synthetic payload")?
                .accounts
                .remove(0)
                .cookies;
                set.items[0].expires_at_unix_seconds =
                    Some((crate::page_observer::now_ms() / 1000 + 3600) as i64);
                let mut payload = crate::transfer_plan::export_payload(
                    &self.data_dir,
                    &self.workspace,
                    &[(identity.account_id, set)],
                    TargetPlatform::current(),
                )?;
                let mut second = payload.accounts[0].clone();
                second.account_ref = "second".into();
                second.name = "合成迁移 B".into();
                payload.accounts.push(second);
                let path = package.clone();
                let pass = passphrase();
                cx.background_executor()
                    .spawn(async move {
                        let bytes = wire::encrypt(&payload, &pass).expect("synthetic encryption");
                        wire::write_file(&path, &bytes).expect("synthetic file");
                    })
                    .detach();
                smoke.stage = 1;
            }
            1 if package.exists() => {
                self.smoke_import_form(package, "SYNTHETIC_WRONG", window, cx);
                smoke.stage = 2;
            }
            2 => {
                let Some(d) = self.transfer_dialog.as_ref() else {
                    return Err("import form missing".into());
                };
                if d.phase == Phase::Form {
                    if !d.message.contains("密码错误")
                        || self.workspace.accounts.len() != smoke.edit_count
                    {
                        return Err("wrong password mutated accounts".into());
                    }
                    self.close_transfer(window, cx);
                    self.smoke_import_form(package, &password, window, cx);
                    smoke.stage = 3;
                }
            }
            3 => {
                let Some(d) = self.transfer_dialog.as_ref() else {
                    return Err("preview missing".into());
                };
                if d.phase == Phase::Form {
                    return Err(d.message.clone());
                }
                if d.phase == Phase::Preview {
                    let plan = d.plan.as_ref().ok_or("preview plan missing")?;
                    if plan
                        .accounts
                        .iter()
                        .any(|a| self.browsers.profile_verified(a.id))
                        || self.workspace.accounts.len() != smoke.edit_count
                    {
                        return Err("preview staged or published account".into());
                    }
                    self.submit_transfer(window, cx);
                    if self
                        .transfer_dialog
                        .as_ref()
                        .is_none_or(|d| d.phase != Phase::Preview)
                    {
                        return Err("preview bypassed acknowledgment".into());
                    }
                    self.transfer_dialog.as_mut().unwrap().acknowledged = true;
                    self.submit_transfer(window, cx);
                    // Cancel while the real asynchronous vault job may be running.
                    self.close_transfer(window, cx);
                    if self.workspace.accounts.len() != smoke.edit_count {
                        return Err("cancelled vault job published accounts".into());
                    }
                    self.smoke_import_form(package, &password, window, cx);
                    smoke.stage = 4;
                }
            }
            4 => {
                let Some(d) = self.transfer_dialog.as_ref() else {
                    return Err("second preview missing".into());
                };
                if d.phase == Phase::Form {
                    return Err(d.message.clone());
                }
                if d.phase == Phase::Preview {
                    self.workspace
                        .rename_account(smoke.first, "原账号：预览期间修改")?;
                    self.persist();
                    let _ =
                        std::fs::write(self.data_dir.join("transfer-ui-preview-ready"), "ready");
                    smoke.paint_started = Some(Instant::now());
                    smoke.stage = 5;
                }
            }
            5 if smoke
                .paint_started
                .is_some_and(|at| at.elapsed() > Duration::from_millis(1500)) =>
            {
                self.transfer_dialog
                    .as_mut()
                    .ok_or("preview vanished")?
                    .acknowledged = true;
                self.submit_transfer(window, cx);
                smoke.stage = 6;
            }
            6 => {
                let Some(d) = self.transfer_dialog.as_ref() else {
                    return Err("import progress vanished".into());
                };
                if d.phase == Phase::Form {
                    return Err(d.message.clone());
                }
                if d.phase == Phase::Done {
                    if d.message.contains("部分页面") {
                        return Err("activation lost a prepared native candidate".into());
                    }
                    if self.workspace.accounts.len() != smoke.edit_count + 2
                        || self
                            .workspace
                            .account(smoke.first)
                            .is_none_or(|a| a.name != "原账号：预览期间修改")
                    {
                        return Err("commit overwrote latest workspace".into());
                    }
                    if !d.password.read(cx).text().is_empty()
                        || !d.confirmation.read(cx).text().is_empty()
                    {
                        return Err("password remained in input".into());
                    }
                    let accounts = &self.workspace.accounts[smoke.edit_count..];
                    if accounts[0].network.proxy_id() != accounts[1].network.proxy_id() {
                        return Err("shared proxy mapping changed".into());
                    }
                    for a in accounts {
                        if !self.browsers.profile_verified(a.id) {
                            return Err("new profile ownership not verified".into());
                        }
                        let view = self
                            .browsers
                            .view(a.active_tab_id())
                            .ok_or("activated view missing")?;
                        let cookies = view
                            .cookies_for_url("https://muse.ai/api/session")
                            .map_err(|_| "native scoped read")?;
                        if !cookies
                            .iter()
                            .any(|c| c.name() == "hatch_sess" && c.http_only() == Some(true))
                        {
                            return Err("activated native Cookie missing".into());
                        }
                    }
                    let metadata = self.workspace.to_json()?;
                    if metadata.contains("SYNTHETIC_SESSION") || metadata.contains("hatch_sess") {
                        return Err("metadata contains Cookie".into());
                    }
                    smoke.paint_started = Some(Instant::now());
                    smoke.stage = 7;
                }
            }
            7 => {
                let accounts = &self.workspace.accounts[smoke.edit_count..];
                let ready = accounts.iter().all(|a| {
                    self.browsers.page_ready(muse_core::BrowserIdentity {
                        account_id: a.id,
                        tab_id: a.active_tab_id(),
                        generation: a.tabs[0].generation,
                    })
                });
                if !ready {
                    if smoke
                        .paint_started
                        .is_some_and(|at| at.elapsed() > Duration::from_secs(8))
                    {
                        let states:Vec<_>=accounts.iter().map(|a|{let id=muse_core::BrowserIdentity{account_id:a.id,tab_id:a.active_tab_id(),generation:a.tabs[0].generation};serde_json::json!({"identity_current":self.workspace.contains_identity(id),"reader":self.browsers.is_cookie_reader(id),"gate":self.browsers.readiness_diagnostics(id),"observer":self.browsers.observer_flags(id),"requests":self.browsers.request_count(a.id),"pending_identity_check":self.cookie_logins.contains_key(&a.id)})}).collect();
                        return Err(format!(
                            "synthetic readiness diagnostics: {}",
                            serde_json::json!({"pages":states,"upstream":crate::proxy_smoke::diagnostics()})
                        ));
                    }
                    return Ok(());
                }
                let proxy_route = accounts.iter().all(|a| {
                    a.network == muse_core::AccountNetwork::Direct
                        || (self.browsers.route_label(a.id).is_some()
                            && self.browsers.profile_verified(a.id))
                });
                if !proxy_route {
                    return Err("activated diagnostic request has no verified account route".into());
                }
                let ids: Vec<_> = accounts.iter().map(|a| a.id).collect();
                self.close_transfer(window, cx);
                self.open_transfer(true, ids[0], window, cx);
                let d = self
                    .transfer_dialog
                    .as_mut()
                    .ok_or("reexport form missing")?;
                d.selected = ids.iter().copied().collect();
                d.queue = ids.into();
                d.path = Some(self.data_dir.join("reexport.muse-account"));
                d.pending_password = Some(passphrase());
                d.snapshot_workspace = Some(self.workspace.clone());
                d.phase = Phase::Snapshot;
                smoke.stage = 8;
            }
            8 => {
                let Some(d) = self.transfer_dialog.as_ref() else {
                    return Err("export progress missing".into());
                };
                if d.phase == Phase::Form {
                    return Err(d.message.clone());
                }
                if d.phase == Phase::Done {
                    let bytes = wire::read_file(&self.data_dir.join("reexport.muse-account"))
                        .map_err(|_| "reexport file missing")?;
                    let exported =
                        wire::decrypt(&bytes, &password).map_err(|_| "reexport failed decode")?;
                    if exported.accounts.len() != 2 || exported.proxies.len() > 1 {
                        return Err("reexport graph changed".into());
                    }
                    self.close_transfer(window, cx);
                    let report = serde_json::json!({"passed":true,"password_confirmation":true,"password_input_cleared":true,"wrong_password_atomic":true,"preview_before_profiles":true,"preview_confirmation_required":true,"cancelled_vault_not_activated":true,"latest_workspace_preserved":true,"native_profile_isolation":true,"native_scoped_cookie_readback":true,"shared_static_proxy_binding":true,"activation_after_commit":true,"diagnostic_route_used":true,"encrypted_reexport":true,"real_service_identity_verified":false,"file_picker_result_injected":true});
                    std::fs::write(self.data_dir.join("smoke-result.json"), report.to_string())
                        .map_err(|_| "synthetic report")?;
                    cx.quit();
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn smoke_import_form(
        &mut self,
        path: PathBuf,
        password: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let owner = self
            .workspace
            .active_account_id()
            .expect("diagnostic account");
        let mut d = TransferDialog::new(false, owner, cx);
        d.path = Some(path);
        d.password.update(cx, |i, cx| i.set_text(password, cx));
        self.transfer_dialog = Some(d);
        self.ensure_page_freeze();
        self.browsers
            .sync_visibility(self.visible_browser_identity(), self.browser_bounds(window));
        self.start_transfer_import(cx);
    }
}
