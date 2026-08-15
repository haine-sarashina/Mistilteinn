/// GPU rendering pipeline using wgpu.
///
/// Renders colored rectangles (layout boxes) and text to the window surface.
///
/// Pipeline stages:
/// 1. Initialize wgpu (instance, adapter, device, surface)
/// 2. Create render pipeline (vertex + fragment shaders)
/// 3. For each frame: clear → draw rectangles → draw text → present
pub mod text;

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
    color: [u8; 4],
) {
    if width <= 0.0 || height <= 0.0 || color[3] == 0 {
        return;
    }
    let [top_w, right_w, bottom_w, left_w] = borders;

    // Top border
    if top_w > 0.0 {
        draw_solid_rect(dest, dest_width, dest_height, x, y, width, top_w, color);
    }
    // Bottom border
    if bottom_w > 0.0 {
        draw_solid_rect(
            dest,
            dest_width,
            dest_height,
            x,
            y + height - bottom_w,
            width,
            bottom_w,
            color,
        );
    }
    // Left border
    if left_w > 0.0 {
        draw_solid_rect(dest, dest_width, dest_height, x, y, left_w, height, color);
    }
    // Right border
    if right_w > 0.0 {
        draw_solid_rect(
            dest,
            dest_width,
            dest_height,
            x + width - right_w,
            y,
            right_w,
            height,
            color,
        );
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
            color,
        );

        // Top border at (2,2)
        let top_idx = (2 * 10 + 2) * 4;
        assert_eq!(&dest[top_idx..top_idx + 4], &[0, 0, 255, 255]);

        // Center at (4,4) should be untouched
        let center_idx = (4 * 10 + 4) * 4;
        assert_eq!(&dest[center_idx..center_idx + 4], &[0, 0, 0, 0]);
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
