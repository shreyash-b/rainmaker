#![allow(dead_code)]

use components::error::Error;
use esp_idf_svc::{
    hal::{
        delay::NON_BLOCK,
        gpio::{Input, InputPin, InterruptType, PinDriver},
        task::notification::Notification,
    },
    sys::esp_timer_get_time,
};
use std::num::NonZeroU32;

const MAX_CALLBACKS_COUNT: usize = 10;

struct Callback<'d> {
    callback_fn: Box<dyn FnMut() + 'd>,
    interval: u32,
    title: &'d str,
}

pub struct GPIDriver<'d, T: InputPin> {
    driver: PinDriver<'d, T, Input>,
    notification: Notification,
    callbacks: [Option<Callback<'d>>; MAX_CALLBACKS_COUNT], // Supports a max of 10 callbacks
    callbacks_len: usize,
    pin_pressed_micros: i64,
    next_press_interval: u32,
    next_press_printed: bool,
    next_press_title: &'d str,
}

impl<'d, T> GPIDriver<'d, T>
where
    T: InputPin,
{
    pub fn new(pin: T) -> Result<Self, Error> {
        let notification = Notification::new();
        let notifier = notification.notifier();

        let mut driver = PinDriver::input(pin)?;

        // Possibly put initialiation in a separate function?
        driver.set_interrupt_type(esp_idf_svc::hal::gpio::InterruptType::LowLevel)?;
        unsafe {
            driver.subscribe(move || {
                let _ = notifier.notify(NonZeroU32::new(1).unwrap());
            })?;
        }
        driver.enable_interrupt()?;

        Ok(Self {
            driver,
            notification,
            callbacks: Default::default(),
            callbacks_len: 0,
            pin_pressed_micros: 0,
            next_press_interval: 0,
            next_press_title: "",
            next_press_printed: true,
        })
    }

    // Sets callback for tap event(press and release quickly)
    pub fn set_tap_cb<U>(&mut self, cb: Box<U>)
    where
        U: FnMut() + 'd,
    {
        self.set_press_cb(cb, 0, "tap action");
    }

    pub fn set_press_cb<U>(&mut self, cb: Box<U>, interval: u32, title: &'d str)
    where
        U: FnMut() + 'd,
    {
        if self.callbacks_len >= MAX_CALLBACKS_COUNT {
            log::error!("Cannot add more than {} callbacks", MAX_CALLBACKS_COUNT);
            return;
        }

        self.callbacks[self.callbacks_len] = Some(Callback {
            callback_fn: cb,
            interval,
            title,
        });
        self.callbacks_len += 1;
    }

    pub fn poll(&mut self) {
        let curr_call_micros = unsafe { esp_timer_get_time() };
        let mut interval = (curr_call_micros - self.pin_pressed_micros) as u32;
        interval /= 1000;

        if self.pin_pressed_micros != 0 {
            if interval >= self.next_press_interval {
                if !self.next_press_printed {
                    log::info!("release for {}", self.next_press_title);
                    self.next_press_printed = true;
                }
                self.update_next_press_name_interval(interval);
            }
        }

        // Check if ISR was triggered before this function call
        if self.notification.wait(NON_BLOCK).is_none() {
            return;
        }

        let curr_call_micros = unsafe { esp_timer_get_time() };
        // Button just pressed
        if self.pin_pressed_micros == 0 {
            self.pin_pressed_micros = curr_call_micros;
            self.driver
                .set_interrupt_type(InterruptType::HighLevel)
                .unwrap(); // notify when released
            self.update_next_press_name_interval(interval);
        } else {
            let mut selected_cb: Option<&mut Callback<'d>> = None;

            for callback in self.callbacks[..self.callbacks_len].iter_mut() {
                if let Some(cb) = callback {
                    match selected_cb {
                        Some(ref mut callback) => {
                            if interval > cb.interval && callback.interval < cb.interval {
                                *callback = cb;
                            }
                        }
                        None => selected_cb = Some(cb),
                    }
                }
            }

            if let Some(cb) = selected_cb {
                (cb.callback_fn)();
            }

            self.driver
                .set_interrupt_type(InterruptType::LowLevel)
                .unwrap();
            self.pin_pressed_micros = 0;

            self.next_press_interval = 0;
            self.next_press_printed = true;
        }
        self.driver.enable_interrupt().unwrap();
    }

    fn update_next_press_name_interval(&mut self, curr_interval: u32) {
        let mut title: &str = self.next_press_title;
        let mut interval: u32 = self.next_press_interval;

        for callback in self.callbacks[..self.callbacks_len].iter() {
            if let Some(cb) = callback {
                if cb.interval > interval && cb.interval > curr_interval {
                    interval = cb.interval;
                    title = cb.title;
                    break;
                }
            }
        }

        if interval > curr_interval {
            self.next_press_title = title;
            self.next_press_interval = interval;
            self.next_press_printed = false;
        }
    }
}
