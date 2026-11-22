//! Native UI integration checks. Only synthetic cookies in disposable diagnostic profiles.
use super::*;

pub(in crate::ui) fn begin(
    this: &mut MuseWindow,
    window: &mut Window,
    cx: &mut Context<MuseWindow>,
) -> Result<muse_core::BrowserIdentity, String> {
    if std::env::args().any(|arg| arg == "--transfer-smoke-test") {
        super::super::transfer_smoke::check(this, window)?;
    }
    let original = this
        .workspace
        .active_browser_identity()
        .ok_or("missing original account")?;
    if this
        .workspace
        .account(original.account_id)
        .is_some_and(|account| account.network != muse_core::AccountNetwork::Direct)
    {
        crate::proxy_pool::migrate(&this.data_dir, &mut this.workspace)?;
    }
    let network = this
        .workspace
        .account(original.account_id)
        .ok_or("missing original route")?
        .network;
    let before = this
        .workspace
        .to_json()
        .map_err(|_| "cannot serialize synthetic workspace")?;
    let count = this.workspace.accounts.len();
    this.open_account_dialog(None, false, window, cx);
    if let Some(dialog) = this.account_dialog.as_mut() {
        dialog.route = crate::proxy_pool::RouteChoice::from(network);
        dialog.force = network.force_proxy();
    }
    let input = this
        .account_dialog
        .as_ref()
        .ok_or("missing new account form")?
        .cookie
        .clone();
    input.update(cx, |input, cx| {
        input.set_text("hatch_vml=synthetic-no-session", cx)
    });
    this.save_account_dialog(window, cx);
    if this.workspace.to_json().ok().as_deref() != Some(before.as_str()) {
        return Err("invalid input changed the account list".into());
    }
    this.close_account_dialog(window, cx);
    if !input.read(cx).text().is_empty() {
        return Err("cancel did not clear the masked Cookie field".into());
    }
    this.open_account_dialog(None, false, window, cx);
    if let Some(dialog) = this.account_dialog.as_mut() {
        dialog.route = crate::proxy_pool::RouteChoice::from(network);
        dialog.force = network.force_proxy();
    }
    let input = this
        .account_dialog
        .as_ref()
        .ok_or("missing reopened account form")?
        .cookie
        .clone();
    let expires = crate::page_observer::now_ms() / 1000 + 3600;
    let value = serde_json::json!([{
        "name":"hatch_sess", "value":"muse-cookie-login-fixture",
        "path":"/", "expires":expires, "httpOnly":true, "secure":false, "sameSite":"lax"
    }])
    .to_string();
    input.update(cx, |input, cx| input.set_text(&value, cx));
    let home = this.home.clone();
    this.home = format!("{home}cookie-login");
    this.save_account_dialog(window, cx);
    this.home = home;
    let imported = this
        .workspace
        .active_browser_identity()
        .ok_or("missing imported account")?;
    if imported.account_id == original.account_id || this.workspace.accounts.len() != count + 1 {
        return Err("Cookie import did not create exactly one new account".into());
    }
    if this
        .workspace
        .account(imported.account_id)
        .is_none_or(|account| account.network != network)
    {
        return Err("Cookie import did not preserve the selected network route".into());
    }
    if !input.read(cx).text().is_empty() || !this.cookie_logins.contains_key(&imported.account_id) {
        return Err("Cookie import did not clear its input or start website verification".into());
    }
    let stored = this
        .browsers
        .view(imported.tab_id)
        .ok_or("missing imported browser")?
        .cookies()
        .map_err(|_| "native Cookie read-back failed")?;
    if !stored.iter().any(|cookie| {
        cookie.name() == "hatch_sess"
            && cookie.value() == "muse-cookie-login-fixture"
            && cookie.http_only() == Some(true)
            && cookie
                .expires_datetime()
                .is_some_and(|at| at.unix_timestamp() > expires as i64 - 2)
    }) {
        return Err("native browser did not preserve the imported session and attributes".into());
    }
    let existing = this
        .browsers
        .view(original.tab_id)
        .ok_or("missing original browser")?
        .cookies()
        .map_err(|_| "original Cookie read-back failed")?;
    if existing
        .iter()
        .any(|cookie| cookie.value() == "muse-cookie-login-fixture")
    {
        return Err("Cookie import escaped the new account profile".into());
    }
    let metadata = std::fs::read_to_string(this.data_dir.join("workspace.json"))
        .map_err(|_| "cannot inspect synthetic workspace metadata")?;
    if metadata.contains("muse-cookie-login-fixture") || metadata.contains("hatch_sess") {
        return Err("Cookie credentials were written into workspace metadata".into());
    }
    Ok(original)
}

pub(in crate::ui) fn finish(
    this: &mut MuseWindow,
    original: muse_core::BrowserIdentity,
    expected_accounts: usize,
    window: &mut Window,
    cx: &mut Context<MuseWindow>,
) -> bool {
    let Some(imported) = this
        .workspace
        .active_account_id()
        .filter(|id| *id != original.account_id)
    else {
        return false;
    };
    this.open_account_dialog(Some(imported), true, window, cx);
    this.save_account_dialog(window, cx);
    if this.workspace.select_account(original.account_id).is_err() {
        return false;
    }
    this.sync_fields(cx);
    this.persist();
    this.workspace.accounts.len() == expected_accounts
        && this.workspace.account(imported).is_none()
        && this.workspace.active_browser_identity() == Some(original)
        && this.browsers.profile_verified(original.account_id)
}
