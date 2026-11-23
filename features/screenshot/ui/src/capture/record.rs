//! The recording's side of the capture overlay: the picker in video mode
//! (`nova record` opens and closes it), the toolbar's sound, the switch to
//! a screenshot and back, the count before a recording starts, and what is
//! being recorded. `ui/capture/service/RecordingService.qml` in today's
//! shell.
//!
//! The recording picker is over the live screen. With screenshots on, the
//! screens are frozen first and the frames held behind it, so the
//! toolbar's switch to a screenshot shows them at once instead of freezing
//! again.

use super::{Capture, Saved};
use crate::tokens::{COUNTDOWN_STEP, PICKER_GONE_DELAY, RECORD_COUNTDOWN};
use gpui::{Context, Subscription, Task};
use nova_recording_api::{ActiveRecording, Command, Event, LayoutRect, RecordAudio, TOPIC};
use nova_screenshot_api::{CaptureMode, Command as ScreenshotCommand};
use nova_ui_client::{Daemon, Refusal};

pub(super) struct Record {
    /// The recording picker shows on every screen.
    pub picking: bool,
    /// Freezing the screens before it shows.
    pub opening: bool,
    /// The screenshot's frames are kept behind it.
    pub held: bool,
    /// It was opened by the toolbar's switch.
    pub switched: bool,
    /// The sound to record, kept from one recording to the next.
    pub desktop_audio: bool,
    pub mic_audio: bool,
    /// Counting down to the start: what's left, 0 when not counting.
    pub countdown: u32,
    /// What novad records, while it does.
    pub active: Option<ActiveRecording>,
    counting: Option<Task<()>>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            picking: false,
            opening: false,
            held: false,
            switched: false,
            desktop_audio: true,
            mic_audio: false,
            countdown: 0,
            active: None,
            counting: None,
        }
    }
}

/// Follows novad's recording events for the capture.
pub(super) fn follow(cx: &mut Context<Capture>) -> Subscription {
    let this = cx.weak_entity();
    Daemon::on_event(TOPIC, cx, move |event: Event, cx| {
        let _ = this.update(cx, |capture, cx| capture.take_recording(event, cx));
    })
}

impl Capture {
    fn take_recording(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::PickRecording => {
                if !self.ready.recording || self.record.active.is_some() {
                    return;
                }
                if self.record.picking || self.record.opening {
                    self.cancel_recording_pick(cx);
                } else {
                    self.open_recording_pick(cx);
                }
            }
            Event::Recording(active) => {
                if active != self.record.active {
                    self.record.active = active;
                    cx.notify();
                }
            }
            Event::RecordingSaved(saved) => self.show_card(Saved::Recording(saved), cx),
            Event::RecordingFailed { reason } => self.show_card(Saved::Failed { reason, started: true }, cx),
        }
    }

    /// Opens the recording picker, the screens frozen first when
    /// screenshots are on.
    fn open_recording_pick(&mut self, cx: &mut Context<Self>) {
        if !self.ready.screenshot {
            self.show_recording_pick(cx);
            return;
        }
        self.record.opening = true;
        self.record.held = true;
        if self.frames.is_some() {
            // The open screenshot picker's frames, held as they are: asking
            // novad again would close them.
            self.frames_held(true, cx);
            return;
        }
        let answer = Daemon::call::<()>(ScreenshotCommand::Screenshot { mode: CaptureMode::Region }, cx);
        cx.spawn(async move |capture, cx| {
            if let Err(error) = answer.await {
                eprintln!("WARN nova-ui: the screens weren't frozen for the recording picker: {error}");
                let _ = capture.update(cx, |capture, cx| {
                    capture.frames_held(false, cx);
                    capture.record.held = false;
                });
            }
        })
        .detach();
    }

    /// The frames frozen for the recording picker are here (`got`), or
    /// went before they came.
    pub(super) fn frames_held(&mut self, got: bool, cx: &mut Context<Self>) {
        if self.record.opening {
            self.record.opening = false;
            if !got {
                self.record.held = false;
                eprintln!("WARN nova-ui: recording without a frozen frame to switch to");
            }
            self.show_recording_pick(cx);
        } else if got && !self.record.picking {
            // Closed again (`nova record` twice) while freezing.
            Daemon::send(ScreenshotCommand::CancelCapture, cx);
        }
    }

    fn show_recording_pick(&mut self, cx: &mut Context<Self>) {
        self.record.picking = true;
        self.mode = CaptureMode::Region;
        eprintln!("INFO nova-ui: recording picker shown");
        cx.notify();
    }

    /// Closes the recording picker: a count that was going is cancelled
    /// with it, and the frames held for the switch go too.
    pub(super) fn cancel_recording_pick(&mut self, cx: &mut Context<Self>) {
        self.record.opening = false;
        if self.record.picking {
            self.hide_recording_pick(cx);
        }
    }

    fn hide_recording_pick(&mut self, cx: &mut Context<Self>) {
        self.record.picking = false;
        self.record.switched = false;
        self.record.countdown = 0;
        self.record.counting = None;
        self.mode = CaptureMode::Region;
        if self.record.held {
            Daemon::send(ScreenshotCommand::CancelCapture, cx);
        }
        eprintln!("INFO nova-ui: recording picker hidden");
        cx.notify();
    }

    /// The card's Try again: the picker once more, with the same sound,
    /// unless it's already open or something is recording.
    pub(super) fn record_again(&mut self, cx: &mut Context<Self>) {
        let busy = self.record.active.is_some() || self.record.picking || self.record.opening;
        if self.ready.recording && !busy {
            self.open_recording_pick(cx);
        }
    }

    /// The toolbar's switch: to the recording picker (`video`), keeping the
    /// screenshot's frames for switching back, or to the screenshot's, the
    /// held frames' at once or a new freeze once this picker has gone.
    pub fn switch_kind(&mut self, video: bool, cx: &mut Context<Self>) {
        if video {
            if !self.ready.recording || self.record.active.is_some() || self.record.picking {
                return;
            }
            self.record.switched = true;
            self.record.held = true;
            self.record.picking = true;
            self.switched = false;
            eprintln!("INFO nova-ui: recording picker shown");
            cx.notify();
            return;
        }
        if !self.record.picking || self.record.countdown > 0 {
            return;
        }
        let mode = self.mode;
        if self.record.held && self.frames.is_some() {
            self.record.held = false;
            self.hide_recording_pick(cx);
            self.switched = true;
            self.mode = mode;
            eprintln!("INFO nova-ui: region picker shown");
            return;
        }
        self.hide_recording_pick(cx);
        // Once what was on the screen has gone, so novad's frozen frames
        // don't show it.
        cx.spawn(async move |capture, cx| {
            cx.background_executor().timer(PICKER_GONE_DELAY).await;
            // `screenshot` closes a pick that opened meanwhile: none may
            // have.
            let Ok(Some(answer)) = capture.update(cx, |capture, cx| {
                let opened = capture.record.picking || capture.record.opening || capture.record.held;
                if opened || capture.frames.is_some() {
                    return None;
                }
                capture.next_mode = Some(mode);
                Some(Daemon::call::<()>(ScreenshotCommand::Screenshot { mode: CaptureMode::Region }, cx))
            }) else {
                return;
            };
            if let Err(error) = answer.await {
                eprintln!("WARN nova-ui: the screenshot picker didn't open: {error}");
                let _ = capture.update(cx, |capture, _| capture.next_mode = None);
            }
        })
        .detach();
    }

    /// Counting down to a recording.
    pub fn counting(&self) -> bool {
        self.record.countdown > 0
    }

    pub fn toggle_desktop_audio(&mut self, cx: &mut Context<Self>) {
        self.record.desktop_audio = !self.record.desktop_audio;
        cx.notify();
    }

    pub fn toggle_mic_audio(&mut self, cx: &mut Context<Self>) {
        self.record.mic_audio = !self.record.mic_audio;
        cx.notify();
    }

    /// Counts down, then starts recording `output`, or `region` of the
    /// layout on it.
    pub fn record(&mut self, output: String, region: Option<LayoutRect>, cx: &mut Context<Self>) {
        if self.record.countdown > 0 || !self.record.picking {
            return;
        }
        self.record.countdown = RECORD_COUNTDOWN;
        cx.notify();
        self.record.counting = Some(cx.spawn(async move |capture, cx| {
            loop {
                cx.background_executor().timer(COUNTDOWN_STEP).await;
                let done = capture.update(cx, |capture, cx| {
                    capture.record.countdown = capture.record.countdown.saturating_sub(1);
                    cx.notify();
                    capture.record.countdown == 0
                });
                if done.unwrap_or(true) {
                    break;
                }
            }
            let _ = capture.update(cx, |capture, cx| {
                if capture.record.picking {
                    capture.begin(output, region, cx);
                }
            });
        }));
    }

    /// The count is over: the picker goes first, so it isn't in the first
    /// frames, and novad starts recording a moment after.
    fn begin(&mut self, output: String, region: Option<LayoutRect>, cx: &mut Context<Self>) {
        // This runs inside the count's own task: it ends by itself.
        if let Some(counting) = self.record.counting.take() {
            counting.detach();
        }
        let audio = RecordAudio { desktop: self.record.desktop_audio, mic: self.record.mic_audio };
        self.hide_recording_pick(cx);
        cx.spawn(async move |capture, cx| {
            cx.background_executor().timer(PICKER_GONE_DELAY).await;
            let Ok(answer) =
                capture.update(cx, |_, cx| Daemon::call::<()>(Command::StartRecording { output, region, audio }, cx))
            else {
                return;
            };
            if let Err(refusal) = answer.await {
                let reason = match refusal {
                    Refusal::Daemon(reason) => reason,
                    other => other.to_string(),
                };
                eprintln!("WARN nova-ui: recording didn't start: {reason}");
                let _ = capture.update(cx, |capture, cx| capture.show_card(Saved::Failed { reason, started: false }, cx));
            }
        })
        .detach();
    }
}
