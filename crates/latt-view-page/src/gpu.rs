//! The GPU: layout in compute passes, drawing, and reading places back for picking.

use std::cell::Cell;
use std::rc::Rc;
use wasm_bindgen::JsValue;
use wgpu::util::DeviceExt;

const GRID: u32 = 512;
const GROUP_SIZE: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    nodes: u32,
    edges: u32,
    grid: u32,
    focused: u32,
    aspect: f32,
    zoom: f32,
    centre_x: f32,
    centre_y: f32,
}

/// Where the camera looks.
#[derive(Clone, Copy)]
pub struct Camera {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

/// How far a read of places has come.
#[derive(Clone, Copy, PartialEq)]
enum Reading {
    Idle,
    Copied,
    Waiting,
}

/// What belongs to one snapshot.
struct Loaded {
    nodes: u32,
    edges: u32,
    node_buffer: wgpu::Buffer,
    flag_buffer: wgpu::Buffer,
    heat_buffer: wgpu::Buffer,
    staging: wgpu::Buffer,
    groups: [wgpu::BindGroup; 4],
    fade_group: wgpu::BindGroup,
    edge_group: wgpu::BindGroup,
    node_group: wgpu::BindGroup,
    reading: Reading,
    ready: Rc<Cell<bool>>,
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    params: wgpu::Buffer,
    density: wgpu::Buffer,
    layout: [wgpu::ComputePipeline; 4],
    fade: wgpu::ComputePipeline,
    edge_pipeline: wgpu::RenderPipeline,
    node_pipeline: wgpu::RenderPipeline,
    loaded: Option<Loaded>,
}

fn failure(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn binding<'a>(entries: &[(u32, &'a wgpu::Buffer)]) -> Vec<wgpu::BindGroupEntry<'a>> {
    entries
        .iter()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: *binding,
            resource: buffer.as_entire_binding(),
        })
        .collect()
}

impl Gpu {
    pub async fn new(canvas: web_sys::HtmlCanvasElement) -> Result<Self, JsValue> {
        let (width, height) = (canvas.width(), canvas.height());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(failure)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(failure)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(failure)?;
        let config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or_else(|| failure("the canvas cannot be drawn on"))?;
        surface.configure(&device, &config);

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders.wgsl").into()),
        });
        let compute = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let target = wgpu::ColorTargetState {
            format: config.format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let render = |vertex: &str, fragment: &str, topology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(target.clone())],
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let layout = [
            compute("clear_grid"),
            compute("deposit"),
            compute("springs"),
            compute("integrate"),
        ];
        let fade = compute("fade");
        let edge_pipeline = render("edge_vs", "edge_fs", wgpu::PrimitiveTopology::LineList);
        let node_pipeline = render("node_vs", "node_fs", wgpu::PrimitiveTopology::TriangleList);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let density = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(GRID * GRID) * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Ok(Self {
            device,
            queue,
            surface,
            config,
            params,
            density,
            layout,
            fade,
            edge_pipeline,
            node_pipeline,
            loaded: None,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Take a new snapshot into buffers.
    pub fn load(&mut self, at: &[[f32; 2]], home: &[[f32; 2]], flags: &[u32], edges: &[[u32; 3]]) {
        let nodes: Vec<f32> = at
            .iter()
            .zip(home)
            .flat_map(|(at, home)| [at[0], at[1], 0.0, 0.0, home[0], home[1]])
            .collect();
        let init = |contents: &[u8], usage| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents,
                    usage,
                })
        };
        let storage = wgpu::BufferUsages::STORAGE;
        let copy_to = wgpu::BufferUsages::COPY_DST;
        // A buffer may not be empty, and an empty project still has a page.
        let node_buffer = init(
            bytemuck::cast_slice(&pad(&nodes, 6)),
            storage | wgpu::BufferUsages::COPY_SRC,
        );
        let edge_buffer = init(bytemuck::cast_slice(&pad(edges, 1)), storage);
        let flag_buffer = init(bytemuck::cast_slice(&pad(flags, 1)), storage | copy_to);
        let heat_buffer = init(
            bytemuck::cast_slice(&vec![0f32; at.len().max(1)]),
            storage | copy_to,
        );
        let force = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: at.len().max(1) as u64 * 8,
            usage: storage,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: node_buffer.size(),
            usage: wgpu::BufferUsages::MAP_READ | copy_to,
            mapped_at_creation: false,
        });
        let group = |layout: wgpu::BindGroupLayout, entries: &[(u32, &wgpu::Buffer)]| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &binding(entries),
            })
        };
        let params = &self.params;
        let density = &self.density;
        let groups = [
            group(
                self.layout[0].get_bind_group_layout(0),
                &[(0, params), (4, density)],
            ),
            group(
                self.layout[1].get_bind_group_layout(0),
                &[(0, params), (1, &node_buffer), (4, density)],
            ),
            group(
                self.layout[2].get_bind_group_layout(0),
                &[
                    (0, params),
                    (1, &node_buffer),
                    (2, &edge_buffer),
                    (3, &force),
                ],
            ),
            group(
                self.layout[3].get_bind_group_layout(0),
                &[(0, params), (1, &node_buffer), (3, &force), (4, density)],
            ),
        ];
        let fade_group = group(
            self.fade.get_bind_group_layout(0),
            &[(0, params), (5, &heat_buffer)],
        );
        let edge_group = group(
            self.edge_pipeline.get_bind_group_layout(0),
            &[
                (0, params),
                (1, &node_buffer),
                (2, &edge_buffer),
                (3, &flag_buffer),
                (4, &heat_buffer),
            ],
        );
        let node_group = group(
            self.node_pipeline.get_bind_group_layout(0),
            &[
                (0, params),
                (1, &node_buffer),
                (3, &flag_buffer),
                (4, &heat_buffer),
            ],
        );
        self.loaded = Some(Loaded {
            nodes: at.len() as u32,
            edges: edges.len() as u32,
            node_buffer,
            flag_buffer,
            heat_buffer,
            staging,
            groups,
            fade_group,
            edge_group,
            node_group,
            reading: Reading::Idle,
            ready: Rc::new(Cell::new(false)),
        });
    }

    pub fn set_flags(&self, flags: &[u32]) {
        if let Some(loaded) = &self.loaded {
            self.queue
                .write_buffer(&loaded.flag_buffer, 0, bytemuck::cast_slice(flags));
        }
    }

    /// Make a symbol glow. Negative is an edit.
    pub fn glow(&self, symbol: u32, heat: f32) {
        if let Some(loaded) = &self.loaded {
            let offset = u64::from(symbol) * 4;
            self.queue
                .write_buffer(&loaded.heat_buffer, offset, bytemuck::bytes_of(&heat));
        }
    }

    /// Draw a frame, moving the layout on if `simulate`, and asking for its places if `read`.
    pub fn frame(&mut self, camera: Camera, focused: bool, simulate: bool, read: bool) {
        let Some(loaded) = &mut self.loaded else {
            return;
        };
        let params = Params {
            nodes: loaded.nodes,
            edges: loaded.edges,
            grid: GRID,
            focused: u32::from(focused),
            aspect: self.config.width as f32 / self.config.height as f32,
            zoom: camera.zoom,
            centre_x: camera.x,
            centre_y: camera.y,
        };
        self.queue
            .write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            if simulate {
                let counts = [GRID * GRID, loaded.nodes, loaded.edges, loaded.nodes];
                for (place, count) in counts.iter().enumerate() {
                    pass.set_pipeline(&self.layout[place]);
                    pass.set_bind_group(0, &loaded.groups[place], &[]);
                    pass.dispatch_workgroups(count.div_ceil(GROUP_SIZE), 1, 1);
                }
            }
            pass.set_pipeline(&self.fade);
            pass.set_bind_group(0, &loaded.fade_group, &[]);
            pass.dispatch_workgroups(loaded.nodes.div_ceil(GROUP_SIZE), 1, 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.006,
                            g: 0.009,
                            b: 0.024,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.edge_pipeline);
            pass.set_bind_group(0, &loaded.edge_group, &[]);
            pass.draw(0..loaded.edges * 2, 0..1);
            pass.set_pipeline(&self.node_pipeline);
            pass.set_bind_group(0, &loaded.node_group, &[]);
            pass.draw(0..6, 0..loaded.nodes);
        }
        let copying = read && loaded.reading == Reading::Idle;
        if copying {
            encoder.copy_buffer_to_buffer(
                &loaded.node_buffer,
                0,
                &loaded.staging,
                0,
                loaded.node_buffer.size(),
            );
            loaded.reading = Reading::Copied;
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        if copying {
            let ready = loaded.ready.clone();
            loaded
                .staging
                .map_async(wgpu::MapMode::Read, .., move |done| {
                    ready.set(done.is_ok());
                });
            loaded.reading = Reading::Waiting;
        }
    }

    /// The places read back since the last time, if any came.
    pub fn take_places(&mut self) -> Option<Vec<[f32; 2]>> {
        let loaded = self.loaded.as_mut()?;
        if loaded.reading != Reading::Waiting || !loaded.ready.get() {
            return None;
        }
        let places = loaded.staging.get_mapped_range(..).ok().map(|range| {
            let nodes: &[[f32; 6]] = bytemuck::cast_slice(&range);
            nodes
                .iter()
                .take(loaded.nodes as usize)
                .map(|node| [node[0], node[1]])
                .collect()
        });
        loaded.staging.unmap();
        loaded.ready.set(false);
        loaded.reading = Reading::Idle;
        places
    }
}

/// `values`, or one item of zeros if there are none.
fn pad<T: Copy + bytemuck::Zeroable>(values: &[T], count: usize) -> Vec<T> {
    if values.is_empty() {
        vec![T::zeroed(); count]
    } else {
        values.to_vec()
    }
}
