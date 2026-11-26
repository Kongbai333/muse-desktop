use muse_core::AccountId;
use std::path::{Path, PathBuf};
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::PCWSTR,
};
use zeroize::Zeroizing;

pub struct ProtectedStore {
    root: PathBuf,
}
impl ProtectedStore {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.join("secrets"),
        }
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
    fn path(&self, id: AccountId, revision: Option<uuid::Uuid>) -> PathBuf {
        match revision {
            Some(revision) => self.root.join(format!("proxy-{id}-{revision}.bin")),
            None => self.root.join(format!("proxy-{id}.bin")),
        }
    }
    pub fn remove_revision(&self, id: AccountId, revision: uuid::Uuid) {
        let _ = std::fs::remove_file(self.path(id, Some(revision)));
    }
    fn save_at(
        &self,
        id: AccountId,
        revision: Option<uuid::Uuid>,
        value: &str,
    ) -> Result<(), String> {
        muse_proxy::Upstream::parse(value).map_err(|e| e.to_string())?;
        self.save_protected(id, revision, value)
    }
    pub fn save_protected(
        &self,
        id: AccountId,
        revision: Option<uuid::Uuid>,
        value: &str,
    ) -> Result<(), String> {
        let encrypted = protect(id, revision, value.as_bytes(), true)?;
        std::fs::create_dir_all(&self.root).map_err(|_| "无法创建代理凭据目录")?;
        let temporary = self
            .root
            .join(format!(".proxy-{}.tmp", uuid::Uuid::new_v4()));
        let destination = self.path(id, revision);
        if revision.is_some() && destination.exists() {
            return Err("代理凭据版本已存在".into());
        }
        use std::io::Write;
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(&encrypted)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, destination)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result.map_err(|_| "代理凭据保存失败".into())
    }
    pub fn load_selected(
        &self,
        id: AccountId,
        revision: Option<uuid::Uuid>,
    ) -> Result<Zeroizing<String>, String> {
        let value = self.load_protected(id, revision)?;
        muse_proxy::Upstream::parse(&value).map_err(|e| e.to_string())?;
        Ok(value)
    }
    pub fn load_protected(
        &self,
        id: AccountId,
        revision: Option<uuid::Uuid>,
    ) -> Result<Zeroizing<String>, String> {
        let path = self.path(id, revision);
        if std::fs::metadata(&path)
            .map_err(|_| "此账号的受保护代理配置不存在；未使用直连")?
            .len()
            > 1024 * 1024
        {
            return Err("代理凭据文件无效".into());
        }
        let bytes = std::fs::read(path).map_err(|_| "此账号的受保护代理配置不存在；未使用直连")?;
        if bytes.len() > 1024 * 1024 {
            return Err("代理凭据文件无效".into());
        }
        let plain = protect(id, revision, &bytes, false)?;
        let value = String::from_utf8(plain.to_vec()).map_err(|_| "代理凭据内容无效")?;
        Ok(Zeroizing::new(value))
    }
}

fn protect(
    id: AccountId,
    revision: Option<uuid::Uuid>,
    input: &[u8],
    encrypt: bool,
) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut owned_input = Zeroizing::new(input.to_vec());
    let entropy = match revision {
        Some(revision) => format!("MuseDesktop.Proxy.v2/{id}/{revision}"),
        None => format!("MuseDesktop.Proxy.v1/{id}"),
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: owned_input.len().try_into().map_err(|_| "代理凭据过大")?,
        pbData: owned_input.as_mut_ptr(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let result = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    result.map_err(|_| "Windows 无法解密或保护此账号的代理凭据；未使用直连")?;
    if output.cbData == 0 || output.pbData.is_null() {
        if !output.pbData.is_null() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
            }
        }
        return Err("受保护代理凭据为空".into());
    }
    let bytes = unsafe {
        Zeroizing::new(std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec())
    };
    unsafe {
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proxy_credentials_are_protected_and_roundtrip_for_the_same_account() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/secret-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        let store = ProtectedStore::new(&root);
        let account = uuid::Uuid::new_v4();
        let sensitive = format!("http://user:{}@127.0.0.1:8080", uuid::Uuid::new_v4());
        store.save_proxy(account, &sensitive).unwrap();
        assert_eq!(
            store.load_selected(account, None).unwrap().as_str(),
            sensitive
        );
        let file = root.join("secrets").join(format!("proxy-{account}.bin"));
        let encrypted = std::fs::read(&file).unwrap();
        assert!(
            !encrypted
                .windows(sensitive.len())
                .any(|part| part == sensitive.as_bytes())
        );
        let other = uuid::Uuid::new_v4();
        std::fs::copy(
            file,
            root.join("secrets").join(format!("proxy-{other}.bin")),
        )
        .unwrap();
        assert!(store.load_selected(other, None).is_err());
    }
}
