//! The touch controls drawn over the game's frame (the Android test build, docs/ANDROID.md): the
//! app's virtual stick and buttons as see-through discs, rings and rectangles ([`set`]), drawn
//! after the frame and the frame-rate counter at each `Present`. Nothing when no shape is set
//! (every Windows run).

use std::sync::Mutex;

/// A shape, in the window's fractions (0 to 1, left to right, top to bottom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Its centre and its half size (a disc's radius in both, as fractions of the window's height,
    /// so it stays round).
    pub centre: [f32; 2],
    pub half: f32,
    /// Its colour (straight alpha).
    pub colour: [f32; 4],
    /// A disc (true) or a square; a ring's thickness as a share of its half size (0: filled).
    pub round: bool,
    pub ring: f32,
}

static SHAPES: Mutex<Vec<Shape>> = Mutex::new(Vec::new());

/// The shapes to draw from the next frame on (none: nothing drawn).
pub fn set(shapes: Vec<Shape>) {
    *SHAPES.lock().unwrap_or_else(|p| p.into_inner()) = shapes;
}

/// Whether any shape is set.
pub(crate) fn any() -> bool {
    !SHAPES.lock().unwrap_or_else(|p| p.into_inner()).is_empty()
}

fn shapes() -> Vec<Shape> {
    SHAPES.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// A shape as the shader takes it: its rectangle in clip space, its colour, its kind and ring.
const FLOATS: usize = 12;
const MAX_SHAPES: usize = 32;

const SHADER: &str = r#"
struct Inst {
    @location(0) rect: vec4<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) kind: vec4<f32>,
};
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) kind: vec4<f32>,
};
@vertex
fn vs(@builtin(vertex_index) v: u32, i: Inst) -> Out {
    let corner = vec2<f32>(f32(v & 1u), f32((v >> 1u) & 1u));
    var o: Out;
    let p = mix(i.rect.xy, i.rect.zw, corner);
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.uv = corner * 2.0 - vec2<f32>(1.0, 1.0);
    o.colour = i.colour;
    o.kind = i.kind;
    return o;
}
@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    var d = max(abs(i.uv.x), abs(i.uv.y));
    if (i.kind.x > 0.5) {
        d = length(i.uv);
    }
    if (d > 1.0) {
        discard;
    }
    if (i.kind.y > 0.0 && d < 1.0 - i.kind.y) {
        discard;
    }
    return i.colour;
}
"#;

/// The drawing (made for the window's format).
pub(crate) struct Overlay {
    pipeline: wgpu::RenderPipeline,
    pub(crate) format: wgpu::TextureFormat,
    instances: wgpu::Buffer,
}

impl Overlay {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Overlay {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("touch overlay"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("touch overlay"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: (FLOATS * 4) as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("touch overlay"),
            size: (MAX_SHAPES * FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Overlay {
            pipeline,
            format,
            instances,
        }
    }

    /// Draws the shapes set ([`set`]) into `target`, a `width` by `height` window frame that
    /// already holds the game's frame; nothing when none is set.
    pub(crate) fn draw(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        let shapes = shapes();
        if shapes.is_empty() || size.0 == 0 || size.1 == 0 {
            return;
        }
        let floats = instances(&shapes, size);
        let n = floats.len() / FLOATS;
        let mut bytes = Vec::with_capacity(floats.len() * 4);
        for f in &floats {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        queue.write_buffer(&self.instances, 0, &bytes);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("touch overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, 0..n as u32);
    }
}

/// The shapes as the shader's instances (at most [`MAX_SHAPES`]): each one's rectangle in clip
/// space (x right, y up), colour, and kind (round, ring).
pub(crate) fn instances(shapes: &[Shape], (w, h): (u32, u32)) -> Vec<f32> {
    let (w, h) = (w as f32, h as f32);
    let mut out = Vec::with_capacity(shapes.len().min(MAX_SHAPES) * FLOATS);
    for s in shapes.iter().take(MAX_SHAPES) {
        // The half size in pixels from the height, then in each axis's fraction.
        let px = s.half * h;
        let (hx, hy) = (px / w, px / h);
        let (x0, x1) = (s.centre[0] - hx, s.centre[0] + hx);
        let (y0, y1) = (s.centre[1] - hy, s.centre[1] + hy);
        out.extend_from_slice(&[
            x0 * 2.0 - 1.0,
            1.0 - y0 * 2.0,
            x1 * 2.0 - 1.0,
            1.0 - y1 * 2.0,
        ]);
        out.extend_from_slice(&s.colour);
        out.extend_from_slice(&[
            f32::from(u8::from(s.round)),
            s.ring.clamp(0.0, 1.0),
            0.0,
            0.0,
        ]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disc_stays_round_on_a_wide_window() {
        let disc = Shape {
            centre: [0.5, 0.5],
            half: 0.1,
            colour: [1.0; 4],
            round: true,
            ring: 0.0,
        };
        let f = instances(&[disc], (2000, 1000));
        assert_eq!(f.len(), FLOATS);
        let (wx, wy) = ((f[2] - f[0]) * 1000.0, (f[1] - f[3]) * 500.0);
        assert!((wx - wy).abs() < 1e-3, "{wx} {wy}");
        assert_eq!(f[8], 1.0);
    }
}
