mod auto_mode_select;
pub use self::auto_mode_select::AutoModeSelect;

#[cfg(feature = "with_camtrig")]
mod camtrig_control;
#[cfg(feature = "with_camtrig")]
pub use self::camtrig_control::CamtrigControl;

#[cfg(feature = "with_camtrig")]
mod led_control;

#[cfg(feature = "with_camtrig")]
pub use self::led_control::LedControl;
