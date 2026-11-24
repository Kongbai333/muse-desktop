//! Only runs under the existing disposable diagnostic smoke harness.
use super::*;
pub(in crate::ui) fn check(this: &mut MuseWindow, window: &Window) -> Result<(), String> {
    if this.diagnostic_origin.is_none() {
        return Err("transfer smoke requires a disposable diagnostic profile".into());
    }
    let before = this.workspace.to_json()?;
    let original = this
        .workspace
        .active_browser_identity()
        .ok_or("missing original")?;
    let mut payload = crate::account_transfer::decode_payload(include_bytes!(
        "../../../checks/fixtures/account-transfer-v1/payload.json"
    ))
    .map_err(|_| "invalid synthetic fixture")?;
    // Only the native fixture uses a short lifetime; canonical wire vectors remain unchanged.
    payload.accounts[0].cookies.items[0].expires_at_unix_seconds =
        Some((crate::page_observer::now_ms() / 1000 + 3600) as i64);
    payload.accounts[0].cookies.items[0].host_only = Some(false);
    let mut scoped = payload.accounts[0].cookies.items[0].clone();
    scoped.path = Some("/api".into());
    scoped.session = Some(true);
    scoped.expires_at_unix_seconds = None;
    // Exercise false as well as true through the actual native migration writer.
    scoped.secure = Some(false);
    payload.accounts[0].cookies.items.push(scoped);
    let mut second = payload.accounts[0].clone();
    second.account_ref = "second".into();
    second.name = "合成账号 B".into();
    second.cookies.items.truncate(1);
    second.cookies.items[0].host_only = Some(true);
    second.cookies.items[0].domain = Some("muse.ai".into());
    payload.accounts.push(second);
    let mut plan = crate::transfer_plan::ImportPlan::prepare(
        payload,
        crate::transfer_plan::TargetPlatform::current(),
        (crate::page_observer::now_ms() / 1000) as i64,
    )?;
    plan.confirm();
    plan.stage_vault(&this.data_dir)?;
    let merged = plan.merge_into(&this.workspace)?;
    let identities: Vec<_> = plan
        .accounts
        .iter()
        .map(|a| muse_core::BrowserIdentity {
            account_id: a.id,
            tab_id: a.active_tab_id(),
            generation: a.tabs[0].generation,
        })
        .collect();
    let result = (|| -> Result<_, String> {
        let mut snapshots = Vec::new();
        for identity in &identities {
            let account = merged
                .account(identity.account_id)
                .ok_or("missing candidate")?;
            let profile = account.profile_dir(&this.data_dir.join("profiles"));
            this.browsers.stage_transfer(
                BrowserRequest {
                    identity: *identity,
                    url: "about:blank",
                    profile: &profile,
                    network: account.network,
                    fingerprint: account.fingerprint,
                    proxy_credentials: merged.proxy_credentials(account),
                    diagnostic_origin: this.diagnostic_origin.as_deref(),
                },
                &plan.cookies[&account.id],
                window,
                this.browser_bounds(window),
            )?;
            if !this.browsers.is_cookie_reader(*identity)
                || !this.browsers.profile_verified(account.id)
                || this
                    .browsers
                    .view(identity.tab_id)
                    .and_then(crate::browser::current_url)
                    .as_deref()
                    != Some("about:blank")
            {
                return Err("candidate navigated or lost its profile before commit".into());
            }
            plan.mark_staged(account.id)?;
            let snapshot = this.browsers.snapshot_transfer(*identity)?;
            if snapshot.items.len() != plan.cookies[&account.id].items.len() {
                return Err("native scope snapshot lost cookies".into());
            }
            snapshots.push((account.id, snapshot));
        }
        let exported = crate::transfer_plan::export_payload(
            &this.data_dir,
            &merged,
            &snapshots,
            crate::transfer_plan::TargetPlatform::current(),
        )?;
        if exported.proxies.len() != 1 || exported.accounts.len() != 2 {
            return Err("shared proxy snapshot changed binding".into());
        }
        if exported.accounts[0]
            .cookies
            .items
            .iter()
            .filter(|c| c.name == "hatch_sess")
            .count()
            != 2
        {
            return Err("same-name path scopes coalesced".into());
        }
        let original_cookies = this
            .browsers
            .view(original.tab_id)
            .ok_or("original browser missing")?
            .cookies()
            .map_err(|_| "original native read failed")?;
        if original_cookies
            .iter()
            .any(|c| c.value() == "SYNTHETIC_SESSION==%2F%3D%25+ONLY")
        {
            return Err("migration escaped candidate profile".into());
        }
        if this.workspace.to_json()? != before {
            return Err("native staging published account metadata".into());
        }
        Ok(())
    })();
    for identity in identities {
        this.browsers.discard_transfer(identity);
    }
    drop(plan); // Rolls back only these new synthetic vault revisions.
    result?;
    std::fs::write(this.data_dir.join("transfer-native-result.json"), "{\"passed\":true,\"native_scope_readback\":true,\"http_only_same_site_expiry\":true,\"shared_proxy_snapshot\":true,\"account_isolation\":true,\"no_navigation_before_commit\":true,\"no_metadata_before_commit\":true}").map_err(|_| "cannot write synthetic report")?;
    Ok(())
}
