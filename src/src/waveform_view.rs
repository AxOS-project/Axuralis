// SPDX-FileCopyrightText: 2025 Ardox
// SPDX-License-Identifier: GPL-3.0-or-later

// Based on gnome-sound-recorder/src/waveform.js:
// - Copyright 2013 Meg Ford
// - Copyright 2022 Kavan Mevada
// Released under the terms of the LGPL 2.0 or later

#![allow(deprecated)]

use std::{
    cell::{Cell, RefCell},
    ops::DivAssign,
};

use adw::subclass::prelude::*;
use glib::clone;
use gtk::{gdk, glib, graphene, prelude::*};
use log::{debug, warn};

#[derive(Debug, PartialEq)]
pub struct PeakPair {
    pub left: f64,
    pub right: f64,
}

impl PeakPair {
    pub fn new(left: f64, right: f64) -> Self {
        Self { left, right }
    }
}

impl DivAssign<f64> for PeakPair {
    fn div_assign(&mut self, rhs: f64) {
        self.left /= rhs;
        self.right /= rhs;
    }
}

mod imp {
    use glib::{subclass::Signal, ParamSpec, ParamSpecDouble, Value};
    use once_cell::sync::Lazy;

    use super::*;

    #[derive(Debug, Default)]
    pub struct WaveformView {
        pub position: Cell<f64>,
        pub hover_position: Cell<Option<f64>>,
        // left and right channel peaks, normalised between 0 and 1
        pub peaks: RefCell<Option<Vec<PeakPair>>>,
        pub next_peaks: RefCell<Option<Vec<PeakPair>>>,
        pub tick_id: RefCell<Option<gtk::TickCallbackId>>,
        pub first_frame_time: Cell<Option<i64>>,
        pub factor: Cell<Option<f64>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WaveformView {
        const NAME: &'static str = "AxuralisWaveformView";
        type Type = super::WaveformView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("waveformview");
            klass.set_accessible_role(gtk::AccessibleRole::Slider);
        }
    }

    impl ObjectImpl for WaveformView {
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![ParamSpecDouble::builder("position")
                    .minimum(0.0)
                    .maximum(1.0)
                    .default_value(0.0)
                    .build()]
            });

            PROPERTIES.as_ref()
        }

        fn set_property(&self, _id: usize, value: &Value, pspec: &ParamSpec) {
            match pspec.name() {
                "position" => self.position.replace(value.get::<f64>().unwrap()),
                _ => unimplemented!(),
            };
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "position" => self.position.get().to_value(),
                _ => unimplemented!(),
            }
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: Lazy<Vec<Signal>> = Lazy::new(|| {
                vec![Signal::builder("position-changed")
                    .param_types([f64::static_type()])
                    .build()]
            });

            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.obj().set_focusable(true);

            self.obj().setup_gesture();

            self.obj()
                .upcast_ref::<gtk::Accessible>()
                .update_property(&[
                    (gtk::accessible::Property::ValueMin(0.0)),
                    (gtk::accessible::Property::ValueMax(1.0)),
                    (gtk::accessible::Property::ValueNow(0.0)),
                ]);
        }

        fn dispose(&self) {
            if let Some(tick_id) = self.tick_id.replace(None) {
                tick_id.remove();
            }
        }
    }

    impl WidgetImpl for WaveformView {
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            debug!("WaveformView::focus({:?})", direction);
            let widget = self.obj();
            if !widget.is_focus() {
                widget.grab_focus();
                return true;
            }

            let pos = self.position.get();

            match direction {
                gtk::DirectionType::Left if pos == 0.0 => false,
                gtk::DirectionType::Left if pos > 0.0 => true,
                gtk::DirectionType::Right if pos < 1.0 => true,
                gtk::DirectionType::Right if pos == 1.0 => false,
                _ => false,
            }
        }

        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::ConstantSize
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => {
                    // We ask for as many samples we can fit within a 256 pixels wide area
                    if let Some(ref peaks) = *self.peaks.borrow() {
                        let n_peaks = peaks.len() as i32;
                        let width = i32::min(n_peaks * 4, 256);

                        (width, width, -1, -1)
                    } else {
                        (256, 256, -1, -1)
                    }
                }
                gtk::Orientation::Vertical => (48, 48, -1, -1),
                _ => (0, 0, -1, -1),
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let w = widget.width();
            let h = widget.height();
            if w == 0 || h == 0 {
                return;
            }

            // Our reference line
            let center_y = h as f32 / 2.0;

            // Grab the colors
            let hc = adw::StyleManager::default().is_high_contrast();

            let style_context = widget.style_context();
            let color = style_context.color();
            let empty_opacity = if hc { 0.4 } else { 0.2 };
            let hover_opacity = if hc { 0.7 } else { 0.45 };

            let empty_color = gdk::RGBA::new(
                color.red(),
                color.green(),
                color.blue(),
                color.alpha() * empty_opacity,
            );

            let is_rtl = widget.direction() == gtk::TextDirection::Rtl;
            let bar_size = 2;
            let space_size = 2;
            let block_size = bar_size + space_size;
            let available_width = w;

            if let Some(ref peaks) = *self.peaks.borrow() {
                let n_peaks = peaks.len() as i32;
                let waveform_width = w as f64;

                // We have two cursors:
                //
                // 1. the state position, updated by the player
                // 2. the hover position, updated by the motion controller
                //
                // The hover position may be behind the state position, if we are
                // scrubbing backwards; or after the state position, if we are
                // scrubbing forward.
                //
                // The area between the state position and the hover position is
                // meant to be shown as a dimmed cursor color; the area between
                // the start of the waveform and the state position is meant to be
                // shown as a full cursor color; and the area between the hover
                // position and the end of the waveform is meant to be shown as a
                // current foreground color.
                let position = if is_rtl {
                    1.0 - self.position.get()
                } else {
                    self.position.get()
                };
                let mut cursor_pos: [f64; 2] =
                    [position * waveform_width, position * waveform_width];
                if let Some(hover) = self.hover_position.get() {
                    if is_rtl {
                        if hover <= position {
                            cursor_pos[1] = hover * waveform_width;
                        } else {
                            cursor_pos[0] = hover * waveform_width;
                        }
                    } else if hover <= position {
                        cursor_pos[0] = hover * waveform_width;
                    } else {
                        cursor_pos[1] = hover * waveform_width;
                    }
                }

                // If the number of samples is too big to fit into the available
                // width, we average the samples that fit within a bar
                let pixels_per_sample = if available_width > n_peaks * block_size {
                    block_size as f64
                } else {
                    (available_width as f64 / 2.0) / n_peaks as f64
                };

                let mut current_pixel = 0.0;
                let mut samples_in_accum = 0;
                let mut accum = PeakPair::new(0.0, 0.0);
                let mut offset = if is_rtl { waveform_width } else { 0.0 };

                for (i, sample) in peaks.iter().enumerate() {
                    current_pixel += pixels_per_sample;
                    samples_in_accum += 1;
                    accum.left += sample.left;
                    accum.right += sample.right;
                    if current_pixel > bar_size as f64 || i == peaks.len() - 1 {
                        accum /= samples_in_accum as f64;

                        // Scale by half: left goes in the upper half of the
                        // available space, and right goes in the lower half
                        let mut left = accum.left / 2.0;
                        let mut right = accum.right / 2.0;

                        // We optionally apply the scaling factor computed
                        // during the animation
                        if let Some(factor) = self.factor.get() {
                            left *= factor.clamp(0.0, 1.0);
                            right *= factor.clamp(0.0, 1.0);
                        }

                        // The block rectangle, clamped to avoid overdrawing
                        let block = {
                            let x = if is_rtl {
                                offset as f32 - bar_size as f32
                            } else {
                                offset as f32
                            };
                            let y =
                                f32::clamp(center_y - right as f32 * h as f32, 0.0, h as f32 / 2.0);
                            let width: f32 = 2.0;
                            let height = f32::clamp(
                                right as f32 * h as f32 + left as f32 * h as f32,
                                2.0,
                                h as f32,
                            );

                            graphene::Rect::new(x, y, width, height)
                        };

                        if is_rtl {
                            if offset > cursor_pos[0] {
                                snapshot.append_color(&color, &block);
                            } else if offset > cursor_pos[1] {
                                let hover_color = gdk::RGBA::new(
                                    color.red(),
                                    color.green(),
                                    color.blue(),
                                    color.alpha() * hover_opacity,
                                );
                                snapshot.append_color(&hover_color, &block);
                            } else {
                                snapshot.append_color(&empty_color, &block);
                            }
                        } else if offset < cursor_pos[0] {
                            snapshot.append_color(&color, &block);
                        } else if offset < cursor_pos[1] {
                            let hover_color = gdk::RGBA::new(
                                color.red(),
                                color.green(),
                                color.blue(),
                                color.alpha() * hover_opacity,
                            );
                            snapshot.append_color(&hover_color, &block);
                        } else {
                            snapshot.append_color(&empty_color, &block);
                        }

                        accum.left = 0.0;
                        accum.right = 0.0;
                        samples_in_accum = 0;
                        current_pixel -= bar_size as f64;

                        if is_rtl {
                            offset -= block_size as f64;
                        } else {
                            offset += block_size as f64;
                        }
                    }
                }
            } else {
                let mut offset = space_size;
                while offset < w - space_size {
                    let x = offset as f32;
                    let y = center_y - 1.0;
                    let width = bar_size as f32;
                    let height: f32 = 2.0;
                    snapshot.append_color(&color, &graphene::Rect::new(x, y, width, height));

                    offset += block_size;
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct WaveformView(ObjectSubclass<imp::WaveformView>)
        @extends gtk::Widget,
        @implements gtk::Accessible;
}

fn ease_out_cubic(t: f64) -> f64 {
    let p = t - 1.0;
    p * p * p + 1.0
}

impl Default for WaveformView {
    fn default() -> Self {
        glib::Object::new()
    }
}

const ANIMATION_USECS: f64 = 250_000.0;

impl WaveformView {
    pub fn new() -> Self {
        Self::default()
    }

    fn setup_gesture(&self) {
        let drag_gesture = gtk::GestureDrag::new();
        drag_gesture.set_name(Some("waveform-drag"));
        drag_gesture.set_button(0);
        drag_gesture.connect_drag_begin(
            clone!(@strong self as this => move |gesture, start_x, _| {
                if !this.has_focus() {
                    this.grab_focus();
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                this.seek_to_coord(start_x);
            }),
        );
        drag_gesture.connect_drag_update(
            clone!(@strong self as this => move |gesture, offset_x, _| {
                if !this.has_focus() {
                    this.grab_focus();
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                this.seek_to_coord(gesture.start_point().unwrap().0 + offset_x);
            }),
        );
        self.add_controller(drag_gesture);

        let motion_gesture = gtk::EventControllerMotion::new();
        motion_gesture.set_name(Some("waveform-motion"));
        motion_gesture.connect_motion(clone!(@strong self as this => move |_, x, _| {
            let width = this.width() as f64;
            let position = x / width;
            this.imp().hover_position.replace(Some(position));
            this.queue_draw();
        }));
        motion_gesture.connect_leave(clone!(@strong self as this => move |_| {
            this.imp().hover_position.replace(None);
            this.queue_draw();
        }));
        self.add_controller(motion_gesture);

        let key_controller = gtk::EventControllerKey::new();
        key_controller.set_name(Some("waveform-key"));
        key_controller.connect_key_released(
            clone!(@strong self as this => move |_, keyval, _, _| {
                let delta = match keyval {
                    gdk::Key::Left => -0.05,
                    gdk::Key::Right => 0.05,
                    _ => return,
                };

                let position = this.imp().position.get() + delta;
                this.emit_by_name::<()>("position-changed", &[&position]);
            }),
        );
        self.add_controller(key_controller);
    }

    fn seek_to_coord(&self, pos: f64) {
        let width = self.width();
        let position = match self.direction() {
            gtk::TextDirection::Rtl => 1.0 - (pos / width as f64),
            _ => pos / width as f64,
        };
        debug!(
            "Seeking to coord {} (width: {}, position: {})",
            pos, width, position
        );
        self.emit_by_name::<()>("position-changed", &[&position]);
    }

    fn normalize_peaks(&self, peaks: Vec<(f64, f64)>) -> Vec<PeakPair> {
        let right_channel: Vec<f64> = peaks.iter().map(|p| p.0).collect();
        let left_channel: Vec<f64> = peaks.iter().map(|p| p.1).collect();

        let max_left: f64 = left_channel
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let max_right: f64 = right_channel
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);

        let normalized: Vec<PeakPair> = peaks
            .iter()
            .map(|p| PeakPair::new(p.0 / max_left, p.1 / max_right))
            .collect();

        debug!("Peaks: {}", normalized.len());
        normalized
    }

    pub fn set_peaks(&self, peaks: Option<Vec<(f64, f64)>>) {
        if let Some(tick_id) = self.imp().tick_id.replace(None) {
            tick_id.remove();
        }

        let peak_pairs = peaks.map(|p| self.normalize_peaks(p));
        let enable_animations = self.settings().is_gtk_enable_animations();
        if !enable_animations {
            self.imp().peaks.replace(peak_pairs);
            self.queue_resize();
            return;
        }

        self.imp().next_peaks.replace(peak_pairs);
        self.imp().factor.set(None);
        self.imp().first_frame_time.set(None);

        let tick_id = self.add_tick_callback(clone!(@strong self as this => move |_, clock| {
            let frame_time = clock.frame_time();
            if let Some(first_frame_time) = this.imp().first_frame_time.get() {
                if frame_time < first_frame_time {
                    warn!("Frame clock going backwards");
                    return glib::ControlFlow::Continue;
                }

                let has_peaks = this.imp().peaks.borrow().is_some();
                let has_next_peaks = this.imp().next_peaks.borrow().is_some();

                if has_peaks && has_next_peaks {
                    // Animate the existing peaks to zero
                    let progress = 1.0 - ((frame_time - first_frame_time) as f64 / ANIMATION_USECS);
                    let delta = ease_out_cubic(progress);
                    if delta < 0.0 {
                        this.imp().peaks.replace(None);
                        this.imp().factor.replace(None);
                    } else {
                        this.imp().factor.replace(Some(delta));
                        this.queue_draw();
                    }
                } else if has_peaks && !has_next_peaks {
                    // Animate the peaks from zero
                    let progress = (frame_time - first_frame_time) as f64 / ANIMATION_USECS;
                    let delta = ease_out_cubic(progress);
                    if delta > 1.0 {
                        // Animation complete
                        this.imp().factor.replace(None);
                        this.imp().first_frame_time.replace(None);
                        this.imp().tick_id.replace(None);
                        this.queue_resize();
                        return glib::ControlFlow::Break;
                    } else {
                        this.imp().factor.replace(Some(delta));
                        this.queue_draw();
                    }
                } else if !has_peaks && has_next_peaks {
                    // Swap peaks
                    let next_peaks = this.imp().next_peaks.take();
                    this.imp().peaks.replace(next_peaks);
                    this.imp().factor.replace(None);
                    this.imp().first_frame_time.replace(None);
                    this.queue_resize();
                } else {
                    // No peaks
                    this.imp().factor.replace(None);
                    this.imp().tick_id.replace(None);
                    this.queue_resize();
                    return glib::ControlFlow::Break;
                }
            } else {
                this.imp().first_frame_time.replace(Some(frame_time));
            }
            glib::ControlFlow::Continue
        }));

        self.imp().tick_id.replace(Some(tick_id));
        self.queue_resize();
    }

    pub fn set_position(&self, position: f64) {
        let pos = position.clamp(0.0, 1.0);
        self.imp().position.replace(pos);
        self.update_property(&[gtk::accessible::Property::ValueNow(pos)]);
        self.queue_draw();
    }
}
