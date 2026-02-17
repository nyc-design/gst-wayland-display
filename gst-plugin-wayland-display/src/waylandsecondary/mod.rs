use gst::glib;
use gst::prelude::*;

mod imp;

glib::wrapper! {
    pub struct WaylandDisplaySecondary(ObjectSubclass<imp::WaylandDisplaySecondary>) @extends gst_base::PushSrc, gst_base::BaseSrc, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "waylanddisplaysecondary",
        gst::Rank::NONE,
        WaylandDisplaySecondary::static_type(),
    )
}
