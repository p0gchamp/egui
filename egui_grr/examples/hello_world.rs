use grr::{BlendChannel, BlendFactor, BlendOp, ClearAttachment, ColorBlend, ColorBlendAttachment, Framebuffer};
use raw_gl_context::{GlConfig, GlContext, Profile};
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use egui_grr::EguiGrr;


fn main() -> anyhow::Result<()> {
    unsafe {
        let event_loop = EventLoop::new();

        let window = WindowBuilder::new()
            .with_title("egui")
            .with_inner_size(LogicalSize::new(1024.0, 768.0))
            .build(&event_loop)?;

        let context = GlContext::create(
            &window,
            GlConfig {
                version: (4, 5),
                profile: Profile::Core,
                red_bits: 8,
                blue_bits: 8,
                green_bits: 8,
                alpha_bits: 0,
                depth_bits: 0,
                stencil_bits: 0,
                samples: None,
                srgb: true,
                double_buffer: true,
                vsync: true,
            },
        )
            .unwrap();

        context.make_current();

        let grr = grr::Device::new(
            |symbol| context.get_proc_address(symbol) as *const _,
            grr::Debug::Enable {
                callback: |report, _, _, _, msg| {
                    println!("{:?}: {:?}", report, msg);
                },
                flags: grr::DebugReport::FULL,
            },
        );

        let mut egui = EguiGrr::new(&window, &grr);

        let mut text = String::with_capacity(128);

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Poll;

            let mut clear_color = [0., 0., 0.];

            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => *control_flow = ControlFlow::Exit,
                Event::WindowEvent {
                    event,
                    ..
                } => {
                    egui.on_event(&event);
                }
                Event::LoopDestroyed => {
                }
                Event::RedrawRequested(_) => {
                    let needs_repaint = egui.run(&window, |egui_ctx| {
                        egui::SidePanel::left("my_side_panel").show(egui_ctx, |ui| {
                            ui.heading("hello world!");
                            if ui.button("quit").clicked() {
                                println!("quit");
                            }
                            ui.code_editor(&mut text);
                        });
                    });

                    if needs_repaint {
                        window.request_redraw();
                        *control_flow = ControlFlow::Poll
                    } else {
                        *control_flow = ControlFlow::Wait
                    };

                    {
                        unsafe {
                            grr.clear_attachment(Framebuffer::DEFAULT, ClearAttachment::ColorFloat(0, [0.3, 0.4, 0.1, 1.0]))
                        }

                        // draw things behind egui here
                        egui.paint(&window, &grr);

                        // draw things on top of egui here
                        context.swap_buffers();
                    }
                }
                Event::MainEventsCleared => {
                    window.request_redraw();
                }
                _ => (),
            }
        })
    }
}