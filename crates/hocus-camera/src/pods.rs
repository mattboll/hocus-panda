//! The formats and buffers the streams offer, as SPA's pods.

use pipewire::spa;
use spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use spa::param::video::VideoFormat;
use spa::pod::{ChoiceValue, Object, Pod, Property, PropertyFlags, Value};
use spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Fraction, Id, Rectangle};

use crate::convert::{Input, Output, Pixels};
use hocus_link::camera::{CAMERA_FPS, CAMERA_SIZE};

fn prop(key: FormatProperties, value: Value) -> Property {
    Property {
        key: key.as_raw(),
        flags: PropertyFlags::empty(),
        value,
    }
}

fn id(v: u32) -> Value {
    Value::Id(Id(v))
}

fn formats(default: VideoFormat, all: &[VideoFormat]) -> Value {
    Value::Choice(ChoiceValue::Id(Choice(
        ChoiceFlags::empty(),
        ChoiceEnum::Enum {
            default: Id(default.as_raw()),
            alternatives: all.iter().map(|f| Id(f.as_raw())).collect(),
        },
    )))
}

fn rectangle_range(default: (u32, u32), min: (u32, u32), max: (u32, u32)) -> Value {
    let r = |(width, height)| Rectangle { width, height };
    Value::Choice(ChoiceValue::Rectangle(Choice(
        ChoiceFlags::empty(),
        ChoiceEnum::Range {
            default: r(default),
            min: r(min),
            max: r(max),
        },
    )))
}

fn rate_range(default: u32, min: u32, max: u32) -> Value {
    let f = |num| Fraction { num, denom: 1 };
    Value::Choice(ChoiceValue::Fraction(Choice(
        ChoiceFlags::empty(),
        ChoiceEnum::Range {
            default: f(default),
            min: f(min),
            max: f(max),
        },
    )))
}

fn format_object(properties: Vec<Property>) -> Object {
    Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties,
    }
}

/// An object's bytes, to make a [`Pod`] of.
pub fn serialize(object: Object) -> Vec<u8> {
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .unwrap_or_default()
}

/// What the source offers: 1280 × 720 at 30 frames a second, B, G, R, x
/// first, or I420 and YUY2 for the readers that take only YUV (the
/// browsers' WebRTC).
pub fn source_format() -> Vec<u8> {
    serialize(format_object(vec![
        prop(FormatProperties::MediaType, id(MediaType::Video.as_raw())),
        prop(
            FormatProperties::MediaSubtype,
            id(MediaSubtype::Raw.as_raw()),
        ),
        prop(
            FormatProperties::VideoFormat,
            formats(
                VideoFormat::BGRx,
                &[
                    VideoFormat::BGRx,
                    VideoFormat::BGRA,
                    VideoFormat::I420,
                    VideoFormat::YUY2,
                ],
            ),
        ),
        prop(
            FormatProperties::VideoSize,
            Value::Rectangle(Rectangle {
                width: CAMERA_SIZE.0,
                height: CAMERA_SIZE.1,
            }),
        ),
        prop(
            FormatProperties::VideoFramerate,
            Value::Fraction(Fraction {
                num: CAMERA_FPS,
                denom: 1,
            }),
        ),
    ]))
}

/// What a reader of the camera takes (the source's own, and the
/// preview's): MJPEG first, as webcams send 720p at 30 a second in it,
/// then raw pixels of the kinds [`crate::convert`] reads.
pub fn camera_formats() -> [Vec<u8>; 2] {
    let size = rectangle_range(CAMERA_SIZE, (160, 120), (1920, 1080));
    let rate = rate_range(CAMERA_FPS, 1, 60);
    let mjpg = format_object(vec![
        prop(FormatProperties::MediaType, id(MediaType::Video.as_raw())),
        prop(
            FormatProperties::MediaSubtype,
            id(MediaSubtype::Mjpg.as_raw()),
        ),
        prop(FormatProperties::VideoSize, size.clone()),
        prop(FormatProperties::VideoFramerate, rate.clone()),
    ]);
    let raw = format_object(vec![
        prop(FormatProperties::MediaType, id(MediaType::Video.as_raw())),
        prop(
            FormatProperties::MediaSubtype,
            id(MediaSubtype::Raw.as_raw()),
        ),
        prop(
            FormatProperties::VideoFormat,
            formats(
                VideoFormat::YUY2,
                &[
                    VideoFormat::YUY2,
                    VideoFormat::NV12,
                    VideoFormat::I420,
                    VideoFormat::BGRx,
                    VideoFormat::BGRA,
                    VideoFormat::RGBx,
                    VideoFormat::RGBA,
                    VideoFormat::RGB,
                    VideoFormat::BGR,
                ],
            ),
        ),
        prop(FormatProperties::VideoSize, size),
        prop(FormatProperties::VideoFramerate, rate),
    ]);
    [serialize(mjpg), serialize(raw)]
}

/// What the preview takes: the source's B, G, R, x alone.
pub fn preview_format() -> Vec<u8> {
    serialize(format_object(vec![
        prop(FormatProperties::MediaType, id(MediaType::Video.as_raw())),
        prop(
            FormatProperties::MediaSubtype,
            id(MediaSubtype::Raw.as_raw()),
        ),
        prop(
            FormatProperties::VideoFormat,
            formats(VideoFormat::BGRx, &[VideoFormat::BGRx, VideoFormat::BGRA]),
        ),
        prop(
            FormatProperties::VideoSize,
            rectangle_range(CAMERA_SIZE, (1, 1), (8192, 8192)),
        ),
        prop(
            FormatProperties::VideoFramerate,
            rate_range(CAMERA_FPS, 0, 120),
        ),
    ]))
}

/// The buffers asked for once a format is agreed: 2 to 8 of one block of
/// `size` bytes, rows of `stride`, in shared memory.
pub fn buffers(stride: usize, size: usize) -> Vec<u8> {
    let p = |key: u32, value: Value| Property {
        key,
        flags: PropertyFlags::empty(),
        value,
    };
    let int = |v: usize| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
    let memory = (1 << spa::sys::SPA_DATA_MemFd) | (1 << spa::sys::SPA_DATA_MemPtr);
    serialize(Object {
        type_: spa::utils::SpaTypes::ObjectParamBuffers.as_raw(),
        id: spa::param::ParamType::Buffers.as_raw(),
        properties: vec![
            p(
                spa::sys::SPA_PARAM_BUFFERS_buffers,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Range {
                        default: 4,
                        min: 2,
                        max: 8,
                    },
                ))),
            ),
            p(spa::sys::SPA_PARAM_BUFFERS_blocks, Value::Int(1)),
            p(spa::sys::SPA_PARAM_BUFFERS_size, int(size)),
            p(spa::sys::SPA_PARAM_BUFFERS_stride, int(stride)),
            p(
                spa::sys::SPA_PARAM_BUFFERS_dataType,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags {
                        default: memory,
                        flags: vec![memory],
                    },
                ))),
            ),
        ],
    })
}

/// The reader's format a camera agreed to, as [`Input`] (its rows' bytes
/// come with each frame).
pub fn input_of(param: &Pod) -> Option<Input> {
    let (media, subtype) = spa::param::format_utils::parse_format(param).ok()?;
    if media != MediaType::Video {
        return None;
    }
    if subtype == MediaSubtype::Mjpg {
        return Some(Input {
            pixels: Pixels::Mjpg,
            size: size_of(param)?,
            stride: 0,
        });
    }
    let mut info = spa::param::video::VideoInfoRaw::default();
    info.parse(param).ok()?;
    let pixels = match info.format() {
        VideoFormat::YUY2 => Pixels::Yuy2,
        VideoFormat::NV12 => Pixels::Nv12,
        VideoFormat::I420 => Pixels::I420,
        VideoFormat::BGRx | VideoFormat::BGRA => Pixels::Bgrx,
        VideoFormat::RGBx | VideoFormat::RGBA => Pixels::Rgbx,
        VideoFormat::RGB => Pixels::Rgb,
        VideoFormat::BGR => Pixels::Bgr,
        _ => return None,
    };
    Some(Input {
        pixels,
        size: (info.size().width, info.size().height),
        stride: 0,
    })
}

/// A format's size, read from its pod (MJPEG's has no raw video info).
fn size_of(param: &Pod) -> Option<(u32, u32)> {
    let (_, value) =
        spa::pod::deserialize::PodDeserializer::deserialize_any_from(param.as_bytes()).ok()?;
    let Value::Object(object) = value else {
        return None;
    };
    object.properties.iter().find_map(|p| {
        (p.key == FormatProperties::VideoSize.as_raw())
            .then_some(&p.value)
            .and_then(|v| match v {
                Value::Rectangle(r) => Some((r.width, r.height)),
                Value::Choice(ChoiceValue::Rectangle(Choice(_, ChoiceEnum::None(r)))) => {
                    Some((r.width, r.height))
                }
                _ => None,
            })
    })
}

/// What a reader of the source agreed to.
pub fn output_of(param: &Pod) -> Option<Output> {
    let mut info = spa::param::video::VideoInfoRaw::default();
    info.parse(param).ok()?;
    match info.format() {
        VideoFormat::BGRx | VideoFormat::BGRA => Some(Output::Bgrx),
        VideoFormat::I420 => Some(Output::I420),
        VideoFormat::YUY2 => Some(Output::Yuy2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pods_read_back() {
        for bytes in [source_format(), preview_format(), buffers(5120, 5120 * 720)]
            .into_iter()
            .chain(camera_formats())
        {
            assert!(Pod::from_bytes(&bytes).is_some());
        }
    }
}
