use std::time::{Duration, Instant};

use gpui::{Context, FocusHandle, Subscription, Task, Window};

pub(super) struct Caret {
    pub(super) visible: bool,
    epoch: Instant,
    interval: Option<Duration>,
    timeout: Option<Duration>,
    blur: Option<Subscription>,
    timer: Option<Task<()>>,
}

impl Caret {
    pub(super) fn new() -> Self {
        Self {
            visible: true,
            epoch: Instant::now(),
            interval: None,
            timeout: None,
            blur: None,
            timer: None,
        }
    }

    pub(super) fn restart(&mut self) {
        self.epoch = Instant::now();
        self.visible = true;
    }

    fn stop(&mut self) {
        self.blur = None;
        self.timer = None;
        self.visible = true;
    }

    fn phase(&self) -> (bool, Option<Duration>) {
        let elapsed = self.epoch.elapsed();
        let Some(interval) = self.interval else {
            return (true, None);
        };
        if self.timeout.is_some_and(|timeout| elapsed >= timeout) {
            return (true, None);
        }
        let ticks = elapsed.as_nanos() / interval.as_nanos();
        let remaining = interval.as_nanos() - elapsed.as_nanos() % interval.as_nanos();
        let delay = Duration::from_nanos(remaining as u64);
        (
            ticks % 2 == 0,
            Some(
                self.timeout
                    .map_or(delay, |timeout| delay.min(timeout - elapsed)),
            ),
        )
    }

    pub(super) fn sync(
        &mut self,
        blink: bool,
        focus: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !blink {
            self.stop();
            return;
        }
        if self.blur.is_none() {
            self.restart();
            (self.interval, self.timeout) = settings();
            self.blur = Some(cx.on_blur(focus, window, |caret, _, _| caret.stop()));
        }
        if self.timer.is_some() {
            return;
        }
        let Some(mut delay) = self.phase().1 else {
            return;
        };
        self.timer = Some(cx.spawn(async move |caret, cx| {
            loop {
                cx.background_executor().timer(delay).await;
                let Ok(Some(next)) = caret.update(cx, |caret, cx| {
                    let (visible, next) = caret.phase();
                    if visible != caret.visible {
                        caret.visible = visible;
                        cx.notify();
                    }
                    if next.is_none() {
                        caret.timer = None;
                    }
                    next
                }) else {
                    break;
                };
                delay = next;
            }
        }));
    }
}

#[cfg(target_os = "windows")]
fn settings() -> (Option<Duration>, Option<Duration>) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCaretBlinkTime, SPI_GETCARETTIMEOUT, SystemParametersInfoW,
    };
    let duration = |value: u32| match value {
        0 | u32::MAX => None,
        millis => Some(Duration::from_millis(u64::from(millis))),
    };
    let interval = duration(unsafe { GetCaretBlinkTime() });
    let mut timeout = 0u32;
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETCARETTIMEOUT,
            0,
            Some((&raw mut timeout).cast()),
            Default::default(),
        )
    };
    (interval, result.ok().and_then(|()| duration(timeout)))
}

#[cfg(not(target_os = "windows"))]
fn settings() -> (Option<Duration>, Option<Duration>) {
    (None, None)
}
