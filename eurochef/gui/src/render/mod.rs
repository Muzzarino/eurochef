use glam::{Mat4, Quat};
use glow::HasContext;

use self::camera::Camera3D;

pub mod billboard;
pub mod blend;
pub mod camera;
pub mod entity;
pub mod gl_helper;
pub mod grid;
pub mod trigger;
pub mod viewer;

#[derive(Default)]
pub struct RenderUniforms {
    pub view: Mat4,
    pub camera_rotation: Quat,
    pub time: f32,
}

impl RenderUniforms {
    pub fn update<C: Camera3D + ?Sized>(
        &mut self,
        orthographic: bool,
        camera: &mut C,
        aspect_ratio: f32,
        time: f32,
    ) {
        let projection = if orthographic {
            glam::Mat4::orthographic_rh_gl(
                (aspect_ratio * -camera.zoom()) * 2.0,
                (-aspect_ratio * -camera.zoom()) * 2.0,
                (1.0 * -camera.zoom()) * 2.0,
                (-1.0 * -camera.zoom()) * 2.0,
                -2500.0,
                2500.0,
            )
        } else {
            glam::Mat4::perspective_rh_gl(90.0_f32.to_radians(), aspect_ratio, 0.02, 2000.0)
        };

        self.view = projection * camera.calculate_matrix();
        self.camera_rotation = camera.rotation();
        self.time = time;
    }
}

pub unsafe fn start_render(gl: &glow::Context) {
    gl.depth_mask(true);
    gl.clear_depth_f32(1.0);
    gl.clear(glow::DEPTH_BUFFER_BIT);
    gl.cull_face(glow::FRONT);
    gl.enable(glow::DEPTH_TEST);
    gl.depth_func(glow::LEQUAL);
}
