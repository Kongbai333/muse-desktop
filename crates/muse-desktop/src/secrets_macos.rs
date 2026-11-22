//! Proxy credentials live in the login Keychain. Disk files contain references only.
use muse_core::AccountId;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

pub struct ProtectedStore {
    data_root: PathBuf,
}
impl ProtectedStore {
    pub fn new(root: &Path) -> Self {
        Self {
            data_root: root.to_path_buf(),
        }
    }
    fn service(&self) -> Result<String, String> {
        let root = std::fs::canonicalize(&self.data_root).map_err(|_| "无法定位凭据目录")?;
        Ok(format!("MuseDesktop.Proxy/{}", root.display()))
    }
    fn key(id: AccountId, revision: Option<uuid::Uuid>) -> String {
        match revision {
            Some(version) => format!("proxy-{id}-{version}"),
            None => format!("proxy-{id}"),
        }
    }
    fn path(&self, id: AccountId, revision: Option<uuid::Uuid>) -> PathBuf {
        self.data_root
            .join("secrets")
            .join(format!("{}.bin", Self::key(id, revision)))
    }
    pub fn save_proxy(&self, id: AccountId, value: &str) -> Result<(), String> {
        self.save_at(id, None, value)
    }
    pub fn save_revision(
        &self,
        id: AccountId,
        revision: uuid::Uuid,
        value: &str,
    ) -> Result<(), String> {
        self.save_at(id, Some(revision), value)
    }
    pub fn remove_revision(&self, id: AccountId, revision: uuid::Uuid) {
        if let Ok(service) = self.service() {
            let _ = delete_generic_password(&service, &Self::key(id, Some(revision)));
        }
        let _ = std::fs::remove_file(self.path(id, Some(revision)));
    }
    fn save_at(
        &self,
        id: AccountId,
        revision: Option<uuid::Uuid>,
        value: &str,
    ) -> Result<(), String> {
        muse_proxy::Upstream::parse(value).map_err(|error| error.to_string())?;
        let parent = self.data_root.join("secrets");
        std::fs::create_dir_all(&parent).map_err(|_| "无法创建凭据目录")?;
        let path = self.path(id, revision);
        if revision.is_some() && path.exists() {
            return Err("代理凭据版本已存在".into());
        }
        let service = self.service()?;
        let key = Self::key(id, revision);
        let temporary = parent.join(format!(".keychain-{}.tmp", uuid::Uuid::new_v4()));
        let marker = format!("MuseDesktop.Keychain.v1\n{key}\n");
        let stage = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(marker.as_bytes())?;
            file.sync_all()
        })();
        if stage.is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err("无法保存凭据引用".into());
        }
        let previous = get_generic_password(&service, &key)
            .ok()
            .map(Zeroizing::new);
        if set_generic_password(&service, &key, value.as_bytes()).is_err() {
            let _ = std::fs::remove_file(&temporary);
            return Err("macOS 钥匙串无法保存代理凭据".into());
        }
        if std::fs::rename(&temporary, &path).is_err() {
            if let Some(previous) = previous {
                let _ = set_generic_password(&service, &key, &previous);
            } else {
                let _ = delete_generic_password(&service, &key);
            }
            let _ = std::fs::remove_file(&temporary);
            return Err("无法完成凭据引用保存".into());
        }
        Ok(())
    }
    pub fn load_selected(
        &self,
        id: AccountId,
        revision: Option<uuid::Uuid>,
    ) -> Result<Zeroizing<String>, String> {
        let marker = std::fs::read_to_string(self.path(id, revision))
            .map_err(|_| "此账号的代理凭据引用不存在；未使用直连")?;
        let key = Self::key(id, revision);
        if marker != format!("MuseDesktop.Keychain.v1\n{key}\n") {
            return Err("代理凭据引用与此账号不一致".into());
        }
        let bytes = Zeroizing::new(
            get_generic_password(&self.service()?, &key)
                .map_err(|_| "macOS 钥匙串无法读取代理凭据；未使用直连")?,
        );
        let value =
            Zeroizing::new(String::from_utf8(bytes.to_vec()).map_err(|_| "代理凭据内容无效")?);
        muse_proxy::Upstream::parse(&value).map_err(|error| error.to_string())?;
        Ok(value)
    }
}
