//! The living wallpapers' shaders (`hocus_scenes::shaders`, GLSL ES 1.00
//! as the compositor compiles them with GLES) adapted for wgpu, whose
//! GLSL reader takes Vulkan's GLSL 4.50: the same code, its declarations
//! said another way.
//!
//! - `uniform T name;` (and arrays) go into one block of the stage's,
//!   laid out by std140 ([`Block`] says where each is);
//! - `uniform sampler2D name;` becomes a texture and a sampler of its
//!   own, and `texture2D(name, …)` reads them;
//! - `attribute` and `varying` get their locations in the order they are
//!   declared (a vertex shader's `varying`s and its fragment shader's
//!   are declared in the same order);
//! - `gl_FragColor` is the stage's first output.

/// A uniform's type, as std140 lays it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Mat3,
}

impl Kind {
    fn of(word: &str) -> Option<Self> {
        Some(match word {
            "float" => Self::Float,
            "vec2" => Self::Vec2,
            "vec3" => Self::Vec3,
            "vec4" => Self::Vec4,
            "mat3" => Self::Mat3,
            _ => return None,
        })
    }

    /// Its alignment and size, bytes.
    const fn layout(self) -> (usize, usize) {
        match self {
            Self::Float => (4, 4),
            Self::Vec2 => (8, 8),
            Self::Vec3 => (16, 12),
            Self::Vec4 => (16, 16),
            Self::Mat3 => (16, 48),
        }
    }
}

/// A stage's uniforms, where each lies in its block.
#[derive(Debug, Clone, Default)]
pub struct Block {
    pub members: Vec<(String, Kind, usize, usize)>,
    pub size: usize,
}

impl Block {
    fn push(&mut self, name: &str, kind: Kind, count: Option<usize>) {
        let (align, size) = kind.layout();
        let (align, stride, n) = match count {
            // An array's elements are each a vec4's at least.
            Some(n) => (16, size.div_ceil(16) * 16, n),
            None => (align, size, 1),
        };
        let at = self.size.div_ceil(align) * align;
        self.members.push((name.to_owned(), kind, at, stride));
        self.size = at + stride * n;
    }

    /// The block's bytes, each named member written from `values` (floats;
    /// a `mat3` its three columns of three, an array its elements' in a
    /// row); the others nothing.
    pub fn bytes(&self, values: &[(&str, &[f32])]) -> Vec<u8> {
        let mut out = vec![0u8; self.size.div_ceil(16).max(1) * 16];
        for (name, floats) in values {
            let Some((_, kind, at, stride)) = self.members.iter().find(|m| m.0 == *name) else {
                continue;
            };
            let per = match kind {
                Kind::Float => 1,
                Kind::Vec2 => 2,
                Kind::Vec3 | Kind::Mat3 => 3,
                Kind::Vec4 => 4,
            };
            // A mat3's columns lie a vec4 apart, as an array's elements.
            let stride = if *kind == Kind::Mat3 { 16 } else { *stride };
            for (i, chunk) in floats.chunks(per).enumerate() {
                for (j, v) in chunk.iter().enumerate() {
                    let o = at + i * stride + j * 4;
                    if let Some(to) = out.get_mut(o..o + 4) {
                        to.copy_from_slice(&v.to_ne_bytes());
                    }
                }
            }
        }
        out
    }
}

/// A shader adapted: its source, its uniforms' block and its textures, in
/// their bindings' order.
#[derive(Debug, Clone)]
pub struct Adapted {
    pub source: String,
    pub block: Block,
    pub textures: Vec<String>,
}

/// The stage a shader is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
}

/// `source` (GLSL ES 1.00) as GLSL 4.50 for wgpu: its block at
/// `binding` of group 0, its textures and samplers after it.
pub fn adapt(source: &str, stage: Stage, binding: u32) -> Adapted {
    let mut block = Block::default();
    let mut textures = Vec::new();
    let mut members = String::new();
    let mut declared = String::new();
    let mut body = String::new();
    let (mut attributes, mut varyings) = (0, 0);
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#version") || trimmed.starts_with("precision ") {
            continue;
        }
        let words: Vec<&str> = trimmed
            .trim_end_matches(';')
            .split_whitespace()
            .filter(|w| !matches!(*w, "highp" | "mediump" | "lowp"))
            .collect();
        match words.as_slice() {
            ["uniform", "sampler2D", name] => {
                let at = binding + 1 + 2 * textures.len() as u32;
                declared.push_str(&format!(
                    "layout(set = 0, binding = {at}) uniform texture2D {name};\n\
                     layout(set = 0, binding = {}) uniform sampler {name}_sampler;\n",
                    at + 1
                ));
                textures.push((*name).to_owned());
            }
            ["uniform", kind, name] => {
                let (name, count) = match name.split_once('[') {
                    Some((n, c)) => (n, c.trim_end_matches(']').parse().ok()),
                    None => (*name, None),
                };
                if let Some(k) = Kind::of(kind) {
                    block.push(name, k, count);
                }
                let dims = count.map(|c| format!("[{c}]")).unwrap_or_default();
                members.push_str(&format!("    {kind} {name}{dims};\n"));
            }
            ["attribute", kind, name] => {
                declared.push_str(&format!(
                    "layout(location = {attributes}) in {kind} {name};\n"
                ));
                attributes += 1;
            }
            ["varying", kind, name] => {
                let way = if stage == Stage::Vertex { "out" } else { "in" };
                declared.push_str(&format!(
                    "layout(location = {varyings}) {way} {kind} {name};\n"
                ));
                varyings += 1;
            }
            _ => {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    for name in &textures {
        body = body.replace(
            &format!("texture2D({name},"),
            &format!("texture(sampler2D({name}, {name}_sampler),"),
        );
    }
    let mut out = String::from("#version 450\n");
    if !members.is_empty() {
        let label = if stage == Stage::Vertex {
            "VertexLook"
        } else {
            "FragmentLook"
        };
        out.push_str(&format!(
            "layout(set = 0, binding = {binding}, std140) uniform {label} {{\n{members}}};\n"
        ));
    }
    out.push_str(&declared);
    if stage == Stage::Fragment {
        out.push_str("layout(location = 0) out vec4 hocus_colour;\n");
        body = body.replace("gl_FragColor", "hocus_colour");
    }
    out.push_str(&body);
    Adapted {
        source: out,
        block,
        textures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn std140_lays_out_the_uniforms() {
        let mut b = Block::default();
        b.push("matrix", Kind::Mat3, None);
        b.push("size", Kind::Vec2, None);
        b.push("time", Kind::Float, None);
        b.push("top", Kind::Vec3, None);
        b.push("holes", Kind::Vec4, Some(8));
        b.push("count", Kind::Float, None);
        let at: Vec<usize> = b.members.iter().map(|m| m.2).collect();
        assert_eq!(at, [0, 48, 56, 64, 80, 208]);
        let bytes = b.bytes(&[("matrix", &[1.0, 2.0, 3.0, 4.0]), ("count", &[5.0])]);
        assert_eq!(bytes.len(), 224);
        assert_eq!(&bytes[16..20], &4.0f32.to_ne_bytes());
        assert_eq!(&bytes[208..212], &5.0f32.to_ne_bytes());
    }

    /// Every living wallpaper's shader reads as GLSL 4.50.
    #[test]
    fn the_wallpapers_shaders_read() {
        use hocus_scenes::shaders as s;
        let clip = |source: &str| source.replacen("varying vec2 v_at;\n", &format!("varying vec2 v_at;\n{}", s::CLIP), 1);
        let cases = [
            (s::VERTEX.to_owned(), Stage::Vertex),
            (s::SPARKS_VERTEX.to_owned(), Stage::Vertex),
            (s::AURORA.to_owned(), Stage::Fragment),
            (s::SKY.to_owned(), Stage::Fragment),
            (s::DEPTH.to_owned(), Stage::Fragment),
            (s::KOI.to_owned(), Stage::Fragment),
            (clip(s::SHAPES), Stage::Fragment),
        ];
        for (source, stage) in cases {
            let adapted = adapt(&source, stage, 0);
            let naga_stage = match stage {
                Stage::Vertex => naga::ShaderStage::Vertex,
                Stage::Fragment => naga::ShaderStage::Fragment,
            };
            let parsed = naga::front::glsl::Frontend::default()
                .parse(&naga::front::glsl::Options::from(naga_stage), &adapted.source);
            assert!(
                parsed.is_ok(),
                "{:?}\n{}",
                parsed.err(),
                adapted.source
            );
        }
    }
}
