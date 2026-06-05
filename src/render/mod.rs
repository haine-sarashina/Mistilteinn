/// GPU rendering pipeline using wgpu.
///
/// Renders colored rectangles (layout boxes) and text to the window surface.
///
/// Pipeline stages:
/// 1. Initialize wgpu (instance, adapter, device, surface)
/// 2. Create render pipeline (vertex + fragment shaders)
/// 3. For each frame: clear → draw rectangles → draw text → present
pub mod text;

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
    /// Current number of rectangles to draw (max 64).
    num_rects: u32,
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
            present_mode: wgpu::PresentMode::Mailbox,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };
        surface.configure(&device, &config);

        // Create render pipeline
        let rect_pipeline = Self::create_rect_pipeline(&device, surface_format);

        // Create uniform buffers (64 rects × vec4 = 256 f32)
        let uniform_size = std::mem::size_of::<[f32; 256]>() as u64;

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
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
                cull_mode: Some(wgpu::Face::Back),
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
        let count = rects.len().min(64);
        self.num_rects = count as u32;

        // Upload positions as vec4 array [x, y, w, h] × 64
        let mut position_data = [0.0f32; 256];
        for (i, rect) in rects.iter().take(64).enumerate() {
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

        // Upload colors as vec4 array [r, g, b, a] × 64
        let mut color_data = [0.0f32; 256];
        for (i, color) in colors.iter().take(64).enumerate() {
            color_data[i * 4] = color.r;
            color_data[i * 4 + 1] = color.g;
            color_data[i * 4 + 2] = color.b;
            color_data[i * 4 + 3] = color.a;
        }
        self.queue
            .write_buffer(&self.rect_color_buffer, 0, bytemuck::bytes_of(&color_data));
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
            rects: array<vec4f, 64>,
        }

        struct RectColor {
            colors: array<vec4f, 64>,
        }

        @group(0) @binding(0)
        var<uniform> rect_data: RectUniform;

        @group(0) @binding(1)
        var<uniform> rect_colors: RectColor;

        @vertex
        fn vs_main(
            @builtin(vertex_index) vertex_index: u32,
        ) -> @builtin(position) vec4f {
            // 6 vertices per rectangle (2 triangles)
            let tri = vertex_index % 6u;
            let rect_idx = (vertex_index / 6u) * 2u + tri / 2u;
            if rect_idx > 63u {
                return vec4f(0.0, 0.0, 0.0, 1.0);
            }
            let r = rect_data.rects[rect_idx];
            // Two triangles per rect
            let offsets = array<vec2f, 3>(
                vec2f(0.0, 0.0),
                vec2f(1.0, 0.0),
                vec2f(1.0, 1.0),
            );
            let offset2 = array<vec2f, 3>(
                vec2f(0.0, 0.0),
                vec2f(1.0, 1.0),
                vec2f(0.0, 1.0),
            );
            let off = if tri < 3u { offsets[tri] } else { offset2[tri - 3u] };
            let px = r.x + off.x * r.z;
            let py = r.y + off.y * r.w;
            return vec4f(px, py, 0.0, 1.0);
        }
    "#;

    /// Fragment shader that outputs the rectangle color.
    pub const RECT_FRAGMENT: &str = r#"
        struct RectColor {
            colors: array<vec4f, 64>,
        }

        @group(0) @binding(1)
        var<uniform> rect_colors: RectColor;

        @fragment
        fn fs_main(
            @builtin(vertex_index) vertex_index: u32,
        ) -> @location(0) vec4f {
            let tri = vertex_index % 6u;
            let rect_idx = (vertex_index / 6u) * 2u + tri / 2u;
            if rect_idx > 63u {
                return vec4f(1.0, 1.0, 1.0, 1.0);
            }
            return rect_colors.colors[rect_idx];
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
}
