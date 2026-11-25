use gpui::{Animation, AnimationExt, AnyElement, ElementId, IntoElement, SharedString, Styled, px};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const POPOVER_DURATION: Duration = Duration::from_millis(140);
const MODAL_DURATION: Duration = Duration::from_millis(220);
const LAYOUT_DURATION: Duration = Duration::from_millis(240);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionKind {
    Popover,
    Modal,
    Layout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionPhase {
    Entering,
    Visible,
    Exiting,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionValue {
    from: f32,
    to: f32,
    started: Instant,
    duration: Option<Duration>,
}

impl MotionValue {
    pub fn new(from: f32, to: f32, started: Instant, duration: Option<Duration>) -> Self {
        Self {
            from,
            to,
            started,
            duration,
        }
    }

    pub fn value_at(self, now: Instant) -> f32 {
        let Some(duration) = self.duration.filter(|duration| !duration.is_zero()) else {
            return self.to;
        };
        let elapsed = now.saturating_duration_since(self.started).as_secs_f32();
        let t = elapsed / duration.as_secs_f32();
        let amount = progress(MotionPhase::Entering, t);
        self.from + (self.to - self.from) * amount
    }

    pub fn retarget(&mut self, to: f32, now: Instant, duration: Option<Duration>) {
        if duration.is_some() && self.to == to {
            return;
        }
        self.from = self.value_at(now);
        self.to = to;
        self.started = now;
        self.duration = duration;
    }

    pub fn is_animating(self, now: Instant) -> bool {
        self.from != self.to
            && self
                .duration
                .is_some_and(|duration| now < self.started + duration)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionState {
    phase: MotionPhase,
    generation: u64,
    started: Instant,
    entry_started: Option<Instant>,
}

impl TransitionState {
    pub fn visible() -> Self {
        Self {
            phase: MotionPhase::Visible,
            generation: next_generation(),
            started: Instant::now(),
            entry_started: None,
        }
    }

    pub fn entering() -> Self {
        Self::entering_at(Instant::now())
    }

    fn entering_at(now: Instant) -> Self {
        Self {
            phase: MotionPhase::Entering,
            generation: next_generation(),
            started: now,
            entry_started: None,
        }
    }

    pub fn phase(self) -> MotionPhase {
        self.phase
    }

    pub fn is_exiting(self) -> bool {
        self.phase == MotionPhase::Exiting
    }

    pub fn generation(self) -> u64 {
        self.generation
    }

    pub fn begin_exit(&mut self) -> Option<u64> {
        self.begin_exit_at(Instant::now())
    }

    fn begin_exit_at(&mut self, now: Instant) -> Option<u64> {
        if self.is_exiting() {
            return None;
        }
        self.entry_started = (self.phase == MotionPhase::Entering).then_some(self.started);
        self.started = now;
        self.phase = MotionPhase::Exiting;
        self.generation = next_generation();
        Some(self.generation)
    }

    pub fn finish_exit(self, generation: u64) -> bool {
        self.is_exiting() && self.generation == generation
    }

    pub fn animation_id(self, name: &str) -> ElementId {
        SharedString::from(format!("{name}-{}", self.generation)).into()
    }

    fn opacity_at(self, now: Instant, duration: Duration) -> f32 {
        let initial = self.entry_started.map_or(1., |entry_started| {
            progress(
                MotionPhase::Entering,
                self.started
                    .saturating_duration_since(entry_started)
                    .as_secs_f32()
                    / duration.as_secs_f32(),
            )
        });
        initial
            * progress(
                self.phase,
                now.saturating_duration_since(self.started).as_secs_f32() / duration.as_secs_f32(),
            )
    }
}

fn next_generation() -> u64 {
    NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionPolicy {
    enabled: bool,
}

impl MotionPolicy {
    pub fn from_setting(client_area_animations: Option<bool>) -> Self {
        Self {
            enabled: client_area_animations.unwrap_or(true),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn system() -> Self {
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };
        use windows::core::BOOL;

        let mut enabled = BOOL(1);
        let result = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some((&mut enabled as *mut BOOL).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };
        Self::from_setting(result.is_ok().then(|| enabled.as_bool()))
    }

    pub fn duration(self, kind: MotionKind) -> Option<Duration> {
        self.enabled.then_some(match kind {
            MotionKind::Popover => POPOVER_DURATION,
            MotionKind::Modal => MODAL_DURATION,
            MotionKind::Layout => LAYOUT_DURATION,
        })
    }
}

pub fn progress(phase: MotionPhase, progress: f32) -> f32 {
    let t = progress.clamp(0., 1.);
    let eased = 1. - (1. - t).powi(3);
    match phase {
        MotionPhase::Entering => eased,
        MotionPhase::Visible => 1.,
        MotionPhase::Exiting => 1. - eased,
    }
}

pub fn fade<E>(element: E, state: TransitionState, policy: MotionPolicy) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_part(
        element,
        state,
        policy,
        MotionKind::Modal,
        "motion-backdrop",
        |element, _| element,
    )
}

pub fn animate<E>(
    element: E,
    state: TransitionState,
    policy: MotionPolicy,
    kind: MotionKind,
    offset: f32,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_part(
        element,
        state,
        policy,
        kind,
        "motion-content",
        move |element, amount| element.mt(px(offset * (1. - amount))),
    )
}

pub fn animate_above<E>(
    element: E,
    state: TransitionState,
    policy: MotionPolicy,
    offset: f32,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_part(
        element,
        state,
        policy,
        MotionKind::Popover,
        "motion-content",
        move |element, amount| element.mb(px(-offset * (1. - amount))),
    )
}

/// Fades a side sheet in while it slides in from the right edge.
pub fn slide_from_right<E>(
    element: E,
    state: TransitionState,
    policy: MotionPolicy,
    offset: f32,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    animate_part(
        element,
        state,
        policy,
        MotionKind::Modal,
        "motion-drawer",
        move |element, amount| element.ml(px(offset * (1. - amount))),
    )
}

fn animate_part<E>(
    element: E,
    state: TransitionState,
    policy: MotionPolicy,
    kind: MotionKind,
    name: &str,
    position: impl Fn(E, f32) -> E + 'static,
) -> AnyElement
where
    E: IntoElement + Styled + 'static,
{
    let Some(duration) = policy.duration(match state.phase() {
        MotionPhase::Entering | MotionPhase::Exiting => kind,
        MotionPhase::Visible => return element.into_any_element(),
    }) else {
        return element.into_any_element();
    };

    element
        .with_animation(
            state.animation_id(name),
            Animation::new(duration),
            move |element, _| {
                let amount = state.opacity_at(Instant::now(), duration);
                position(element.opacity(amount), amount)
            },
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn entry_and_exit_progress_follow_the_same_eased_curve_in_reverse() {
        assert_eq!(progress(MotionPhase::Entering, 0.), 0.);
        assert_eq!(progress(MotionPhase::Entering, 0.5), 0.875);
        assert_eq!(progress(MotionPhase::Entering, 1.), 1.);

        assert_eq!(progress(MotionPhase::Exiting, 0.), 1.);
        assert_eq!(progress(MotionPhase::Exiting, 0.5), 0.125);
        assert_eq!(progress(MotionPhase::Exiting, 1.), 0.);
        assert_eq!(progress(MotionPhase::Visible, 0.5), 1.);
    }

    #[test]
    fn exit_generation_rejects_duplicate_close_and_stale_completion() {
        let mut transition = TransitionState::entering();
        let old_generation = transition.generation();
        let entering_id = transition.animation_id("account-dialog");
        let closing_generation = transition.begin_exit().unwrap();
        let exiting_id = transition.animation_id("account-dialog");

        assert!(closing_generation > old_generation);
        assert_ne!(entering_id, exiting_id);
        assert_eq!(transition.begin_exit(), None);
        assert!(!transition.finish_exit(old_generation));
        assert!(transition.finish_exit(closing_generation));
    }

    #[test]
    fn reopening_an_overlay_invalidates_its_pending_close_generation() {
        let mut closing = TransitionState::entering();
        let stale_generation = closing.begin_exit().unwrap();

        let reopened = TransitionState::entering();

        assert!(!reopened.finish_exit(stale_generation));
        assert_eq!(reopened.phase(), MotionPhase::Entering);
    }

    #[test]
    fn closing_during_entrance_fades_from_the_current_opacity() {
        let start = Instant::now();
        let duration = Duration::from_millis(200);
        let close_at = start + Duration::from_millis(50);
        let mut transition = TransitionState::entering_at(start);
        assert_eq!(transition.opacity_at(close_at, duration), 0.578125);

        transition.begin_exit_at(close_at);

        assert_eq!(transition.opacity_at(close_at, duration), 0.578125);
        assert_eq!(transition.opacity_at(close_at + duration, duration), 0.);
    }

    #[test]
    fn enabled_motion_uses_bounded_durations_and_disabled_motion_is_absent() {
        let enabled = MotionPolicy::from_setting(Some(true));
        assert_eq!(
            enabled.duration(MotionKind::Popover),
            Some(Duration::from_millis(140))
        );
        assert_eq!(
            enabled.duration(MotionKind::Modal),
            Some(Duration::from_millis(220))
        );
        assert_eq!(
            enabled.duration(MotionKind::Layout),
            Some(Duration::from_millis(240))
        );

        let disabled = MotionPolicy::from_setting(Some(false));
        for kind in [MotionKind::Popover, MotionKind::Modal, MotionKind::Layout] {
            assert_eq!(disabled.duration(kind), None);
        }
        assert_eq!(
            MotionPolicy::from_setting(None).duration(MotionKind::Modal),
            Some(MODAL_DURATION)
        );
    }

    #[test]
    fn out_of_range_animation_progress_stays_within_visible_opacity_bounds() {
        assert_eq!(progress(MotionPhase::Entering, -1.), 0.);
        assert_eq!(progress(MotionPhase::Entering, 2.), 1.);
        assert_eq!(progress(MotionPhase::Exiting, -1.), 1.);
        assert_eq!(progress(MotionPhase::Exiting, 2.), 0.);
    }

    #[test]
    fn retargeting_panel_motion_starts_from_its_current_interpolated_value() {
        let start = Instant::now();
        let mut value = MotionValue::new(0., 100., start, Some(Duration::from_millis(200)));
        let midpoint = start + Duration::from_millis(100);
        assert_eq!(value.value_at(midpoint), 87.5);

        value.retarget(50., midpoint, Some(Duration::from_millis(100)));
        assert_eq!(value.value_at(midpoint), 87.5);
        assert_eq!(value.value_at(midpoint + Duration::from_millis(100)), 50.);
    }

    #[test]
    fn disabled_motion_jumps_directly_to_its_target() {
        let start = Instant::now();
        let value = MotionValue::new(0., 100., start, None);
        assert_eq!(value.value_at(start), 100.);
    }
}
