pub mod canvas;
pub mod filter;
/// GPU rendering pipeline using wgpu.
///
/// Renders colored rectangles (layout boxes) and text to the window surface.
///
/// Pipeline stages:
/// 1. Initialize wgpu (instance, adapter, device, surface)
/// 2. Create render pipeline (vertex + fragment shaders)
/// 3. For each frame: clear → draw rectangles → draw text → present
pub mod font_data;
pub mod icons;
pub mod painter;
pub mod text;

use crate::css::{
    BackgroundLength, BackgroundPosition, BackgroundRepeat, BackgroundSize, BorderStyle,
};
use crate::layout::Rect;

/// Maximum number of rectangles the GPU render pipeline can handle in a single frame.
pub const MAX_RECTS: usize = 2048;

/// Wgpu rendering context.
pub struct Renderer {
    window: winit::window::Window,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// Render pipeline for drawing rectangles.
    rect_pipeline: wgpu::RenderPipeline,
    /// Uniform buffer for rectangle positions (clip space).
    rect_uniform_buffer: wgpu::Buffer,
    /// Uniform buffer for rectangle colors.
    rect_color_buffer: wgpu::Buffer,
    /// Bind group linking uniforms to the shader.
    rect_bind_group: wgpu::BindGroup,
    /// Current number of rectangles to draw (max MAX_RECTS).
    num_rects: u32,
    /// Texture for overlaying rasterized text onto the frame.
    text_texture: Option<wgpu::Texture>,
    /// Texture view for the text texture.
    text_texture_view: Option<wgpu::TextureView>,
    /// Sampler for the text texture (nearest-neighbor for crisp text).
    text_sampler: wgpu::Sampler,
    /// Render pipeline for drawing a textured quad (text overlay).
    text_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for the text pipeline (texture + sampler).
    text_bind_group_layout: wgpu::BindGroupLayout,
}

impl Renderer {
    /// Get a reference to the underlying window.
    pub fn window(&self) -> &winit::window::Window {
        &self.window
    }
}

/// Simple rectangle in clip-space coordinates.
#[derive(Clone, Copy)]
pub struct RectClip {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Color in f32 RGBA, each component in [0.0, 1.0].
#[derive(Clone, Copy)]
pub struct ColorF {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Renderer {
    /// Initialize wgpu and create the rendering context.
    pub async fn new(
        window: winit::window::Window,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create wgpu instance
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // Create surface from the window.
        // The surface borrows the window, but since we store the window in the
        // Renderer and it lives for the full session, we can safely extend
        // the lifetime to 'static.
        let surface = instance
            .create_surface(&window)
            .map_err(|e| format!("Failed to create surface: {:?}", e))?;
        let surface: wgpu::Surface<'static> = unsafe { std::mem::transmute(surface) };

        // Request an adapter
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .ok_or("Failed to find an appropriate adapter")?;

        // Request a device
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("Failed to request device: {}", e))?;

        // Configure the surface
        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f: &wgpu::TextureFormat| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        // Create render pipeline
        let rect_pipeline = Self::create_rect_pipeline(&device, surface_format);

        // Create uniform buffers (MAX_RECTS rects × vec4 = MAX_RECTS * 4 f32)
        let uniform_size = std::mem::size_of::<[f32; MAX_RECTS * 4]>() as u64;

        let rect_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rect Uniform Buffer"),
            size: uniform_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let rect_color_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rect Color Buffer"),
            size: uniform_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind group
        let bind_group_layout = rect_pipeline.get_bind_group_layout(0);
        let rect_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Rect Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: rect_uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: rect_color_buffer.as_entire_binding(),
                },
            ],
        });

        // Create text sampler — nearest-neighbor for crisp text rendering
        let text_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Create text render pipeline (textured quad overlay)
        let text_pipeline = Self::create_text_pipeline(&device, surface_format);
        let text_bind_group_layout = text_pipeline.get_bind_group_layout(0);

        log::info!(
            "Renderer initialized — surface format: {:?}, size: {}x{}",
            surface_format,
            size.width,
            size.height
        );

        Ok(Self {
            window,
            device,
            queue,
            surface,
            config,
            rect_pipeline,
            rect_uniform_buffer,
            rect_color_buffer,
            rect_bind_group,
            num_rects: 0,
            text_texture: None,
            text_texture_view: None,
            text_sampler,
            text_pipeline,
            text_bind_group_layout,
        })
    }

    /// Create the render pipeline for drawing rectangles.
    fn create_rect_pipeline(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Rectangle Shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}\n{}", shader::RECT_VERTEX, shader::RECT_FRAGMENT).into(),
            ),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Rect Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rect Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Rectangle Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Add,
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        },
                        alpha: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Add,
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None, // No depth buffer for 2D
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        })
    }

    /// Create the render pipeline for drawing a fullscreen textured quad (text overlay).
    fn create_text_pipeline(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Text Quad Shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!("{}\n{}", shader::TEXT_VERTEX, shader::TEXT_FRAGMENT).into(),
            ),
        });

        // Separate bind group layout for text texture + sampler
        let text_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Text Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Text Pipeline Layout"),
            bind_group_layouts: &[&text_bind_group_layout],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Text Quad Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_text_quad",
                buffers: &[], // Vertex positions are generated in the shader
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_text_quad",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Add,
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        },
                        alpha: wgpu::BlendComponent {
                            operation: wgpu::BlendOperation::Add,
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        })
    }

    /// Resize the renderer when the window is resized.
    pub fn resize(&mut self, new_width: u32, new_height: u32) {
        if new_width == 0 || new_height == 0 {
            return; // Ignore spurious resize events
        }
        self.config.width = new_width;
        self.config.height = new_height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Set rectangle data (positions in clip space).
    ///
    /// Clip space: x ∈ [-1, 1], y ∈ [-1, 1], origin at center.
    pub fn set_rects(&mut self, rects: &[RectClip], colors: &[ColorF]) {
        let count = rects.len().min(MAX_RECTS);
        self.num_rects = count as u32;

        // Upload positions as vec4 array [x, y, w, h] × MAX_RECTS
        let mut position_data = [0.0f32; MAX_RECTS * 4];
        for (i, rect) in rects.iter().take(MAX_RECTS).enumerate() {
            position_data[i * 4] = rect.x;
            position_data[i * 4 + 1] = rect.y;
            position_data[i * 4 + 2] = rect.width;
            position_data[i * 4 + 3] = rect.height;
        }
        self.queue.write_buffer(
            &self.rect_uniform_buffer,
            0,
            bytemuck::bytes_of(&position_data),
        );

        // Upload colors as vec4 array [r, g, b, a] × MAX_RECTS
        let mut color_data = [0.0f32; MAX_RECTS * 4];
        for (i, color) in colors.iter().take(MAX_RECTS).enumerate() {
            color_data[i * 4] = color.r;
            color_data[i * 4 + 1] = color.g;
            color_data[i * 4 + 2] = color.b;
            color_data[i * 4 + 3] = color.a;
        }
        self.queue
            .write_buffer(&self.rect_color_buffer, 0, bytemuck::bytes_of(&color_data));
    }

    /// Upload an RGBA bitmap as the text overlay texture.
    ///
    /// The `rgba_data` must contain `width * height * 4` bytes of RGBA pixel data.
    /// This texture is then composited on top of rectangles during rendering,
    /// using alpha blending so transparent areas show through.
    pub fn set_text_bitmap(&mut self, width: u32, height: u32, rgba_data: &[u8]) {
        if width == 0 || height == 0 || rgba_data.is_empty() {
            return;
        }

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Text Overlay Texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );

        self.text_texture = Some(texture);
        self.text_texture_view = None; // Invalidate cached view (created fresh each frame)
    }

    /// Render a frame: clear the surface and draw all rectangles.
    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            // Pre-create text bind group if needed.
            // Must be created BEFORE render_pass so it outlives render_pass on scope exit.
            let text_bg = if let Some(text_tex) = self.text_texture.as_ref() {
                let text_tex = text_tex;
                let text_view = text_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Text Bind Group"),
                    layout: &self.text_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&text_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.text_sampler),
                        },
                    ],
                });
                Some((text_view, bg))
            } else {
                None
            };

            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0, // White background (web default)
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    resolve_target: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.rect_pipeline);
            render_pass.set_bind_group(0, &self.rect_bind_group, &[]);

            // Draw triangles: 6 vertices per rectangle (2 triangles)
            let vert_count = self.num_rects * 6;
            render_pass.draw(0..vert_count, 0..1);

            // Draw text overlay if available
            if let Some((_view, ref bg)) = text_bg {
                render_pass.set_pipeline(&self.text_pipeline);
                render_pass.set_bind_group(0, bg, &[]);
                render_pass.draw(0..6, 0..1); // Fullscreen quad: 6 vertices
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }
}

/// Shader source — embedded as &str to avoid external .wgsl files.
mod shader {
    /// Vertex shader with per-rectangle uniform buffer.
    pub const RECT_VERTEX: &str = r#"
        struct RectUniform {
            rects: array<vec4f, 2048>,
        }

        struct RectColor {
            colors: array<vec4f, 2048>,
        }

        @group(0) @binding(0)
        var<uniform> rect_data: RectUniform;

        @group(0) @binding(1)
        var<uniform> rect_colors: RectColor;

        struct VertexOutput {
            @builtin(position) position: vec4f,
            @location(0) color: vec4f,
        }

        @vertex
        fn vs_main(
            @builtin(vertex_index) vertex_index: u32,
        ) -> VertexOutput {
            var out: VertexOutput;
            let tri = vertex_index % 6u;
            let rect_idx = vertex_index / 6u;
            if rect_idx > 2047u {
                out.position = vec4f(0.0, 0.0, 0.0, 1.0);
                out.color = vec4f(0.0, 0.0, 0.0, 0.0);
                return out;
            }
            let r = rect_data.rects[rect_idx];
            var off: vec2f;
            switch tri {
                case 0u: { off = vec2f(0.0, 0.0); }
                case 1u: { off = vec2f(1.0, 0.0); }
                case 2u: { off = vec2f(1.0, 1.0); }
                case 3u: { off = vec2f(0.0, 0.0); }
                case 4u: { off = vec2f(1.0, 1.0); }
                case 5u: { off = vec2f(0.0, 1.0); }
                default: { off = vec2f(0.0, 0.0); }
            }
            let px = r.x + off.x * r.z;
            let py = r.y - off.y * r.w;
            out.position = vec4f(px, py, 0.0, 1.0);
            out.color = rect_colors.colors[rect_idx];
            return out;
        }
    "#;

    /// Fragment shader that outputs the rectangle color.
    pub const RECT_FRAGMENT: &str = r#"
        @fragment
        fn fs_main(in: VertexOutput) -> @location(0) vec4f {
            return in.color;
        }
    "#;

    /// Vertex shader for the fullscreen text overlay quad.
    pub const TEXT_VERTEX: &str = r#"
        struct VertexOutput {
            @builtin(position) position: vec4f,
            @location(0) uv: vec2f,
        }

        @vertex
        fn vs_text_quad(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
            var out: VertexOutput;
            switch vertex_index {
                case 0u: {
                    out.position = vec4f(-1.0, -1.0, 0.0, 1.0);
                    out.uv = vec2f(0.0, 0.0);
                }
                case 1u: {
                    out.position = vec4f(-1.0, 1.0, 0.0, 1.0);
                    out.uv = vec2f(0.0, 1.0);
                }
                case 2u: {
                    out.position = vec4f(1.0, 1.0, 0.0, 1.0);
                    out.uv = vec2f(1.0, 1.0);
                }
                case 3u: {
                    out.position = vec4f(-1.0, -1.0, 0.0, 1.0);
                    out.uv = vec2f(0.0, 0.0);
                }
                case 4u: {
                    out.position = vec4f(1.0, 1.0, 0.0, 1.0);
                    out.uv = vec2f(1.0, 1.0);
                }
                case 5u: {
                    out.position = vec4f(1.0, -1.0, 0.0, 1.0);
                    out.uv = vec2f(1.0, 0.0);
                }
                default: {
                    out.position = vec4f(0.0, 0.0, 0.0, 1.0);
                    out.uv = vec2f(0.0, 0.0);
                }
            }
            return out;
        }
    "#;

    /// Fragment shader for the text overlay — samples the text texture with alpha blending.
    pub const TEXT_FRAGMENT: &str = r#"
        @group(0) @binding(0) var text_tex: texture_2d<f32>;
        @group(0) @binding(1) var text_smp: sampler;

        @fragment
        fn fs_text_quad(@location(0) uv: vec2f) -> @location(0) vec4f {
            let tex_uv = vec2f(uv.x, 1.0 - uv.y); // Flip Y to match layout coords
            return textureSample(text_tex, text_smp, tex_uv);
        }
    "#;
}

/// Helper: convert layout-space rectangle to clip-space.
///
/// Layout coords: (0,0) at top-left, y increases downward.
/// Clip coords: (0,0) at center, y increases upward, range [-1, 1].
pub fn layout_to_clip(
    lx: f32,
    ly: f32,
    lw: f32,
    lh: f32,
    view_width: f32,
    view_height: f32,
) -> RectClip {
    let cx = (lx / view_width) * 2.0 - 1.0;
    let cy = -((ly / view_height) * 2.0 - 1.0); // Flip Y
    let cw = (lw / view_width) * 2.0;
    let ch = (lh / view_height) * 2.0;
    RectClip {
        x: cx,
        y: cy,
        width: cw,
        height: ch,
    }
}

/// Convert CSS RGBA [u8;4] (0-255 range) to render ColorF (0.0-1.0 range).
pub fn color_u8_to_f32(color: [u8; 4]) -> ColorF {
    ColorF {
        r: color[0] as f32 / 255.0,
        g: color[1] as f32 / 255.0,
        b: color[2] as f32 / 255.0,
        a: color[3] as f32 / 255.0,
    }
}

/// Composite a decoded RGBA image into an RGBA buffer at the specified position.
///
/// Performs alpha blending: each source pixel is blended over the destination
/// using `src * alpha + dst * (1 - alpha)`.
///
/// Pixels that fall outside the destination buffer are silently clipped.
/// Negative offsets are supported — they shift the image so the top-left
/// of the source maps to a negative coordinate, and only pixels whose
/// mapped position falls within the destination are written.
///
/// # Arguments
/// * `src_rgba` - Source RGBA pixel data (4 bytes per pixel, row-major).
/// * `src_width` - Width of the source image in pixels.
/// * `src_height` - Height of the source image in pixels.
/// * `dest` - Destination RGBA buffer (mutable, 4 bytes per pixel).
/// * `dest_width` - Width of the destination buffer in pixels.
/// Composite an image buffer into a destination RGBA buffer at the specified position.
///
/// Blends `src_rgba` into `dest` using standard alpha compositing (over operator).
/// Handles out-of-bounds clipping automatically.
///
/// * `src_rgba` - Raw RGBA8 pixel data for the source image.
/// * `src_width` - Width of the source image in pixels.
/// * `src_height` - Height of the source image in pixels.
/// * `dest` - Target RGBA8 buffer to blend into.
/// * `dest_width` - Width of the destination buffer in pixels.
/// * `dest_height` - Height of the destination buffer in pixels.
/// * `dest_x` - X position (in layout space) to place the top-left of the image.
/// * `dest_y` - Y position (in layout space) to place the top-left of the image.
pub fn composite_image(
    src_rgba: &[u8],
    src_width: u32,
    src_height: u32,
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    dest_x: f32,
    dest_y: f32,
) {
    composite_image_scaled(
        src_rgba,
        src_width,
        src_height,
        dest,
        dest_width,
        dest_height,
        dest_x,
        dest_y,
        src_width as f32,
        src_height as f32,
    );
}

/// Composite and scale an image buffer into a destination RGBA buffer.
pub fn composite_image_scaled(
    src_rgba: &[u8],
    src_width: u32,
    src_height: u32,
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    dest_x: f32,
    dest_y: f32,
    target_width: f32,
    target_height: f32,
) {
    if src_width == 0 || src_height == 0 || target_width <= 0.0 || target_height <= 0.0 {
        return;
    }

    let dx_base = dest_x as i32;
    let dy_base = dest_y as i32;
    let target_w_i = target_width as i32;
    let target_h_i = target_height as i32;

    for out_y in 0..target_h_i {
        let d_y = dy_base + out_y;
        if d_y < 0 || d_y >= dest_height as i32 {
            continue;
        }

        let src_y = ((out_y as f32 / target_height) * src_height as f32)
            .min((src_height - 1) as f32) as usize;

        for out_x in 0..target_w_i {
            let d_x = dx_base + out_x;
            if d_x < 0 || d_x >= dest_width as i32 {
                continue;
            }

            let src_x = ((out_x as f32 / target_width) * src_width as f32)
                .min((src_width - 1) as f32) as usize;

            let src_idx = (src_y * src_width as usize + src_x) * 4;
            let dst_idx = ((d_y as usize) * dest_width as usize + (d_x as usize)) * 4;

            if src_idx + 3 < src_rgba.len() && dst_idx + 3 < dest.len() {
                // Alpha blend: src * a + dst * (1 - a)
                let a = src_rgba[src_idx + 3] as f32 / 255.0;
                for c in 0..4 {
                    let src_val = src_rgba[src_idx + c] as f32;
                    let dst_val = dest[dst_idx + c] as f32;
                    dest[dst_idx + c] = (src_val * a + dst_val * (1.0 - a)) as u8;
                }
            }
        }
    }
}

/// The parts of `bounds` that `clip` does not cover, as up to four rectangles.
///
/// Empty when `bounds` lies entirely inside `clip`.
fn outside_clip(bounds: Rect, clip: Rect) -> Vec<Rect> {
    let mut parts = Vec::new();
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return parts;
    }
    if clip.intersect(&bounds).is_none() {
        parts.push(bounds);
        return parts;
    }

    let top = clip.y.max(bounds.y);
    let bottom = clip.bottom().min(bounds.bottom());

    // Full-width strips above and below the clip.
    if bounds.y < top {
        parts.push(Rect::new(bounds.x, bounds.y, bounds.width, top - bounds.y));
    }
    if bounds.bottom() > bottom {
        parts.push(Rect::new(
            bounds.x,
            bottom,
            bounds.width,
            bounds.bottom() - bottom,
        ));
    }
    // Side strips, limited to the rows the clip spans.
    let band_height = (bottom - top).max(0.0);
    if band_height > 0.0 {
        if bounds.x < clip.x {
            parts.push(Rect::new(
                bounds.x,
                top,
                (clip.x - bounds.x).min(bounds.width),
                band_height,
            ));
        }
        if bounds.right() > clip.right() {
            let x = clip.right().max(bounds.x);
            parts.push(Rect::new(x, top, bounds.right() - x, band_height));
        }
    }
    parts
}

/// Copy a rectangle of pixels out of the buffer.
fn save_region(buffer: &[u8], buf_width: u32, buf_height: u32, rect: Rect) -> Vec<u8> {
    let (x0, y0, x1, y1) = clamp_rect(rect, buf_width, buf_height);
    let mut saved = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
    for y in y0..y1 {
        let row = (y * buf_width + x0) as usize * 4;
        let len = ((x1 - x0) * 4) as usize;
        saved.extend_from_slice(&buffer[row..row + len]);
    }
    saved
}

/// Put a saved rectangle of pixels back.
fn restore_region(buffer: &mut [u8], buf_width: u32, buf_height: u32, rect: Rect, saved: &[u8]) {
    let (x0, y0, x1, y1) = clamp_rect(rect, buf_width, buf_height);
    let stride = ((x1 - x0) * 4) as usize;
    for (i, y) in (y0..y1).enumerate() {
        let row = (y * buf_width + x0) as usize * 4;
        let src = i * stride;
        if src + stride <= saved.len() && row + stride <= buffer.len() {
            buffer[row..row + stride].copy_from_slice(&saved[src..src + stride]);
        }
    }
}

/// A rect as integer pixel bounds inside the buffer.
fn clamp_rect(rect: Rect, buf_width: u32, buf_height: u32) -> (u32, u32, u32, u32) {
    let x0 = rect.x.floor().max(0.0) as u32;
    let y0 = rect.y.floor().max(0.0) as u32;
    let x1 = (rect.right().ceil().max(0.0) as u32).min(buf_width);
    let y1 = (rect.bottom().ceil().max(0.0) as u32).min(buf_height);
    (x0.min(x1), y0.min(y1), x1, y1)
}

/// Paint with everything outside `clip` left untouched.
///
/// None of the painters takes a clip rectangle, and threading one through all
/// of them — solid fills, rounded fills, four border sides with their own dash
/// phases, tiled backgrounds, glyph masks, scaled images — would touch every
/// call site including the browser chrome. Instead the pixels of `bounds` that
/// fall outside `clip` are saved, the paint runs, and those pixels are put back.
///
/// When the item lies entirely inside the clip, which is the ordinary case,
/// there is nothing to save and this costs nothing. `bounds` may be generous:
/// a larger one only means more area is correctly restored.
pub fn with_scissor(
    buffer: &mut [u8],
    buf_width: u32,
    buf_height: u32,
    bounds: Rect,
    clip: Option<Rect>,
    paint: impl FnOnce(&mut [u8]),
) {
    let Some(clip) = clip else {
        paint(buffer);
        return;
    };

    let spill = outside_clip(bounds, clip);
    if spill.is_empty() {
        paint(buffer);
        return;
    }

    let saved: Vec<(Rect, Vec<u8>)> = spill
        .into_iter()
        .map(|rect| (rect, save_region(buffer, buf_width, buf_height, rect)))
        .collect();

    paint(buffer);

    for (rect, pixels) in saved {
        restore_region(buffer, buf_width, buf_height, rect, &pixels);
    }
}

/// Draw the drop arrow of a `<select>`, inside the padding the UA style
/// reserves on its right edge.
pub fn draw_select_arrow(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) {
    if width < 16.0 || height < 8.0 {
        return;
    }
    let color = [96, 96, 96, 255];
    // A solid triangle, drawn as shrinking horizontal runs from a 9px base.
    let half_base = 4.0;
    let cx = x + width - 12.0;
    let cy = y + height / 2.0 - 2.0;

    for row in 0..=(half_base as i32) {
        let run = half_base - row as f32;
        draw_solid_rect(
            dest,
            dest_width,
            dest_height,
            cx - run,
            cy + row as f32,
            run * 2.0 + 1.0,
            1.0,
            color,
        );
    }
}

/// Draw the chrome of a `<video>` or `<audio>` element.
///
/// Nothing here plays anything. What it draws is what a reader needs in order
/// to understand the page: a video is a dark rectangle with a play button in
/// the middle of it, unless a poster frame has already been painted there, and
/// a control bar along the bottom when the markup asked for one.
pub fn draw_media_chrome(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    kind: crate::layout::MediaKind,
    controls: bool,
    has_poster: bool,
) {
    use crate::layout::MediaKind;

    if width < 8.0 || height < 8.0 {
        return;
    }

    let bar_height = (height * 0.28).clamp(20.0, 40.0);
    let is_audio = kind == MediaKind::Audio;

    if is_audio {
        // An audio player is all control bar; there is no picture to show.
        draw_rounded_rect_fill(
            dest,
            dest_width,
            dest_height,
            x,
            y,
            width,
            height,
            (height / 4.0).min(12.0),
            [40, 40, 44, 255],
        );
    } else {
        // A poster frame is the picture, so it must not be blacked out. Without
        // one there is nothing behind the button, and a dark field is what the
        // reader expects to see.
        if !has_poster {
            draw_solid_rect(
                dest,
                dest_width,
                dest_height,
                x,
                y,
                width,
                height,
                [20, 20, 24, 255],
            );
        }
        draw_play_triangle(
            dest,
            dest_width,
            dest_height,
            x + width / 2.0,
            y + (height - if controls { bar_height } else { 0.0 }) / 2.0,
            (width.min(height) * 0.22).clamp(10.0, 44.0),
            [255, 255, 255, 220],
        );
    }

    if !controls {
        return;
    }

    let bar_y = y + height - bar_height;
    if !is_audio {
        // Translucent, so the last rows of the picture still show through.
        draw_solid_rect(
            dest,
            dest_width,
            dest_height,
            x,
            bar_y,
            width,
            bar_height,
            [0, 0, 0, 140],
        );
    }

    let inset = (bar_height * 0.3).max(6.0);
    let button_size = bar_height * 0.34;
    draw_play_triangle(
        dest,
        dest_width,
        dest_height,
        x + inset + button_size / 2.0,
        bar_y + bar_height / 2.0,
        button_size,
        [235, 235, 235, 255],
    );

    // The scrub bar. Nothing is playing, so it sits at the start.
    let track_x = x + inset * 2.0 + button_size;
    let track_width = width - (track_x - x) - inset * 3.0 - button_size;
    if track_width > 4.0 {
        let track_y = bar_y + bar_height / 2.0 - 1.5;
        draw_rounded_rect_fill(
            dest,
            dest_width,
            dest_height,
            track_x,
            track_y,
            track_width,
            3.0,
            1.5,
            [255, 255, 255, 90],
        );
    }

    draw_speaker(
        dest,
        dest_width,
        dest_height,
        x + width - inset - button_size,
        bar_y + bar_height / 2.0,
        button_size,
        [235, 235, 235, 255],
    );
}

/// A play button: a solid right-pointing triangle centred on (cx, cy).
fn draw_play_triangle(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    cx: f32,
    cy: f32,
    size: f32,
    color: [u8; 4],
) {
    if size < 4.0 {
        return;
    }
    let half = size / 2.0;
    let rows = size.round() as i32;
    for row in 0..=rows {
        // The run narrows to a point at the tip, symmetrically about the middle.
        let from_middle = (row as f32 - half).abs();
        let run = (half - from_middle) * (size / half.max(1.0)) * 0.5;
        if run <= 0.0 {
            continue;
        }
        draw_solid_rect(
            dest,
            dest_width,
            dest_height,
            cx - half * 0.5,
            cy - half + row as f32,
            run.max(1.0),
            1.0,
            color,
        );
    }
}

/// A speaker glyph: a small box with a cone opening to the right.
fn draw_speaker(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    cx: f32,
    cy: f32,
    size: f32,
    color: [u8; 4],
) {
    if size < 6.0 {
        return;
    }
    let half = size / 2.0;
    draw_solid_rect(
        dest,
        dest_width,
        dest_height,
        cx - half,
        cy - half * 0.4,
        half * 0.6,
        half * 0.8,
        color,
    );
    let rows = (size * 0.8).round() as i32;
    for row in 0..=rows {
        let spread = (row as f32 / rows.max(1) as f32 - 0.5).abs() * 2.0;
        let run = half * (1.0 - spread);
        if run <= 0.0 {
            continue;
        }
        draw_solid_rect(
            dest,
            dest_width,
            dest_height,
            cx - half * 0.4,
            cy - size * 0.4 + row as f32,
            run,
            1.0,
            color,
        );
    }
}

/// Resolve `background-size` to the pixel size one tile is drawn at.
///
/// Returns `None` when the result would be degenerate (a zero-sized image or
/// box), which the caller treats as "nothing to paint".
pub fn background_tile_size(
    size: BackgroundSize,
    img_w: f32,
    img_h: f32,
    box_w: f32,
    box_h: f32,
) -> Option<(f32, f32)> {
    if img_w <= 0.0 || img_h <= 0.0 || box_w <= 0.0 || box_h <= 0.0 {
        return None;
    }
    let aspect = img_w / img_h;

    let (w, h) = match size {
        BackgroundSize::Auto => (img_w, img_h),
        // Cover scales until neither axis leaves a gap; contain until both fit.
        BackgroundSize::Cover => {
            let scale = (box_w / img_w).max(box_h / img_h);
            (img_w * scale, img_h * scale)
        }
        BackgroundSize::Contain => {
            let scale = (box_w / img_w).min(box_h / img_h);
            (img_w * scale, img_h * scale)
        }
        BackgroundSize::Explicit(sw, sh) => {
            let resolve = |v: BackgroundLength, basis: f32| match v {
                BackgroundLength::Auto => None,
                BackgroundLength::Pixels(px) => Some(px),
                BackgroundLength::Percent(p) => Some(basis * p),
            };
            match (resolve(sw, box_w), resolve(sh, box_h)) {
                (Some(w), Some(h)) => (w, h),
                // One axis `auto` keeps the image's aspect ratio.
                (Some(w), None) => (w, w / aspect),
                (None, Some(h)) => (h * aspect, h),
                (None, None) => (img_w, img_h),
            }
        }
    };

    if w <= 0.0 || h <= 0.0 {
        None
    } else {
        Some((w, h))
    }
}

/// Resolve one axis of `background-position` to a pixel offset from the box's
/// leading edge.
///
/// A percentage aligns *the same point* of the image and the box — 100% puts
/// the image's right edge on the box's right edge — which is why the basis is
/// the leftover space rather than the box width.
pub fn background_position_offset(value: BackgroundLength, box_size: f32, tile_size: f32) -> f32 {
    match value {
        BackgroundLength::Auto => 0.0,
        BackgroundLength::Pixels(px) => px,
        BackgroundLength::Percent(p) => (box_size - tile_size) * p,
    }
}

/// Paint a CSS background image into `dest`, honouring size, position and
/// repeat, and clipped to the element's box.
#[allow(clippy::too_many_arguments)]
pub fn draw_background_image(
    src_rgba: &[u8],
    src_width: u32,
    src_height: u32,
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    box_x: f32,
    box_y: f32,
    box_w: f32,
    box_h: f32,
    size: BackgroundSize,
    position: BackgroundPosition,
    repeat: BackgroundRepeat,
) {
    let Some((tile_w, tile_h)) =
        background_tile_size(size, src_width as f32, src_height as f32, box_w, box_h)
    else {
        return;
    };

    let origin_x = box_x + background_position_offset(position.x, box_w, tile_w);
    let origin_y = box_y + background_position_offset(position.y, box_h, tile_h);
    let (repeat_x, repeat_y) = repeat.axes();

    // Step back to the first tile that is still visible, so a positioned
    // repeating background fills the box on the near side too.
    let first = |origin: f32, box_start: f32, tile: f32, repeats: bool| -> f32 {
        if !repeats || tile <= 0.0 || origin <= box_start {
            origin
        } else {
            origin - ((origin - box_start) / tile).ceil() * tile
        }
    };
    let start_x = first(origin_x, box_x, tile_w, repeat_x);
    let start_y = first(origin_y, box_y, tile_h, repeat_y);

    // A tile smaller than a pixel would loop forever.
    if (repeat_x && tile_w < 1.0) || (repeat_y && tile_h < 1.0) {
        return;
    }

    let mut y = start_y;
    loop {
        let mut x = start_x;
        loop {
            composite_image_clipped(
                src_rgba,
                src_width,
                src_height,
                dest,
                dest_width,
                dest_height,
                x,
                y,
                tile_w,
                tile_h,
                box_x,
                box_y,
                box_w,
                box_h,
            );
            if !repeat_x {
                break;
            }
            x += tile_w;
            if x >= box_x + box_w {
                break;
            }
        }
        if !repeat_y {
            break;
        }
        y += tile_h;
        if y >= box_y + box_h {
            break;
        }
    }
}

/// Composite a scaled image, discarding anything outside the clip rectangle.
///
/// A background tile is positioned relative to the box but must never paint
/// past its edges, so scaling and clipping have to happen together — the
/// source pixel is chosen from the tile's full extent even when only part of
/// that tile lands inside the box.
#[allow(clippy::too_many_arguments)]
fn composite_image_clipped(
    src_rgba: &[u8],
    src_width: u32,
    src_height: u32,
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    tile_x: f32,
    tile_y: f32,
    tile_w: f32,
    tile_h: f32,
    clip_x: f32,
    clip_y: f32,
    clip_w: f32,
    clip_h: f32,
) {
    if src_width == 0 || src_height == 0 || tile_w <= 0.0 || tile_h <= 0.0 {
        return;
    }

    let x_start = tile_x.max(clip_x).max(0.0).floor() as i64;
    let y_start = tile_y.max(clip_y).max(0.0).floor() as i64;
    let x_end = (tile_x + tile_w)
        .min(clip_x + clip_w)
        .min(dest_width as f32)
        .ceil() as i64;
    let y_end = (tile_y + tile_h)
        .min(clip_y + clip_h)
        .min(dest_height as f32)
        .ceil() as i64;

    for dy in y_start..y_end {
        let v = (dy as f32 + 0.5 - tile_y) / tile_h;
        if !(0.0..1.0).contains(&v) {
            continue;
        }
        let sy = ((v * src_height as f32) as usize).min(src_height as usize - 1);

        for dx in x_start..x_end {
            let u = (dx as f32 + 0.5 - tile_x) / tile_w;
            if !(0.0..1.0).contains(&u) {
                continue;
            }
            let sx = ((u * src_width as f32) as usize).min(src_width as usize - 1);

            let src_idx = (sy * src_width as usize + sx) * 4;
            let dst_idx = (dy as usize * dest_width as usize + dx as usize) * 4;
            if src_idx + 3 >= src_rgba.len() || dst_idx + 3 >= dest.len() {
                continue;
            }
            let color = [
                src_rgba[src_idx],
                src_rgba[src_idx + 1],
                src_rgba[src_idx + 2],
                src_rgba[src_idx + 3],
            ];
            blend_pixel(dest, dst_idx, color, 1.0);
        }
    }
}

/// Helper to blend an RGBA pixel into destination buffer.
#[inline]
fn blend_pixel(dest: &mut [u8], dst_idx: usize, color: [u8; 4], alpha_factor: f32) {
    let a = (color[3] as f32 / 255.0) * alpha_factor;
    if a <= 0.0 {
        return;
    }
    let inv_a = 1.0 - a;
    dest[dst_idx] = (color[0] as f32 * a + dest[dst_idx] as f32 * inv_a) as u8;
    dest[dst_idx + 1] = (color[1] as f32 * a + dest[dst_idx + 1] as f32 * inv_a) as u8;
    dest[dst_idx + 2] = (color[2] as f32 * a + dest[dst_idx + 2] as f32 * inv_a) as u8;
    dest[dst_idx + 3] = ((color[3] as f32 * a + dest[dst_idx + 3] as f32 * inv_a).min(255.0)) as u8;
}

/// Draw a solid colored rectangle with alpha blending.
pub fn draw_solid_rect(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [u8; 4],
) {
    if width <= 0.0 || height <= 0.0 || color[3] == 0 {
        return;
    }
    let x_start = (x as i32).max(0) as usize;
    let y_start = (y as i32).max(0) as usize;
    let x_end = ((x + width) as i32).clamp(0, dest_width as i32) as usize;
    let y_end = ((y + height) as i32).clamp(0, dest_height as i32) as usize;

    for dy in y_start..y_end {
        for dx in x_start..x_end {
            let idx = (dy * dest_width as usize + dx) * 4;
            if idx + 3 < dest.len() {
                blend_pixel(dest, idx, color, 1.0);
            }
        }
    }
}

/// Draw a rounded filled rectangle with alpha blending.
pub fn draw_rounded_rect_fill(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    color: [u8; 4],
) {
    if width <= 0.0 || height <= 0.0 || color[3] == 0 {
        return;
    }
    let r = radius.min(width / 2.0).min(height / 2.0);
    if r <= 0.5 {
        draw_solid_rect(dest, dest_width, dest_height, x, y, width, height, color);
        return;
    }

    let x_start = (x as i32).max(0) as usize;
    let y_start = (y as i32).max(0) as usize;
    let x_end = ((x + width) as i32).clamp(0, dest_width as i32) as usize;
    let y_end = ((y + height) as i32).clamp(0, dest_height as i32) as usize;

    let r_sq = r * r;
    let left_corner_cx = x + r;
    let right_corner_cx = x + width - r;
    let top_corner_cy = y + r;
    let bottom_corner_cy = y + height - r;

    for dy in y_start..y_end {
        let py = dy as f32 + 0.5;
        for dx in x_start..x_end {
            let px = dx as f32 + 0.5;

            // Check if pixel is inside rounded corner areas
            let in_top_left = px < left_corner_cx && py < top_corner_cy;
            let in_top_right = px > right_corner_cx && py < top_corner_cy;
            let in_bottom_left = px < left_corner_cx && py > bottom_corner_cy;
            let in_bottom_right = px > right_corner_cx && py > bottom_corner_cy;

            let mut inside = true;
            if in_top_left {
                let dist_sq = (px - left_corner_cx).powi(2) + (py - top_corner_cy).powi(2);
                inside = dist_sq <= r_sq;
            } else if in_top_right {
                let dist_sq = (px - right_corner_cx).powi(2) + (py - top_corner_cy).powi(2);
                inside = dist_sq <= r_sq;
            } else if in_bottom_left {
                let dist_sq = (px - left_corner_cx).powi(2) + (py - bottom_corner_cy).powi(2);
                inside = dist_sq <= r_sq;
            } else if in_bottom_right {
                let dist_sq = (px - right_corner_cx).powi(2) + (py - bottom_corner_cy).powi(2);
                inside = dist_sq <= r_sq;
            }

            if inside {
                let idx = (dy * dest_width as usize + dx) * 4;
                if idx + 3 < dest.len() {
                    blend_pixel(dest, idx, color, 1.0);
                }
            }
        }
    }
}

/// Draw 4-side borders around a rectangle.
pub fn draw_rect_borders(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    borders: [f32; 4], // [top, right, bottom, left]
    colors: [[u8; 4]; 4],
    styles: [BorderStyle; 4],
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let [top_w, right_w, bottom_w, left_w] = borders;

    // Each side spans the full edge, so adjacent sides overlap in the corners.
    // Real browsers mitre the join; painting the full edge is close enough at
    // the widths pages use, and keeps corners filled rather than notched.
    let sides = [
        // (index, x, y, w, h, horizontal?)
        (0usize, x, y, width, top_w, true),
        (1usize, x + width - right_w, y, right_w, height, false),
        (2usize, x, y + height - bottom_w, width, bottom_w, true),
        (3usize, x, y, left_w, height, false),
    ];

    for (i, sx, sy, sw, sh, horizontal) in sides {
        if sw <= 0.0 || sh <= 0.0 || colors[i][3] == 0 || !styles[i].is_visible() {
            continue;
        }
        draw_border_side(
            dest,
            dest_width,
            dest_height,
            sx,
            sy,
            sw,
            sh,
            horizontal,
            styles[i],
            colors[i],
            if horizontal { sh } else { sw },
        );
    }
}

/// Paint one border edge in its declared style.
///
/// `thickness` is the edge's own width — the dash and dot geometry is derived
/// from it, as CSS specifies, so a thick border gets proportionally long
/// dashes rather than a fine stipple.
#[allow(clippy::too_many_arguments)]
fn draw_border_side(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    horizontal: bool,
    style: BorderStyle,
    color: [u8; 4],
    thickness: f32,
) {
    match style {
        BorderStyle::None | BorderStyle::Hidden => {}
        BorderStyle::Dashed | BorderStyle::Dotted => {
            // Dashes are 3× the border thickness with a gap of the same size;
            // dots are square segments of one thickness.
            let (seg, gap) = if style == BorderStyle::Dotted {
                (thickness, thickness)
            } else {
                (thickness * 3.0, thickness * 2.0)
            };
            let span = if horizontal { w } else { h };
            let step = seg + gap;
            if step <= 0.0 {
                return;
            }
            let mut offset = 0.0;
            while offset < span {
                let len = seg.min(span - offset);
                if horizontal {
                    draw_solid_rect(dest, dest_width, dest_height, x + offset, y, len, h, color);
                } else {
                    draw_solid_rect(dest, dest_width, dest_height, x, y + offset, w, len, color);
                }
                offset += step;
            }
        }
        BorderStyle::Double => {
            // Three equal bands: line, gap, line. Below 3px there is no room to
            // split, so it degrades to solid — as browsers do.
            let band = (thickness / 3.0).floor();
            if band < 1.0 {
                draw_solid_rect(dest, dest_width, dest_height, x, y, w, h, color);
                return;
            }
            if horizontal {
                draw_solid_rect(dest, dest_width, dest_height, x, y, w, band, color);
                draw_solid_rect(
                    dest,
                    dest_width,
                    dest_height,
                    x,
                    y + h - band,
                    w,
                    band,
                    color,
                );
            } else {
                draw_solid_rect(dest, dest_width, dest_height, x, y, band, h, color);
                draw_solid_rect(
                    dest,
                    dest_width,
                    dest_height,
                    x + w - band,
                    y,
                    band,
                    h,
                    color,
                );
            }
        }
        _ => draw_solid_rect(dest, dest_width, dest_height, x, y, w, h, color),
    }
}

/// Render an SVG string into an RGBA bitmap at target width and height using resvg.
pub fn render_svg_to_rgba(
    svg_data: &str,
    target_width: f32,
    target_height: f32,
) -> Option<(Vec<u8>, u32, u32)> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg_data, &opt).ok()?;
    let svg_w = tree.size().width();
    let svg_h = tree.size().height();

    let fit_w = if target_width > 0.0 {
        target_width
    } else {
        svg_w
    };
    let fit_h = if target_height > 0.0 {
        target_height
    } else {
        svg_h
    };

    let pixmap_w = fit_w.round().max(1.0) as u32;
    let pixmap_h = fit_h.round().max(1.0) as u32;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixmap_w, pixmap_h)?;

    let sx = pixmap_w as f32 / svg_w;
    let sy = pixmap_h as f32 / svg_h;
    let transform = resvg::tiny_skia::Transform::from_scale(sx, sy);

    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some((pixmap.take(), pixmap_w, pixmap_h))
}

/// Draw a line (horizontal underline) with alpha blending.
pub fn draw_underline(
    dest: &mut [u8],
    dest_width: u32,
    dest_height: u32,
    x: f32,
    y: f32,
    width: f32,
    thickness: f32,
    color: [u8; 4],
) {
    draw_solid_rect(
        dest,
        dest_width,
        dest_height,
        x,
        y,
        width,
        thickness.max(1.0),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_to_clip_center() {
        // (400, 320) on 1000×800 → clip (-0.2, +0.2) since 320/800 = 0.4 above center
        let rect = layout_to_clip(400.0, 320.0, 200.0, 160.0, 1000.0, 800.0);
        assert!((rect.x - (-0.2)).abs() < 0.001);
        assert!((rect.y - (0.2)).abs() < 0.001);
        assert!((rect.width - 0.4).abs() < 0.001);
        assert!((rect.height - 0.4).abs() < 0.001);
    }

    #[test]
    fn layout_to_clip_full_screen() {
        // Full viewport (0,0) → clip (-1, +1), size (2, 2)
        // Note: layout top (y=0) maps to clip top (y=+1) because Y is flipped
        let rect = layout_to_clip(0.0, 0.0, 800.0, 600.0, 800.0, 600.0);
        assert!((rect.x - (-1.0)).abs() < 0.001);
        assert!((rect.y - (1.0)).abs() < 0.001);
        assert!((rect.width - 2.0).abs() < 0.001);
        assert!((rect.height - 2.0).abs() < 0.001);
    }

    #[test]
    fn layout_to_clip_top_left() {
        let rect = layout_to_clip(0.0, 0.0, 100.0, 100.0, 1000.0, 1000.0);
        assert!((rect.x - (-1.0)).abs() < 0.001);
        assert!((rect.y - (1.0)).abs() < 0.001);
        assert!((rect.width - 0.2).abs() < 0.001);
        assert!((rect.height - 0.2).abs() < 0.001);
    }

    #[test]
    fn color_u8_to_f32_red() {
        let color = color_u8_to_f32([255, 0, 0, 255]);
        assert!((color.r - 1.0).abs() < 0.001);
        assert!((color.g - 0.0).abs() < 0.001);
        assert!((color.b - 0.0).abs() < 0.001);
        assert!((color.a - 1.0).abs() < 0.001);
    }

    #[test]
    fn color_u8_to_f32_transparent() {
        let color = color_u8_to_f32([0, 0, 0, 0]);
        assert!((color.r - 0.0).abs() < 0.001);
        assert!((color.a - 0.0).abs() < 0.001);
    }

    #[test]
    fn color_u8_to_f32_white() {
        let color = color_u8_to_f32([255, 255, 255, 255]);
        assert!((color.r - 1.0).abs() < 0.001);
        assert!((color.g - 1.0).abs() < 0.001);
        assert!((color.b - 1.0).abs() < 0.001);
    }

    #[test]
    fn composite_image_basic() {
        // 4x4 destination buffer, all zeros
        let mut dest = vec![0u8; 4 * 4 * 4];
        // 2x2 source image: solid red
        let src = [
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];

        composite_image(&src, 2, 2, &mut dest, 4, 4, 1.0, 1.0);

        let idx = (1 * 4 + 1) * 4;
        assert_eq!(dest[idx], 255);
        assert_eq!(dest[idx + 1], 0);
        assert_eq!(dest[idx + 2], 0);
        assert_eq!(dest[idx + 3], 255);

        let idx = 0;
        assert_eq!(dest[idx], 0);
    }

    #[test]
    fn composite_image_alpha_blend() {
        let mut dest = vec![0u8; 2 * 2 * 4];
        // Semi-transparent green (alpha = 128 ~ 0.5)
        let src = [
            0, 255, 0, 128, 0, 255, 0, 128, 0, 255, 0, 128, 0, 255, 0, 128,
        ];

        composite_image(&src, 2, 2, &mut dest, 2, 2, 0.0, 0.0);

        // G = 255 * 0.5 + 0 * 0.5 ~ 127
        assert_eq!(dest[0], 0);
        assert!((dest[1] as i32 - 127).abs() <= 1);
    }

    #[test]
    fn composite_image_clips_out_of_bounds() {
        let mut dest = vec![0u8; 2 * 2 * 4];
        let src = vec![255u8; 4 * 4 * 4];

        composite_image(&src, 4, 4, &mut dest, 2, 2, 0.0, 0.0);

        for chunk in dest.chunks(4) {
            assert_eq!(chunk[0], 255);
        }
    }

    #[test]
    fn composite_image_negative_offset_clips() {
        let mut dest = vec![0u8; 4 * 4 * 4];
        let src = vec![255u8; 4 * 4 * 4];

        composite_image(&src, 4, 4, &mut dest, 4, 4, -1.0, -1.0);

        // Source pixel (1,1) maps to dest(0,0) — should be painted
        let idx = 0;
        assert_eq!(dest[idx], 255, "dest(0,0) gets source(1,1)");

        // Source pixel (3,3) maps to dest(2,2) — should be painted
        let idx = (2 * 4 + 2) * 4;
        assert_eq!(dest[idx], 255, "dest(2,2) gets source(3,3)");

        // The last row (dest y=3) has no mapped source — should remain 0
        let idx = (3 * 4 + 0) * 4;
        assert_eq!(dest[idx], 0, "dest(3,0) is out of image bounds");

        // The last column (dest x=3) has no mapped source — should remain 0
        let idx = 3 * 4;
        assert_eq!(dest[idx], 0, "dest(0,3) is out of image bounds");
    }

    #[test]
    fn test_draw_solid_rect() {
        let mut dest = vec![0u8; 10 * 10 * 4];
        let color = [255, 0, 0, 255];
        draw_solid_rect(&mut dest, 10, 10, 2.0, 2.0, 4.0, 4.0, color);

        // (2,2) should be red
        let idx = (2 * 10 + 2) * 4;
        assert_eq!(&dest[idx..idx + 4], &[255, 0, 0, 255]);

        // (0,0) should be untouched (transparent black)
        assert_eq!(&dest[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn test_draw_rect_borders() {
        let mut dest = vec![0u8; 10 * 10 * 4];
        let color = [0, 0, 255, 255];
        // 1px border on all 4 sides of a 6x6 rect at (2,2)
        draw_rect_borders(
            &mut dest,
            10,
            10,
            2.0,
            2.0,
            6.0,
            6.0,
            [1.0, 1.0, 1.0, 1.0],
            [color; 4],
            [BorderStyle::Solid; 4],
        );

        // Top border at (2,2)
        let top_idx = (2 * 10 + 2) * 4;
        assert_eq!(&dest[top_idx..top_idx + 4], &[0, 0, 255, 255]);

        // Center at (4,4) should be untouched
        let center_idx = (4 * 10 + 4) * 4;
        assert_eq!(&dest[center_idx..center_idx + 4], &[0, 0, 0, 0]);
    }

    /// Whether the top-edge pixel at `x` was painted.
    fn top_edge_painted(dest: &[u8], stride: usize, x: usize) -> bool {
        dest[x * 4 + 3] != 0
    }

    #[test]
    fn a_box_fully_inside_the_clip_needs_no_saving() {
        let bounds = Rect::new(10.0, 10.0, 20.0, 20.0);
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(
            outside_clip(bounds, clip).is_empty(),
            "the ordinary case must cost nothing"
        );
    }

    #[test]
    fn a_box_fully_outside_the_clip_is_entirely_spill() {
        let bounds = Rect::new(200.0, 200.0, 20.0, 20.0);
        let clip = Rect::new(0.0, 0.0, 100.0, 100.0);
        let spill = outside_clip(bounds, clip);
        assert_eq!(spill.len(), 1);
        assert_eq!(spill[0].width, 20.0);
    }

    #[test]
    fn the_spill_of_a_partly_clipped_box_covers_exactly_what_is_outside() {
        // A 100-wide box in a 40-wide clip: 60 wide of spill on the right.
        let bounds = Rect::new(0.0, 0.0, 100.0, 10.0);
        let clip = Rect::new(0.0, 0.0, 40.0, 10.0);
        let spill = outside_clip(bounds, clip);
        let area: f32 = spill.iter().map(|r| r.width * r.height).sum();
        assert_eq!(area, 60.0 * 10.0, "spill: {spill:?}");
    }

    #[test]
    fn the_scissor_keeps_paint_inside_the_clip() {
        let mut dest = vec![0u8; 20 * 20 * 4];
        let clip = Rect::new(0.0, 0.0, 10.0, 20.0);

        // Paint a full-width bar; only the left half may survive.
        with_scissor(
            &mut dest,
            20,
            20,
            Rect::new(0.0, 0.0, 20.0, 4.0),
            Some(clip),
            |buf| {
                draw_solid_rect(buf, 20, 20, 0.0, 0.0, 20.0, 4.0, [255, 0, 0, 255]);
            },
        );

        let px = |x: usize, y: usize| dest[(y * 20 + x) * 4 + 3];
        assert_ne!(px(0, 0), 0, "inside the clip is painted");
        assert_ne!(px(9, 3), 0, "up to the clip edge is painted");
        assert_eq!(px(10, 0), 0, "past the clip edge is not");
        assert_eq!(px(19, 3), 0, "nor is the far end");
    }

    #[test]
    fn the_scissor_leaves_earlier_paint_outside_the_clip_intact() {
        // The spill is restored, not cleared: whatever was already there must
        // come back untouched.
        let mut dest = vec![0u8; 20 * 20 * 4];
        draw_solid_rect(&mut dest, 20, 20, 0.0, 0.0, 20.0, 4.0, [0, 0, 255, 255]);

        let clip = Rect::new(0.0, 0.0, 10.0, 20.0);
        with_scissor(
            &mut dest,
            20,
            20,
            Rect::new(0.0, 0.0, 20.0, 4.0),
            Some(clip),
            |buf| {
                draw_solid_rect(buf, 20, 20, 0.0, 0.0, 20.0, 4.0, [255, 0, 0, 255]);
            },
        );

        let px = |x: usize| dest[x * 4..x * 4 + 4].to_vec();
        assert_eq!(px(0), vec![255, 0, 0, 255], "inside: overpainted");
        assert_eq!(
            px(15),
            vec![0, 0, 255, 255],
            "outside: the earlier blue kept"
        );
    }

    #[test]
    fn no_clip_paints_everything() {
        let mut dest = vec![0u8; 20 * 20 * 4];
        with_scissor(
            &mut dest,
            20,
            20,
            Rect::new(0.0, 0.0, 20.0, 4.0),
            None,
            |buf| {
                draw_solid_rect(buf, 20, 20, 0.0, 0.0, 20.0, 4.0, [255, 0, 0, 255]);
            },
        );
        assert_ne!(dest[19 * 4 + 3], 0);
    }

    #[test]
    fn background_size_cover_fills_the_box_contain_fits_inside_it() {
        // A 100x50 image in a 200x200 box.
        let cover = background_tile_size(BackgroundSize::Cover, 100.0, 50.0, 200.0, 200.0).unwrap();
        assert_eq!(cover, (400.0, 200.0), "cover leaves no gap on either axis");

        let contain =
            background_tile_size(BackgroundSize::Contain, 100.0, 50.0, 200.0, 200.0).unwrap();
        assert_eq!(contain, (200.0, 100.0), "contain fits entirely inside");
    }

    #[test]
    fn background_size_auto_axis_keeps_the_aspect_ratio() {
        let size = BackgroundSize::Explicit(BackgroundLength::Pixels(50.0), BackgroundLength::Auto);
        let (w, h) = background_tile_size(size, 100.0, 50.0, 200.0, 200.0).unwrap();
        assert_eq!((w, h), (50.0, 25.0));

        // Percentages resolve against the box, not the image.
        let pct = BackgroundSize::Explicit(
            BackgroundLength::Percent(0.5),
            BackgroundLength::Percent(0.25),
        );
        assert_eq!(
            background_tile_size(pct, 100.0, 50.0, 200.0, 400.0).unwrap(),
            (100.0, 100.0)
        );
    }

    #[test]
    fn background_position_percent_aligns_matching_edges() {
        // 100% must put the image's right edge on the box's right edge, so the
        // basis is the leftover space rather than the box width.
        assert_eq!(
            background_position_offset(BackgroundLength::Percent(1.0), 200.0, 50.0),
            150.0
        );
        assert_eq!(
            background_position_offset(BackgroundLength::Percent(0.5), 200.0, 50.0),
            75.0
        );
        assert_eq!(
            background_position_offset(BackgroundLength::Pixels(12.0), 200.0, 50.0),
            12.0
        );
    }

    /// A 2x2 fully opaque image: red, green / blue, white.
    fn tiny_image() -> Vec<u8> {
        vec![
            255, 0, 0, 255, // (0,0) red
            0, 255, 0, 255, // (1,0) green
            0, 0, 255, 255, // (0,1) blue
            255, 255, 255, 255, // (1,1) white
        ]
    }

    #[test]
    fn no_repeat_background_paints_one_tile_only() {
        let mut dest = vec![0u8; 10 * 10 * 4];
        draw_background_image(
            &tiny_image(),
            2,
            2,
            &mut dest,
            10,
            10,
            0.0,
            0.0,
            10.0,
            10.0,
            BackgroundSize::Auto,
            BackgroundPosition::default(),
            BackgroundRepeat::NoRepeat,
        );

        let alpha = |x: usize, y: usize| dest[(y * 10 + x) * 4 + 3];
        assert_ne!(alpha(0, 0), 0, "the single tile is painted at the origin");
        assert_ne!(alpha(1, 1), 0);
        assert_eq!(alpha(3, 3), 0, "nothing repeats past the 2x2 tile");
    }

    #[test]
    fn repeat_background_tiles_across_the_whole_box() {
        let mut dest = vec![0u8; 8 * 8 * 4];
        draw_background_image(
            &tiny_image(),
            2,
            2,
            &mut dest,
            8,
            8,
            0.0,
            0.0,
            8.0,
            8.0,
            BackgroundSize::Auto,
            BackgroundPosition::default(),
            BackgroundRepeat::Repeat,
        );

        assert!(
            dest.chunks(4).all(|px| px[3] != 0),
            "every pixel of the box is covered"
        );
        // The pattern repeats with a period of 2px.
        let px = |x: usize, y: usize| dest[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].to_vec();
        assert_eq!(px(0, 0), px(2, 0));
        assert_eq!(px(1, 1), px(7, 7));
    }

    #[test]
    fn repeat_x_leaves_the_vertical_axis_alone() {
        let mut dest = vec![0u8; 8 * 8 * 4];
        draw_background_image(
            &tiny_image(),
            2,
            2,
            &mut dest,
            8,
            8,
            0.0,
            0.0,
            8.0,
            8.0,
            BackgroundSize::Auto,
            BackgroundPosition::default(),
            BackgroundRepeat::RepeatX,
        );

        let alpha = |x: usize, y: usize| dest[(y * 8 + x) * 4 + 3];
        assert_ne!(alpha(6, 0), 0, "tiles run along x");
        assert_eq!(alpha(0, 4), 0, "but not down y");
    }

    #[test]
    fn background_never_paints_outside_its_box() {
        let mut dest = vec![0u8; 20 * 20 * 4];
        // A 10x10 box at (5,5) with a tile larger than the box.
        draw_background_image(
            &tiny_image(),
            2,
            2,
            &mut dest,
            20,
            20,
            5.0,
            5.0,
            10.0,
            10.0,
            BackgroundSize::Explicit(
                BackgroundLength::Pixels(30.0),
                BackgroundLength::Pixels(30.0),
            ),
            BackgroundPosition::default(),
            BackgroundRepeat::Repeat,
        );

        let alpha = |x: usize, y: usize| dest[(y * 20 + x) * 4 + 3];
        assert_ne!(alpha(6, 6), 0, "inside the box");
        assert_eq!(alpha(4, 6), 0, "left of the box stays clear");
        assert_eq!(alpha(15, 6), 0, "right of the box stays clear");
        assert_eq!(alpha(6, 16), 0, "below the box stays clear");
    }

    #[test]
    fn positioned_repeat_still_covers_the_near_edge() {
        // With the origin pushed 3px in, the tiles must also step back to fill
        // the space before it.
        let mut dest = vec![0u8; 12 * 4 * 4];
        draw_background_image(
            &tiny_image(),
            2,
            2,
            &mut dest,
            12,
            4,
            0.0,
            0.0,
            12.0,
            4.0,
            BackgroundSize::Auto,
            BackgroundPosition {
                x: BackgroundLength::Pixels(3.0),
                y: BackgroundLength::Pixels(0.0),
            },
            BackgroundRepeat::Repeat,
        );

        let alpha = |x: usize, y: usize| dest[(y * 12 + x) * 4 + 3];
        assert_ne!(alpha(0, 0), 0, "the gap before the origin is filled");
        assert_ne!(alpha(11, 0), 0, "and the far edge too");
    }

    #[test]
    fn dotted_border_leaves_gaps_along_the_edge() {
        let mut dest = vec![0u8; 20 * 20 * 4];
        draw_rect_borders(
            &mut dest,
            20,
            20,
            0.0,
            0.0,
            20.0,
            20.0,
            [1.0, 0.0, 0.0, 0.0],
            [[0, 0, 0, 255]; 4],
            [BorderStyle::Dotted; 4],
        );

        // 1px thickness → 1px on, 1px off.
        assert!(top_edge_painted(&dest, 20, 0), "first dot is painted");
        assert!(!top_edge_painted(&dest, 20, 1), "gap follows the dot");
        assert!(top_edge_painted(&dest, 20, 2), "second dot is painted");
    }

    #[test]
    fn dashed_border_draws_longer_runs_than_dotted() {
        let mut dest = vec![0u8; 40 * 20 * 4];
        draw_rect_borders(
            &mut dest,
            40,
            20,
            0.0,
            0.0,
            40.0,
            20.0,
            [2.0, 0.0, 0.0, 0.0],
            [[0, 0, 0, 255]; 4],
            [BorderStyle::Dashed; 4],
        );

        // 2px thickness → 6px dash, 4px gap.
        for x in 0..6 {
            assert!(top_edge_painted(&dest, 40, x), "dash pixel {x}");
        }
        for x in 6..10 {
            assert!(!top_edge_painted(&dest, 40, x), "gap pixel {x}");
        }
        assert!(top_edge_painted(&dest, 40, 10), "next dash starts");
    }

    #[test]
    fn double_border_splits_into_two_bands() {
        let mut dest = vec![0u8; 20 * 20 * 4];
        draw_rect_borders(
            &mut dest,
            20,
            20,
            0.0,
            0.0,
            20.0,
            20.0,
            [6.0, 0.0, 0.0, 0.0],
            [[0, 0, 0, 255]; 4],
            [BorderStyle::Double; 4],
        );

        let row_painted = |y: usize| dest[(y * 20) * 4 + 3] != 0;
        assert!(row_painted(0), "outer band");
        assert!(row_painted(1), "outer band is 2px for a 6px border");
        assert!(!row_painted(2), "gap between the bands");
        assert!(!row_painted(3), "gap between the bands");
        assert!(row_painted(4), "inner band");
        assert!(row_painted(5), "inner band");
    }

    #[test]
    fn border_style_none_paints_nothing() {
        let mut dest = vec![0u8; 10 * 10 * 4];
        draw_rect_borders(
            &mut dest,
            10,
            10,
            0.0,
            0.0,
            10.0,
            10.0,
            [2.0; 4],
            [[255, 0, 0, 255]; 4],
            [BorderStyle::None; 4],
        );
        assert!(dest.iter().all(|&b| b == 0));
    }

    #[test]
    fn each_side_paints_in_its_own_colour() {
        let mut dest = vec![0u8; 10 * 10 * 4];
        draw_rect_borders(
            &mut dest,
            10,
            10,
            0.0,
            0.0,
            10.0,
            10.0,
            [1.0; 4],
            [
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255, 255, 0, 255],
            ],
            [BorderStyle::Solid; 4],
        );

        // Sample the middle of each edge to stay clear of the corner overlaps.
        let px = |x: usize, y: usize| dest[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4].to_vec();
        assert_eq!(px(5, 0), vec![255, 0, 0, 255], "top");
        assert_eq!(px(9, 5), vec![0, 255, 0, 255], "right");
        assert_eq!(px(5, 9), vec![0, 0, 255, 255], "bottom");
        assert_eq!(px(0, 5), vec![255, 255, 0, 255], "left");
    }

    #[test]
    fn test_draw_rounded_rect_fill() {
        let mut dest = vec![0u8; 20 * 20 * 4];
        let color = [0, 255, 0, 255];
        draw_rounded_rect_fill(&mut dest, 20, 20, 0.0, 0.0, 20.0, 20.0, 5.0, color);

        // Center at (10,10) should be filled
        let center_idx = (10 * 20 + 10) * 4;
        assert_eq!(&dest[center_idx..center_idx + 4], &[0, 255, 0, 255]);

        // Extreme corner at (0,0) should be clipped by corner radius
        assert_eq!(&dest[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn test_draw_underline() {
        let mut dest = vec![0u8; 10 * 10 * 4];
        let color = [255, 255, 0, 255];
        draw_underline(&mut dest, 10, 10, 1.0, 5.0, 8.0, 1.0, color);

        let line_idx = (5 * 10 + 2) * 4;
        assert_eq!(&dest[line_idx..line_idx + 4], &[255, 255, 0, 255]);
    }
}
