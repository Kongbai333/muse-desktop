//! Explicit, account-scoped WebView2 cookie export. Values never enter logs.
use muse_core::{AccountId, BrowserIdentity};
use serde::Serialize;
use std::collections::BTreeMap;
use zeroize::{Zeroize, Zeroizing};

#[derive(Serialize)]
pub struct CookieItem {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: Option<i64>,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: String,
}
impl Drop for CookieItem {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}
pub struct CookieBundle {
    pub items: Vec<CookieItem>,
    pub captured_at: u64,
}
pub struct CookieTarget {
    pub id: AccountId,
    pub name: String,
    pub identity: Option<BrowserIdentity>,
    pub request: uuid::Uuid,
    pub temporary: bool,
    pub prior_active_account: Option<AccountId>,
    pub prior_active_tab: Option<muse_core::TabId>,
    pub requested: bool,
    pub finished: bool,
    pub started: std::time::Instant,
    pub status: String,
    pub bundle: Option<CookieBundle>,
    pub selected: bool,
}
pub struct CookieDialog {
    pub targets: Vec<CookieTarget>,
    pub message: String,
    pub extracting: bool,
    pub transition: crate::motion::TransitionState,
}
impl CookieDialog {
    pub fn begin_exit(&mut self) -> Option<(u64, Vec<(BrowserIdentity, bool)>)> {
        let generation = self.transition.begin_exit()?;
        self.extracting = false;
        let resources = self
            .targets
            .iter_mut()
            .filter_map(|target| {
                let identity = target.identity.take();
                let temporary = std::mem::replace(&mut target.temporary, false);
                target.requested = false;
                target.finished = true;
                target.bundle = None;
                identity.map(|identity| (identity, temporary))
            })
            .collect();
        Some((generation, resources))
    }
}

/// Matches muse2api's admin frontend: one raw header, or labelled lines for a batch.
pub fn muse2api_text(
    accounts: &[(&str, &CookieBundle)],
) -> Result<Zeroizing<String>, &'static str> {
    if accounts.is_empty() {
        return Err("请至少勾选一个账号");
    }
    let mut output = Zeroizing::new(String::new());
    for (label, bundle) in accounts {
        let mut cookies = BTreeMap::<&str, &CookieItem>::new();
        for item in &bundle.items {
            if item
                .expires
                .is_some_and(|expires| expires >= 0 && expires as u64 <= bundle.captured_at / 1000)
            {
                continue;
            }
            if item.name.is_empty()
                || item
                    .name
                    .bytes()
                    .any(|byte| byte <= 32 || matches!(byte, b'=' | b';' | b'|' | 127))
                || item.value.chars().any(|ch| ch.is_control() || ch == ';')
            {
                continue;
            }
            let previous = cookies.get(item.name.as_str());
            if previous.is_none_or(|previous| item.path.len() > previous.path.len()) {
                cookies.insert(&item.name, item);
            }
        }
        if cookies
            .get("hatch_sess")
            .is_none_or(|item| item.value.is_empty())
        {
            return Err("所选账号缺少有效 hatch_sess，请先登录后重试");
        }
        if !output.is_empty() {
            output.push('\n');
        }
        if accounts.len() > 1 {
            let label = label
                .chars()
                .map(|ch| {
                    if ch.is_control() || matches!(ch, '|' | '=' | '#') {
                        ' '
                    } else {
                        ch
                    }
                })
                .collect::<String>();
            let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
            output.push_str(if label.is_empty() { "Muse" } else { &label });
            output.push_str(" | ");
        }
        for (index, (name, item)) in cookies.iter().enumerate() {
            if index > 0 {
                output.push_str("; ");
            }
            output.push_str(name);
            output.push('=');
            output.push_str(&item.value);
        }
        if output.len() > 1_000_000 {
            return Err("所选 Cookie 数据过多，请减少账号数量");
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn bundle(session: &str) -> CookieBundle {
        CookieBundle {
            captured_at: 1,
            items: vec![
                CookieItem {
                    name: "hatch_sess".into(),
                    value: session.into(),
                    domain: "muse.ai".into(),
                    path: "/".into(),
                    expires: None,
                    http_only: true,
                    secure: true,
                    same_site: "Lax".into(),
                },
                CookieItem {
                    name: "hatch_vml".into(),
                    value: "workspace".into(),
                    domain: "muse.ai".into(),
                    path: "/".into(),
                    expires: None,
                    http_only: true,
                    secure: true,
                    same_site: "Lax".into(),
                },
            ],
        }
    }

    #[test]
    fn beginning_cookie_dialog_exit_releases_temporary_resources_and_cookie_values() {
        let identity = BrowserIdentity {
            account_id: uuid::Uuid::new_v4(),
            tab_id: uuid::Uuid::new_v4(),
            generation: 3,
        };
        let mut dialog = CookieDialog {
            targets: vec![CookieTarget {
                id: identity.account_id,
                name: "test".into(),
                identity: Some(identity),
                request: uuid::Uuid::new_v4(),
                temporary: true,
                prior_active_account: None,
                prior_active_tab: None,
                requested: true,
                finished: false,
                started: Instant::now(),
                status: "正在读取登录 Cookie…".into(),
                bundle: Some(bundle("sensitive-session")),
                selected: true,
            }],
            message: "读取中".into(),
            extracting: true,
            transition: crate::motion::TransitionState::entering(),
        };
        dialog.targets.push(CookieTarget {
            id: uuid::Uuid::new_v4(),
            name: "completed".into(),
            identity: None,
            request: uuid::Uuid::new_v4(),
            temporary: false,
            prior_active_account: None,
            prior_active_tab: None,
            requested: true,
            finished: true,
            started: Instant::now(),
            status: "已读取".into(),
            bundle: Some(bundle("completed-session")),
            selected: true,
        });

        let (generation, resources) = dialog.begin_exit().unwrap();

        assert_eq!(resources, vec![(identity, true)]);
        assert!(!dialog.extracting);
        assert!(dialog.targets[0].identity.is_none());
        assert!(!dialog.targets[0].temporary);
        assert!(dialog.targets[0].bundle.is_none());
        assert!(dialog.targets[1].bundle.is_none());
        assert!(dialog.targets[0].finished);
        assert!(dialog.transition.finish_exit(generation));
        assert!(dialog.begin_exit().is_none());
    }
    #[test]
    fn muse2api_single_account_uses_the_admin_cookie_header_route() {
        let account = bundle("fixture-single==");
        let text = muse2api_text(&[("小号", &account)]).unwrap();
        assert_eq!(&*text, "hatch_sess=fixture-single==; hatch_vml=workspace");
        assert!(!text.contains('|'));
    }
    #[test]
    fn muse2api_multi_account_preserves_labels_and_credential_boundaries() {
        let first = bundle("fixture-first==");
        let second = bundle("fixture-second");
        let text = muse2api_text(&[("主号 | = test\nextra", &first), ("小号", &second)]).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert_eq!(
            &*text,
            "主号 test extra | hatch_sess=fixture-first==; hatch_vml=workspace\n小号 | hatch_sess=fixture-second; hatch_vml=workspace"
        );
    }
    #[test]
    fn muse2api_missing_session_rejects_the_whole_selection() {
        let valid = bundle("fixture-valid");
        let mut invalid = bundle("");
        invalid.items.remove(0);
        assert!(muse2api_text(&[("valid", &valid), ("invalid", &invalid)]).is_err());
        assert!(muse2api_text(&[]).is_err());
    }

    #[test]
    fn muse2api_reference_admin_import_contract() {
        let single = bundle("fixture-single==");
        let first = bundle("fixture-first==");
        let second = bundle("fixture-second");
        let data = serde_json::json!({
            "single":muse2api_text(&[("小号",&single)]).unwrap().as_str(),
            "batch":muse2api_text(&[("主号 | = test\nextra",&first),("小号",&second)]).unwrap().as_str(),
        });
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixture = root.join("target/cookie-import-synthetic.json");
        std::fs::write(&fixture, data.to_string()).unwrap();
        let output = std::process::Command::new("node.exe")
            .arg(root.join("scripts/check-cookie-import.cjs"))
            .arg(&fixture)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Reference import failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
