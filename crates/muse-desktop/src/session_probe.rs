//! Read-only live verification. Reports flags only, never cookies or page text.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct SessionProbe {
    started: Instant,
    evaluated: bool,
    finished: bool,
    report: PathBuf,
}

impl SessionProbe {
    pub fn new(root: &Path) -> Option<Self> {
        std::env::args()
            .any(|argument| argument == "--session-probe")
            .then(|| {
                let _ = std::fs::write(root.join("session-probe.json"), serde_json::json!({"checked":false,"status":"checking","direct_fallback":false}).to_string());
                Self {
                started: Instant::now(),
                evaluated: false,
                finished: false,
                report: root.join("session-probe.json"),
                }
            })
    }

    pub fn tick(
        &mut self,
        pool: &crate::browser::BrowserPool,
        workspace: &muse_core::Workspace,
        avatar_frames: usize,
    ) {
        if self.finished {
            return;
        }
        if self.started.elapsed() > Duration::from_secs(45) {
            let identity = workspace.active_browser_identity();
            let _ = std::fs::write(&self.report, serde_json::json!({"checked":false,"reason":"page readiness timed out","direct_fallback":false,"observer":identity.and_then(|id|pool.observer_flags(id)),"readiness":identity.map(|id|pool.readiness_diagnostics(id))}).to_string());
            if let Some(view) = identity.and_then(|id| pool.view(id.tab_id)) {
                let report = self.report.clone();
                let _=view.evaluate_script_with_callback(r#"(() => {
                    const result={observer_exists:!!window.__museDesktopObserver,document_complete:document.readyState==='complete',origin_is_muse:['https://muse.ai','https://www.muse.ai'].includes(location.origin)};
                    result.has_body=!!document.body;
                    result.content_present=!!document.body && (document.body.textContent||'').trim().length>=4;
                    result.visibility=document.visibilityState;
                    try {const s=window.__museDesktopObserver?.snapshot();result.snapshot_returned=!!s;result.snapshot_ready=!!s?.ready;result.snapshot_document_state=s?.document_state;result.snapshot_content=s?.content_present;result.style_waiting=s?.style_waiting;} catch(e){result.snapshot_error=e.name;}
                    return JSON.stringify(result);
                })()"#,move |raw| {
                    let value=serde_json::from_str::<String>(&raw).ok().and_then(|s|serde_json::from_str::<serde_json::Value>(&s).ok()).or_else(||serde_json::from_str::<serde_json::Value>(&raw).ok());
                    if let Ok(text)=std::fs::read_to_string(&report) && let Ok(mut output)=serde_json::from_str::<serde_json::Value>(&text) {
                        output["script_diagnostic"]=value.unwrap_or(serde_json::Value::Null);
                        let _=std::fs::write(&report,output.to_string());
                    }
                });
            }
            self.finished = true;
            return;
        }
        let Some(identity) = workspace.active_browser_identity() else {
            return;
        };
        let Some(tab) = workspace.active_account().and_then(|a| a.active_tab()) else {
            return;
        };
        let quota = workspace.active_account().map(|a| a.quota.clone());
        let quota_synced = quota.as_ref().is_some_and(|q| {
            matches!(
                q,
                muse_core::QuotaState::Known(_) | muse_core::QuotaState::Unlimited { .. }
            )
        });
        if self.started.elapsed() < Duration::from_secs(8)
            || tab.loading
            || self.evaluated
            || !pool.page_ready(identity)
            || (!quota_synced && self.started.elapsed() < Duration::from_secs(32))
            || (avatar_frames < 2 && self.started.elapsed() < Duration::from_secs(24))
        {
            return;
        }
        let Some(view) = pool.view(identity.tab_id) else {
            return;
        };
        self.evaluated = true;
        let report = self.report.clone();
        let profile_verified = pool.profile_verified(identity.account_id);
        let observer_flags = pool.observer_flags(identity).cloned();
        let remaining = quota
            .as_ref()
            .and_then(|q| q.remaining_ratio())
            .map(|r| r * 100.);
        let extra=workspace.active_account().and_then(|account|match &account.extra_quota {
            muse_core::ExtraQuotaState::Known(value)=>Some(serde_json::json!({"remaining":value.remaining(),"total":value.total(),"display_available":true})),
            _=>None,
        });
        let script = r#"(() => {
            const text = (document.body?.innerText || '').slice(0, 60000);
            const u = new URL(location.href);
            const errorPage = /ERR_PROXY|ERR_TUNNEL|ERR_CONNECTION|ERR_NAME_NOT_RESOLVED|Local proxy|上游代理|代理连接|代理认证/.test(text) || u.protocol === 'chrome-error:';
            const credentialType = String.fromCharCode(112,97,115,115,119,111,114,100);
            const loginForm = !!document.querySelector('input[type="' + credentialType + '"]') || /\/(login|signin|sign-in)(\/|$)/i.test(u.pathname);
            const signedIn = !loginForm && /sign\s*out|log\s*out|退出登录|登出|my\s+(library|videos)|我的视频|account\s+settings/i.test(text);
            return JSON.stringify({ checked: true, reached_muse: u.protocol === 'https:' && (u.hostname === 'muse.ai' || u.hostname === 'www.muse.ai'),
                browser_error: errorPage, login_form: loginForm, signed_in_ui: signedIn,
                session_validity: signedIn ? 'signed-in UI detected' : loginForm ? 'login required' : 'not conclusively determined', direct_fallback: false });
        })()"#;
        let _ = view.evaluate_script_with_callback(script, move |value| {
            let parsed = serde_json::from_str::<String>(&value).ok().and_then(|inner| serde_json::from_str::<serde_json::Value>(&inner).ok())
                .or_else(|| serde_json::from_str::<serde_json::Value>(&value).ok());
            let mut output = parsed.unwrap_or_else(|| serde_json::json!({"checked":false,"reason":"page probe unavailable","direct_fallback":false}));
            output["actual_profile_verified"]=serde_json::json!(profile_verified);
            output["page_readiness_verified"]=serde_json::json!(true);
            output["quota_synced"]=serde_json::json!(quota_synced);
            output["quota_remaining_percent"]=serde_json::json!(remaining);
            output["quota_adapter"]=serde_json::json!(observer_flags);
            output["extra_quota"]=serde_json::json!(extra);
            output["site_avatar_loaded"]=serde_json::json!(avatar_frames > 0);
            output["site_avatar_animation_frames"]=serde_json::json!(avatar_frames);
            let _ = std::fs::write(&report, serde_json::to_string_pretty(&output).unwrap_or_default());
        });
        self.finished = true;
    }
}
