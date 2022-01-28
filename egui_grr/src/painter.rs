use std::ffi::CString;
use std::io::Read;
use std::os::unix::raw::dev_t;
use grr::{BaseFormat, BlendChannel, BlendFactor, BlendOp, BufferRange, ColorBlend, ColorBlendAttachment, Compare, Constant, Extent, Filter, Format, FormatLayout, HostImageCopy, ImageCopy, ImageType, ImageViewType, MemoryFlags, MemoryLayout, Offset, PipelineFlags, Region, SamplerAddress, SamplerDesc, ShaderFlags, SubresourceLayers, SubresourceRange, VertexAttributeDesc, VertexBufferView, Viewport};
use grr::Primitive::Triangles;
use {
    ahash::AHashMap,
    egui::{emath::Rect, epaint::Mesh},
    std::rc::Rc,
};
use egui::epaint;

pub struct Painter {
    max_texture_side: usize,
    pub(crate) pipeline: grr::Pipeline,
    pub(crate) vertex_array: grr::VertexArray,

    pub(crate) textures: AHashMap<egui::TextureId, grr::Image>,
}

impl Painter {
    pub fn new(device: &grr::Device) -> Painter {
        let max_texture_side = 4096;

        let vertex_array = unsafe {
            let vao = device.create_vertex_array(&[VertexAttributeDesc {
                location: 0,
                binding: 0,
                format: grr::VertexFormat::Xy32Float,
                offset: 0,
            }, VertexAttributeDesc {
                location: 1,
                binding: 0,
                format: grr::VertexFormat::Xy32Float,
                offset: (2 * std::mem::size_of::<f32>()) as _,
            }, VertexAttributeDesc {
                location: 2,
                binding: 0,
                format: grr::VertexFormat::Xyzw8Uint,
                offset: (4 * std::mem::size_of::<f32>()) as _,
            }
            ]).expect("couldnt create a vertex array");
            device.object_name(vao, "egui vao");
            vao
        };

        let vertex_shader = unsafe {
            let bytes = include_bytes!("shader/egui_vertex.glsl");
            device.create_shader(grr::ShaderStage::Vertex, bytes, ShaderFlags::VERBOSE)
        }
            .expect("Failed to compile shader");

        let fragment_shader = unsafe {
            let bytes = include_bytes!("shader/egui_fragment.glsl");
            device.create_shader(grr::ShaderStage::Fragment, bytes, ShaderFlags::VERBOSE)
        }.expect("Failed to compile shader");

        let pipeline = unsafe {
            device.create_graphics_pipeline(
                grr::GraphicsPipelineDesc {
                    vertex_shader: Some(vertex_shader),
                    tessellation_control_shader: None,
                    tessellation_evaluation_shader: None,
                    geometry_shader: None,
                    fragment_shader: Some(fragment_shader),
                    mesh_shader: None,
                    task_shader: None,
                },
                grr::PipelineFlags::VERBOSE,
            ).expect("failed to create a egui graphics pipeline")
        };

        unsafe {
            device.delete_shaders(&[vertex_shader, fragment_shader]);
        }

        Painter {
            max_texture_side,
            pipeline,
            vertex_array,
            textures: Default::default(),
        }
    }

    pub fn max_texture_side(&self) -> usize {
        self.max_texture_side
    }

    /// Main entry-point for painting a frame.
    /// You should call `target.clear_color(..)` before
    /// and `target.finish()` after this.
    pub fn paint_meshes(
        &mut self,
        device: &grr::Device,
        pixels_per_point: f32,
        dimensions: [u32; 2],
        cipped_meshes: Vec<egui::ClippedMesh>,
    ) {
        for egui::ClippedMesh(clip_rect, mesh) in cipped_meshes {
            self.paint_mesh(device, pixels_per_point, clip_rect, dimensions, &mesh);
        }
    }

    #[inline(never)] // Easier profiling
    fn paint_mesh(
        &mut self,
        device: &grr::Device,
        pixels_per_point: f32,
        clip_rect: Rect,
        dimensions: [u32; 2],
        mesh: &Mesh,
    ) {
        debug_assert!(mesh.is_valid());

        let vertex_buffer = {
            #[repr(C)]
            #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
            struct Vertex {
                a_pos: [f32; 2],
                a_tc: [f32; 2],
                a_srgba: [u8; 4],
            }

            let vertices: &[Vertex] = bytemuck::cast_slice(&mesh.vertices);

            // TODO: we should probably reuse the `VertexBuffer` instead of allocating a new one each frame.
            unsafe { device.create_buffer_from_host(bytemuck::cast_slice(vertices), MemoryFlags::DYNAMIC) }.expect("could not create the vertex buffer for egui")
        };

        // TODO: we should probably reuse the `IndexBuffer` instead of allocating a new one each frame.
        let index_buffer = unsafe { device.create_buffer_from_host(&bytemuck::cast_slice(&mesh.indices), MemoryFlags::DYNAMIC).unwrap() };
        let (width_in_pixels, height_in_pixels) = (dimensions[0], dimensions[1]);
        let width_in_points = width_in_pixels as f32 / pixels_per_point;
        let height_in_points = height_in_pixels as f32 / pixels_per_point;

        if let Some(texture) = self.get_texture(mesh.texture_id) {
            // The texture coordinates for text are so that both nearest and linear should work with the egui font texture.
            // For user textures linear sampling is more likely to be the right choice.

            let view = unsafe {
                device.create_image_view(*texture, ImageViewType::D2, Format::R8G8B8A8_SRGB, SubresourceRange {
                    levels: 0..1,
                    layers: 0..1,
                })
            }.unwrap();

            let screen_size = [width_in_points, height_in_points];


            // egui outputs colors with premultiplied alpha:

            // Less important, but this is technically the correct alpha blend function
            // when you want to make use of the framebuffer alpha (for screenshots, compositing, etc).

            // egui outputs mesh in both winding orders:
            // let backface_culling = glium::BackfaceCullingMode::CullingDisabled;

            // TODO: disable culling

            // Transform clip rect to physical pixels:
            let clip_min_x = pixels_per_point * clip_rect.min.x;
            let clip_min_y = pixels_per_point * clip_rect.min.y;
            let clip_max_x = pixels_per_point * clip_rect.max.x;
            let clip_max_y = pixels_per_point * clip_rect.max.y;

            // Make sure clip rect can fit within a `u32`:
            let clip_min_x = clip_min_x.clamp(0.0, width_in_pixels as f32);
            let clip_min_y = clip_min_y.clamp(0.0, height_in_pixels as f32);
            let clip_max_x = clip_max_x.clamp(clip_min_x, width_in_pixels as f32);
            let clip_max_y = clip_max_y.clamp(clip_min_y, height_in_pixels as f32);

            let clip_min_x = clip_min_x.round() as u32;
            let clip_min_y = clip_min_y.round() as u32;
            let clip_max_x = clip_max_x.round() as u32;
            let clip_max_y = clip_max_y.round() as u32;


            let sampler = unsafe {
                device.create_sampler(SamplerDesc {
                    min_filter: Filter::Linear,
                    mag_filter: Filter::Linear,
                    mip_map: None,
                    address: (SamplerAddress::ClampEdge, SamplerAddress::ClampEdge, SamplerAddress::ClampEdge),
                    lod_bias: 0.0,
                    lod: 0.0..10.0,
                    compare: None,
                    border_color: [0.0, 0.0, 0.0, 1.0],
                })
            }.expect("err sampler");


            unsafe {
                device.set_viewport(0, &[Viewport {
                    x: 0.0,
                    y: 0.0,
                    w: width_in_pixels as f32,
                    h: height_in_pixels as f32,
                    n: 0.0,
                    f: 1.0,
                }]);
                device.set_scissor(0, &[Region {
                    x: clip_min_x as _,
                    y: (height_in_pixels - clip_max_y) as _,
                    w: (clip_max_x - clip_min_x) as _,
                    h: (clip_max_y - clip_min_y) as _,
                }]);

                device.bind_pipeline(self.pipeline);
                device.bind_color_blend_state(&ColorBlend{
                    attachments: vec![ColorBlendAttachment{
                        blend_enable: true,
                        color: BlendChannel {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            blend_op: BlendOp::Add
                        },
                        alpha: BlendChannel {
                            src_factor: BlendFactor::OneMinusDstAlpha,
                            dst_factor: BlendFactor::One,
                            blend_op: BlendOp::Add
                        }
                    }]
                });


                device.bind_vertex_array(self.vertex_array);
                device.bind_vertex_buffers(self.vertex_array, 0, &[VertexBufferView {
                    buffer: vertex_buffer,
                    offset: 0,
                    stride: ((std::mem::size_of::<f32>() * 4) + (std::mem::size_of::<u8>() * 4)) as _,
                    input_rate: grr::InputRate::Vertex,
                }]);
                device.bind_uniform_constants(self.pipeline, 0, &[Constant::Vec2(screen_size)]);
                device.bind_samplers(0, &[sampler]);
                device.bind_image_views(0, &[view]);

                device.bind_index_buffer(self.vertex_array, index_buffer);
                device.draw_indexed(grr::Primitive::Triangles, grr::IndexTy::U32, 0..(*&mesh.indices.len() as u32), 0..1, 0);
            }
            unsafe {
                device.delete_buffers(&[vertex_buffer, index_buffer]);
                device.delete_sampler(sampler);
                device.delete_image_view(view);
            }
        }
    }

    // ------------------------------------------------------------------------

    pub fn set_texture(
        &mut self,
        device: &grr::Device,
        tex_id: egui::TextureId,
        delta: &egui::epaint::ImageDelta,
    ) {
        let (buf, width, height) = match &delta.image {
            epaint::ImageData::Color(image) => {
                assert_eq!(
                    image.width() * image.height(),
                    image.pixels.len(),
                    "Mismatch between texture size and texel count"
                );
                //todo
                let pixels : Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                (pixels, image.width() as u32, image.height() as u32)
            }
            epaint::image::ImageData::Alpha(image) => {
                let gamma = 1.0;

                let data: Vec<u8> = image
                    .srgba_pixels(gamma)
                    .flat_map(|a| a.to_array())
                    .collect();
                (data, image.width() as u32, image.height() as u32)
            }
        };

        if let Some(pos) = delta.pos {
            // update a sub-region
            if let Some(image) = self.textures.get(&tex_id) {
                unsafe {
                    device.copy_host_to_image(&buf, *image, HostImageCopy{
                        host_layout: MemoryLayout {
                            base_format: BaseFormat::RGBA,
                            format_layout: FormatLayout::U8,
                            row_length: width,
                            image_height: height,
                            alignment: 4
                        },
                        image_subresource: SubresourceLayers { level: 0, layers: 0..1 },
                        image_offset: Offset {
                            x: pos[0] as _,
                            y: pos[1] as _,
                            z: 0
                        },
                        image_extent: Extent {
                            width,
                            height,
                            depth: 1
                        }
                    });
                };
            }
        } else {
            let img = unsafe { self.copy_host_to_tex(&device, &buf, width, height)};
            self.textures.insert(tex_id, img);
        }
    }

    pub fn free_texture(&mut self, tex_id: egui::TextureId, device: &grr::Device) {
        if let Some(img) = self.textures.remove(&tex_id) {
            unsafe { device.delete_image(img) };
        }
    }
    unsafe fn copy_host_to_tex(&mut self, device : &grr::Device, pixels : &[u8], width : u32, height : u32) -> grr::Image {
        let tex = device.create_image(ImageType::D2 {
            width,
            height,
            layers: 1,
            samples: 1,
        }, grr::Format::R8G8B8A8_SRGB, 1).unwrap();

        device.copy_host_to_image(pixels, tex, HostImageCopy {
            host_layout: grr::MemoryLayout {
                base_format: BaseFormat::RGBA,
                format_layout: FormatLayout::U8,
                row_length: width,
                image_height: height,
                alignment: 4,
            },
            image_subresource: grr::SubresourceLayers {
                level: 0,
                layers: 0..1,
            },
            image_offset: grr::Offset {
                x: 0,
                y: 0,
                z: 0,
            },
            image_extent: grr::Extent {
                width,
                height,
                depth: 1,
            },
        });
        tex
    }

    fn get_texture(&self, texture_id: egui::TextureId) -> Option<&grr::Image> {
        self.textures.get(&texture_id)
    }
}