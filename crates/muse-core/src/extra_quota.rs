//! The invitation/topup pool is independent of subscription usage.

#[derive(Clone, Debug, PartialEq)]
pub struct ExtraQuotaObservation {
    remaining: Option<f64>,
    total: Option<f64>,
    display_value: Option<String>,
    observed_at: u64,
}
impl ExtraQuotaObservation {
    pub fn new(
        remaining: Option<f64>,
        total: Option<f64>,
        display: Option<&str>,
        observed_at: u64,
    ) -> crate::Result<Self> {
        if remaining.is_some_and(|value| !value.is_finite() || value < 0.)
            || total.is_some_and(|value| !value.is_finite() || value < 0.)
            || remaining
                .zip(total)
                .is_some_and(|(remaining, total)| remaining > total)
        {
            return Err("额外额度数值无效".into());
        }
        let display_value = display
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if display_value
            .as_ref()
            .is_some_and(|value| value.chars().count() > 96 || value.chars().any(char::is_control))
            || (remaining.is_none() && display_value.is_none())
        {
            return Err("未读取到有效的额外额度".into());
        }
        Ok(Self {
            remaining,
            total,
            display_value,
            observed_at,
        })
    }
    pub fn remaining(&self) -> Option<f64> {
        self.remaining
    }
    pub fn total(&self) -> Option<f64> {
        self.total
    }
    pub fn observed_at(&self) -> u64 {
        self.observed_at
    }
    pub fn label(&self) -> String {
        if let Some(value) = &self.display_value {
            return value.clone();
        }
        match (self.remaining, self.total) {
            (Some(remaining), Some(total)) => format!("剩余 {remaining} / {total}"),
            (Some(remaining), None) => format!("剩余 {remaining}"),
            _ => "尚未读取额外额度".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum ExtraQuotaState {
    #[default]
    Unknown,
    Known(ExtraQuotaObservation),
    Stale(ExtraQuotaObservation),
}
impl ExtraQuotaState {
    pub fn observed_at(&self) -> Option<u64> {
        match self {
            Self::Known(v) | Self::Stale(v) => Some(v.observed_at()),
            _ => None,
        }
    }
    pub fn at_time(self, now: u64) -> Self {
        match self {
            Self::Known(value)
                if value.observed_at > now.saturating_add(30_000)
                    || now.saturating_sub(value.observed_at) >= 300_000 =>
            {
                Self::Stale(value)
            }
            state => state,
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Unknown => "尚未读取额外额度".into(),
            Self::Known(v) => v.label(),
            Self::Stale(v) => format!("{} · 未更新", v.label()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_zero_is_preserved_and_unknown_does_not_become_zero() {
        let value = ExtraQuotaObservation::new(Some(0.), Some(200.), None, 1000).unwrap();
        assert_eq!(value.remaining(), Some(0.));
        assert_eq!(value.label(), "剩余 0 / 200");
        assert_eq!(ExtraQuotaState::Unknown.label(), "尚未读取额外额度");
        assert!(ExtraQuotaObservation::new(None, None, None, 1000).is_err());
    }
    #[test]
    fn site_display_units_and_balance_without_total_are_retained() {
        let value =
            ExtraQuotaObservation::new(Some(600.), Some(1000.), Some("剩余 600 积分"), 1000)
                .unwrap();
        assert_eq!(value.label(), "剩余 600 积分");
        assert_eq!(
            ExtraQuotaObservation::new(Some(400.), None, None, 1000)
                .unwrap()
                .label(),
            "剩余 400"
        );
        assert_eq!(
            ExtraQuotaObservation::new(None, None, Some("已使用 12%"), 1000)
                .unwrap()
                .label(),
            "已使用 12%"
        );
    }
    #[test]
    fn invalid_pools_are_rejected_and_old_snapshots_expire() {
        for (remaining, total) in [
            (Some(-1.), None),
            (Some(f64::NAN), None),
            (Some(101.), Some(100.)),
            (None, Some(-1.)),
        ] {
            assert!(ExtraQuotaObservation::new(remaining, total, None, 1000).is_err());
        }
        let state = ExtraQuotaState::Known(
            ExtraQuotaObservation::new(Some(5.), Some(10.), None, 1000).unwrap(),
        );
        assert!(matches!(
            state.clone().at_time(2000),
            ExtraQuotaState::Known(_)
        ));
        assert!(matches!(state.at_time(301000), ExtraQuotaState::Stale(_)));
    }
}
