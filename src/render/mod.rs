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
    /// Current number of rectangles to draw (max 512).
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
    /// Vertex buffer for a fullscreen quad (2 triangles).
    quad_vertex_buffer: wgpu::Buffer,
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

        // Create uniform buffers (512 rects × vec4 = 2048 f32)
        let uniform_size = std::mem::size_of::<[f32; 2048]>() as u64;

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

        // Create fullscreen quad vertex buffer
        // Vertex format: [x, y, uv_x, uv_y] as f32 — 6 vertices for 2 triangles
        let quad_vertices: [f32; 24] = [
            // Triangle 1
            -1.0, -1.0, 0.0, 0.0, -1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, // Triangle 2
            -1.0, -1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 0.0,
        ];
        let quad_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Quad Vertex Buffer"),
            size: quad_vertices.len() as u64 * std::mem::size_of::<f32>() as u64,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        queue.write_buffer(&quad_vertex_buffer, 0, bytemuck::cast_slice(&quad_vertices));

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
            quad_vertex_buffer,
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
        let count = rects.len().min(512);
        self.num_rects = count as u32;

        // Upload positions as vec4 array [x, y, w, h] × 512
        let mut position_data = [0.0f32; 2048];
        for (i, rect) in rects.iter().take(512).enumerate() {
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

        // Upload colors as vec4 array [r, g, b, a] × 512
        let mut color_data = [0.0f32; 2048];
        for (i, color) in colors.iter().take(512).enumerate() {
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
            rects: array<vec4f, 512>,
        }

        struct RectColor {
            colors: array<vec4f, 512>,
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
            if rect_idx > 511u {
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
            colors: array<vec4f, 512>,
        }

        @group(0) @binding(1)
        var<uniform> rect_colors: RectColor;

        @fragment
        fn fs_main(
            @builtin(vertex_index) vertex_index: u32,
        ) -> @location(0) vec4f {
            let tri = vertex_index % 6u;
            let rect_idx = (vertex_index / 6u) * 2u + tri / 2u;
            if rect_idx > 511u {
                return vec4f(1.0, 1.0, 1.0, 1.0);
            }
            return rect_colors.colors[rect_idx];
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
            var positions = array<vec2f, 6>(
                vec2f(-1.0, -1.0), vec2f(-1.0, 1.0), vec2f(1.0, 1.0),
                vec2f(-1.0, -1.0), vec2f(1.0, 1.0), vec2f(1.0, -1.0),
            );
            var uvs = array<vec2f, 6>(
                vec2f(0.0, 0.0), vec2f(0.0, 1.0), vec2f(1.0, 1.0),
                vec2f(0.0, 0.0), vec2f(1.0, 1.0), vec2f(1.0, 0.0),
            );
            var out: VertexOutput;
            out.position = vec4f(positions[vertex_index], 0.0, 1.0);
            out.uv = uvs[vertex_index];
            return out;
        }
    "#;

    /// Fragment shader for the text overlay — samples the text texture with alpha blending.
    pub const TEXT_FRAGMENT: &str = r#"
        @group(0) @binding(0) var text_tex: texture_2d<f32>;
        @group(0) @binding(1) var text_smp: sampler;

        @fragment
        fn fs_text_quad(@location(0) uv: vec2f) -> @location(0) vec4f {
            let tex_uv = vec2f(1.0 - uv.x, 1.0 - uv.y); // Flip to match layout coords
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
    let dx_base = dest_x as i32;
    let dy_base = dest_y as i32;

    for src_y in 0..src_height as i32 {
        let d_y = dy_base + src_y;
        if d_y < 0 || d_y >= dest_height as i32 {
            continue;
        }

        for src_x in 0..src_width as i32 {
            let d_x = dx_base + src_x;
            if d_x < 0 || d_x >= dest_width as i32 {
                continue;
            }

            let src_idx = ((src_y as usize) * src_width as usize + (src_x as usize)) * 4;
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

        let idx = (0 * 4 + 0) * 4;
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
        let idx = (0 * 4 + 0) * 4;
        assert_eq!(dest[idx], 255, "dest(0,0) gets source(1,1)");

        // Source pixel (3,3) maps to dest(2,2) — should be painted
        let idx = (2 * 4 + 2) * 4;
        assert_eq!(dest[idx], 255, "dest(2,2) gets source(3,3)");

        // The last row (dest y=3) has no mapped source — should remain 0
        let idx = (3 * 4 + 0) * 4;
        assert_eq!(dest[idx], 0, "dest(3,0) is out of image bounds");

        // The last column (dest x=3) has no mapped source — should remain 0
        let idx = (0 * 4 + 3) * 4;
        assert_eq!(dest[idx], 0, "dest(0,3) is out of image bounds");
    }
}
