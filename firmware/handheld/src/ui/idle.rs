//! Idle behavior: dim the screen, and eventually power off, when the device is
//! left sitting in a menu.
//!
//! Idling only happens in the menus. A running game is left alone -- the player
//! may be watching it rather than pressing buttons -- as is anything driven over
//! USB, which makes progress without any input at all.

use std::time::Duration;

use crate::bitstream::{self, CurrentBitstream};
use crate::device::drivers::usb::{self, UsbMode};
use crate::device::{Device, DisplayMode};
use crate::kvs;

/// How long the device must sit idle before the screen dims.
///
/// Dimming doubles as a warning that the device is about to power itself off.
pub const DIM_TIMEOUT: Duration = Duration::from_secs(60);

/// Fraction of the configured brightness to use while dimmed.
const DIM_FACTOR: f32 = 0.25;

/// Labels for the "Auto Power Off" setting, in [`POWER_OFF_TIMEOUTS`] order.
pub const POWER_OFF_CHOICES: &[&str] =
    &["Off", "2 minutes", "5 minutes", "10 minutes", "30 minutes"];

/// Idle timeouts offered by the "Auto Power Off" setting, indexed by its value.
/// `None` disables automatic power off.
const POWER_OFF_TIMEOUTS: &[Option<Duration>] = &[
    None,
    Some(Duration::from_secs(2 * 60)),
    Some(Duration::from_secs(5 * 60)),
    Some(Duration::from_secs(10 * 60)),
    Some(Duration::from_secs(30 * 60)),
];

const _: () = assert!(POWER_OFF_TIMEOUTS.len() == POWER_OFF_CHOICES.len());

/// How long to wait after dimming before powering off, or `None` if automatic
/// power off is disabled.
pub fn power_off_delay() -> Option<Duration> {
    // `get` already substitutes the key's own default (5 minutes), so this
    // fallback only fires if the read itself fails. Index 0 is "Off": if we
    // can't tell what the user asked for, don't power their device off.
    let index = kvs::keys::AUTO_POWER_OFF.get().unwrap_or_default();
    // A negative index wraps to a large `usize`, which `get` rejects.
    let timeout = (*POWER_OFF_TIMEOUTS.get(index as usize)?)?;
    Some(timeout.saturating_sub(DIM_TIMEOUT))
}

/// Whether the device may idle (dim, and eventually power off) right now.
pub fn may_idle() -> bool {
    // A game is running: the player may be watching it rather than pressing
    // buttons. The menus run on the boot bitstream, which is `None`.
    if !matches!(*bitstream::current(), CurrentBitstream::None) {
        return false;
    }
    let device = Device::lock();

    // Mass storage and cartridge reader sessions run without any button input --
    // but only while a host is actually attached. Neither mode is reset by
    // unplugging the cable, only by ending the session from the Tools screen, so
    // without the VBUS check pulling the cable would leave the device unable to
    // ever idle again.
    if !matches!(
        usb::current_mode(),
        UsbMode::SerialJtag | UsbMode::ConsoleOnly
    ) && device.get_vbus_pgood()
    {
        return false;
    }

    // Docked: the internal backlight is already off, and the user is looking at
    // the external display.
    device.get_display_mode() == DisplayMode::Internal
}

/// Whether the device may power itself off right now.
///
/// Stricter than [`may_idle`]: there is no battery to save while running off
/// USB power, and powering off would cut short whatever it's plugged into.
pub fn may_power_off() -> bool {
    !Device::lock().get_vbus_pgood()
}

/// Dim or restore the backlight.
pub fn set_dimmed(dimmed: bool) {
    let brightness = kvs::keys::BRIGHTNESS.get().unwrap();
    let brightness = if dimmed {
        brightness * DIM_FACTOR
    } else {
        brightness
    };
    Device::lock().set_brightness(brightness);
}

/// Power off after sitting idle.
pub fn power_off() -> ! {
    log::info!("Idle timeout, powering off");
    Device::lock().power_off()
}
