//! Batched draws for many same-shape solid quads (particles, sparks, smoke)
//! through a ggez `InstanceArray`. One draw call regardless of how many
//! instances we push — a big win on web where every `canvas.draw(&mesh, ...)`
//! round-trips through wgpu validation and the wasm/JS bridge.
//!
//! The batch owns its own unit 1×1 quad mesh anchored at the origin so a
//! particle's `dest = (x, y)` + `scale = (w, h)` covers `[x, x+w] × [y, y+h]`
//! in screen space. Callers convert world → screen themselves (using
//! `Camera`) before pushing — `push_world` does the toroidal X-wrap fan-out.

use ggez::glam::Vec2;
use ggez::graphics::{
    Canvas, Color, DrawMode, DrawParam, InstanceArray, Mesh, MeshBuilder, Rect,
};
use ggez::{Context, GameResult};

use crate::render::camera::Camera;

/// One batched layer of solid-color quad instances. Call `begin` at the
/// start of the frame, `push_world` (or `push_screen`) per instance, then
/// `flush` once to emit the single draw call.
pub struct InstanceQuadBatch {
    quad: Mesh,
    instances: InstanceArray,
}

impl InstanceQuadBatch {
    pub fn new(ctx: &mut Context) -> GameResult<Self> {
        // 1×1 quad anchored at (0,0). Same shape ggez uses for `graphics::Quad`
        // but owned by us so we can pair it with our own `InstanceArray`.
        let mut mb = MeshBuilder::new();
        mb.rectangle(DrawMode::fill(), Rect::new(0.0, 0.0, 1.0, 1.0), Color::WHITE)?;
        let quad = Mesh::from_data(ctx, mb.build());
        // `None` defaults to ggez's `white_image` — perfect for solid quads.
        let instances = InstanceArray::new(ctx, None);
        Ok(Self { quad, instances })
    }

    /// Drop everything pushed last frame. Cheap — clears the param/uniform
    /// `Vec`s but doesn't touch any GPU buffers.
    pub fn begin(&mut self) {
        self.instances.clear();
    }

    /// Centered square at `screen_pos`, drawn `size` × `size` screen pixels.
    pub fn push_screen(&mut self, screen_pos: Vec2, size: f32, color: Color) {
        if size <= 0.0 {
            return;
        }
        let half = size * 0.5;
        let dest = Vec2::new(screen_pos.x - half, screen_pos.y - half);
        self.instances.push(
            DrawParam::new()
                .dest(dest)
                .scale([size, size])
                .color(color),
        );
    }

    /// Centered square at `world_pos` (Y-up world coords) with a world-space
    /// `radius`. Issues one push per visible wrap copy so particles straddling
    /// the X seam don't half-vanish.
    pub fn push_world(
        &mut self,
        camera: &Camera,
        world_pos: Vec2,
        radius: f32,
        color: Color,
    ) {
        let size = radius * 2.0 * camera.scale();
        if size <= 0.0 {
            return;
        }
        for cand in camera
            .world_x_offsets_for(world_pos.x, radius)
            .into_iter()
            .flatten()
        {
            let screen = camera.world_to_screen(Vec2::new(cand, world_pos.y));
            self.push_screen(screen, size, color);
        }
    }

    /// Emit the batched draw call, if any instances were pushed.
    pub fn flush(&self, canvas: &mut Canvas) {
        if self.instances.instances().is_empty() {
            return;
        }
        canvas.draw_instanced_mesh(self.quad.clone(), &self.instances, DrawParam::default());
    }
}
