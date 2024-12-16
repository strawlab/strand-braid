#[macro_use]
extern crate glib;
use glib::prelude::*;
#[macro_use]
extern crate gstreamer as gst;
extern crate gstreamer_base as gst_base;
extern crate gstreamer_video as gst_video;

#[macro_use]
extern crate lazy_static;

mod apriltagdetector;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Clone, Copy)]
#[repr(u32)]
pub enum TagFamily {
    Family36h11 = 0,
    FamilyStandard41h12 = 1,
    Family16h5 = 2,
    Family25h9 = 3,
    FamilyCircle21h7 = 4,
    FamilyCircle49h12 = 5,
    FamilyCustom48h12 = 6,
    FamilyStandard52h13 = 7,
}

impl std::fmt::Display for TagFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use TagFamily::*;
        let fam = match self {
            Family36h11 => "36h11".to_string(),
            FamilyStandard41h12 => "standard-41h12".to_string(),
            Family16h5 => "16h5".to_string(),
            Family25h9 => "25h9".to_string(),
            FamilyCircle21h7 => "circle-21h7".to_string(),
            FamilyCircle49h12 => "circle-49h12".to_string(),
            FamilyCustom48h12 => "custom-48h12".to_string(),
            FamilyStandard52h13 => "standard-52h13".to_string(),
        };

        write!(f, "{}", fam)
    }
}

impl glib::translate::ToGlib for TagFamily {
    type GlibType = i32;

    fn to_glib(&self) -> i32 {
        *self as i32
    }
}

impl glib::translate::FromGlib<i32> for TagFamily {
    fn from_glib(value: i32) -> Self {
        use TagFamily::*;
        match value {
            0 => Family36h11,
            1 => FamilyStandard41h12,
            2 => Family16h5,
            3 => Family25h9,
            4 => FamilyCircle21h7,
            5 => FamilyCircle49h12,
            6 => FamilyCustom48h12,
            7 => FamilyStandard52h13,
            _ => unreachable!(),
        }
    }
}

impl StaticType for TagFamily {
    fn static_type() -> glib::Type {
        tag_family_get_type()
    }
}

impl<'a> glib::value::FromValueOptional<'a> for TagFamily {
    unsafe fn from_value_optional(value: &glib::Value) -> Option<Self> {
        Some(glib::value::FromValue::from_value(value))
    }
}

impl<'a> glib::value::FromValue<'a> for TagFamily {
    unsafe fn from_value(value: &glib::Value) -> Self {
        use glib::translate::ToGlibPtr;

        glib::translate::from_glib(gobject_sys::g_value_get_enum(value.to_glib_none().0))
    }
}

impl glib::value::SetValue for TagFamily {
    unsafe fn set_value(value: &mut glib::Value, this: &Self) {
        use glib::translate::{ToGlib, ToGlibPtrMut};

        gobject_sys::g_value_set_enum(value.to_glib_none_mut().0, this.to_glib())
    }
}

fn tag_family_get_type() -> glib::Type {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    static mut TYPE: glib::Type = glib::Type::Invalid;

    ONCE.call_once(|| {
        use std::ffi;
        use std::ptr;

        static mut VALUES: [gobject_sys::GEnumValue; 9] = [
            gobject_sys::GEnumValue {
                value: TagFamily::Family36h11 as i32,
                value_name: b"36H11\0" as *const _ as *const _,
                value_nick: b"36h11\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: TagFamily::FamilyStandard41h12 as i32,
                value_name: b"Standard 41H12\0" as *const _ as *const _,
                value_nick: b"standard-41h12\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: TagFamily::Family16h5 as i32,
                value_name: b"16H5\0" as *const _ as *const _,
                value_nick: b"16h5\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: TagFamily::Family25h9 as i32,
                value_name: b"25H9\0" as *const _ as *const _,
                value_nick: b"25h9\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: TagFamily::FamilyCircle21h7 as i32,
                value_name: b"Circle 21hH7\0" as *const _ as *const _,
                value_nick: b"circle-21h7\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: TagFamily::FamilyCircle49h12 as i32,
                value_name: b"Circle 49H12\0" as *const _ as *const _,
                value_nick: b"circle-49h12\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: TagFamily::FamilyCustom48h12 as i32,
                value_name: b"Custom 48H12\0" as *const _ as *const _,
                value_nick: b"custom-48h12\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: TagFamily::FamilyStandard52h13 as i32,
                value_name: b"Standard 52H13\0" as *const _ as *const _,
                value_nick: b"standard-52h13\0" as *const _ as *const _,
            },
            gobject_sys::GEnumValue {
                value: 0,
                value_name: ptr::null(),
                value_nick: ptr::null(),
            },
        ];

        let name = ffi::CString::new("GstApriltagTagFamily").unwrap();
        #[allow(static_mut_refs)]
        unsafe {
            let type_ = gobject_sys::g_enum_register_static(name.as_ptr(), VALUES.as_ptr());
            TYPE = glib::translate::from_glib(type_);
        }
    });

    unsafe { TYPE }
}

gst_plugin_define!(
    apriltag,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    "BSD",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    apriltagdetector::register(plugin)?;
    Ok(())
}
