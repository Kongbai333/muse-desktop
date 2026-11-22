//! Request generation and cancellation/commit ownership for asynchronous migration jobs.
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct JobGate {
    pub id: uuid::Uuid,
    state: Arc<AtomicU8>,
}
impl JobGate {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            state: Arc::new(AtomicU8::new(0)),
        }
    }
    pub fn accepts(&self, id: uuid::Uuid) -> bool {
        self.id == id && self.state.load(Ordering::Acquire) == 0
    }
    pub fn cancel(&self) -> bool {
        self.state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn begin_commit(&self) -> bool {
        self.state
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
    pub fn committing(&self) -> bool {
        self.state.load(Ordering::Acquire) == 2
    }
    pub fn finish(&self) {
        self.state.store(3, Ordering::Release);
    }
}
pub fn confirmed_password(
    value: String,
    confirmation: &str,
    export: bool,
) -> Result<Zeroizing<String>, &'static str> {
    let value = Zeroizing::new(value);
    if !(8..=1024).contains(&value.len()) {
        return Err(crate::account_transfer::TransferError::Password.message());
    }
    if export && value.as_str() != confirmation {
        return Err("两次迁移密码不一致，请保留完全相同的字符、空格和换行");
    }
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_or_old_jobs_cannot_own_the_commit_point() {
        let gate = JobGate::new();
        let old = JobGate::new();
        assert!(!gate.accepts(old.id));
        assert!(gate.accepts(gate.id));
        assert!(gate.cancel());
        assert!(!gate.accepts(gate.id));
        assert!(!gate.begin_commit());
    }
    #[test]
    fn commit_point_has_one_owner_and_cannot_be_cancelled_or_reentered() {
        let gate = JobGate::new();
        assert!(gate.begin_commit());
        assert!(gate.committing());
        assert!(!gate.cancel());
        assert!(!gate.begin_commit());
        gate.finish();
        assert!(!gate.committing());
        assert!(!gate.accepts(gate.id));
    }
    #[test]
    fn confirmation_compares_exact_utf8_and_paste_never_rewrites_passwords() {
        let value = " 合成🔑\n原字符 ";
        assert!(confirmed_password(value.to_owned(), value, true).is_ok());
        assert!(confirmed_password(value.to_owned(), value.trim(), true).is_err());
        assert_eq!(crate::input_mask::paste_text(value, true), value);
        assert_eq!(
            crate::input_mask::paste_text("ordinary\ninput", false),
            "ordinary input"
        );
    }
}
