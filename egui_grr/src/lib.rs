use egui_winit::*;

mod painter;

pub use self::painter::{
    Painter,
    PainterSettings
};

pub struct EguiGrr {
    pub egui_ctx: egui::Context,
    pub egui_winit: egui_winit::State,
    pub painter: crate::Painter,

    shapes: Vec<egui::epaint::ClippedShape>,
    textures_delta: egui::TexturesDelta,
}

impl EguiGrr {
    pub fn new(window: &winit::window::Window, grr: &grr::Device) -> Self {
        let painter = Painter::new(grr, Default::default());

        Self {
            egui_ctx: Default::default(),
            egui_winit: egui_winit::State::new(painter.max_texture_side(), window),
            painter,
            shapes: Default::default(),
            textures_delta: Default::default(),
        }
    }

    /// Returns `true` if egui wants exclusive use of this event
    /// (e.g. a mouse click on an egui window, or entering text into a text field).
    /// For instance, if you use egui for a game, you want to first call this
    /// and only when this returns `false` pass on the events to your game.
    ///
    /// Note that egui uses `tab` to move focus between elements, so this will always return `true` for tabs.
    pub fn on_event(&mut self, event: &winit::event::WindowEvent<'_>) -> bool {
        self.egui_winit.on_event(&self.egui_ctx, event)
    }

    /// Returns `true` if egui requests a repaint.
    ///
    /// Call [`Self::paint`] later to paint.
    pub fn run(
        &mut self,
        window: &winit::window::Window,
        run_ui: impl FnMut(&egui::Context),
    ) -> bool {
        let raw_input = self.egui_winit.take_egui_input(window);
        let (egui_output, shapes) = self.egui_ctx.run(raw_input, run_ui);
        let needs_repaint = egui_output.needs_repaint;
        let textures_delta = self
            .egui_winit
            .handle_output(window, &self.egui_ctx, egui_output);


        self.shapes = shapes;
        self.textures_delta.append(textures_delta);
        needs_repaint
    }

    /// Paint the results of the last call to [`Self::run`].
    pub fn paint(&mut self, window: &winit::window::Window, gl: &grr::Device) {
        let shapes = std::mem::take(&mut self.shapes);
        let mut textures_delta = std::mem::take(&mut self.textures_delta);

        for (id, image_delta) in textures_delta.set {
            self.painter.set_texture(gl, id, &image_delta);
        }

        let clipped_meshes = self.egui_ctx.tessellate(shapes);
        let dimensions: [u32; 2] = window.inner_size().into();
        self.painter.paint_meshes(
            gl,
            self.egui_ctx.pixels_per_point(),
            dimensions,
            clipped_meshes,
        );

        for id in textures_delta.free.drain(..) {
            self.painter.free_texture(id, &gl);
        }
    }

    /// Call to release the allocated graphics resources.
    pub fn destroy(&mut self, gl: &grr::Device) {
        unsafe {
            gl.delete_pipeline(self.painter.pipeline);
            gl.delete_vertex_array(self.painter.vertex_array);
            for (id, img) in &self.textures_delta.set{
                self.painter.free_texture(*id, &gl);
            }

        }
    }
}
