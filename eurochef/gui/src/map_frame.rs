use std::sync::{Arc, Mutex};

use eurochef_edb::entity::{EXGeoBaseEntity, EXGeoEntity};
use glam::{Mat4, Quat, Vec3, Vec4};
use glow::HasContext;

use crate::{
    entities::ProcessedEntityMesh,
    entity_frame::RenderableTexture,
    maps::ProcessedMap,
    render::{
        camera::Camera3D,
        entity::EntityRenderer,
        viewer::{BaseViewer, CameraType},
    },
};

pub struct MapFrame {
    pub textures: Vec<RenderableTexture>,
    pub ref_renderers: Vec<Arc<Mutex<EntityRenderer>>>,
    pub placement_renderers: Vec<(u32, EXGeoBaseEntity, Arc<Mutex<EntityRenderer>>)>,

    pub viewer: Arc<Mutex<BaseViewer>>,
    sky_ent: String,

    /// Used to prevent keybinds being triggered while a textfield is focused
    textfield_focused: bool,
}

impl MapFrame {
    pub fn new(
        gl: &glow::Context,
        meshes: &[&ProcessedEntityMesh],
        textures: &[RenderableTexture],
        entities: &Vec<(u32, EXGeoEntity, ProcessedEntityMesh)>,
    ) -> Self {
        assert!(textures.len() != 0);

        let mut s = Self {
            textures: textures.to_vec(),
            ref_renderers: vec![],
            placement_renderers: vec![],
            viewer: Arc::new(Mutex::new(BaseViewer::new(gl))),
            sky_ent: String::new(),
            textfield_focused: false,
        };

        unsafe {
            for m in meshes {
                let r = Arc::new(Mutex::new(EntityRenderer::new(gl)));
                r.lock().unwrap().load_mesh(gl, m);
                s.ref_renderers.push(r);
            }

            for (i, e, m) in entities {
                // Only allow split/normal meshes
                match e {
                    EXGeoEntity::Mesh(_) => {}
                    EXGeoEntity::Split(_) => {}
                    _ => continue,
                };

                let r = Arc::new(Mutex::new(EntityRenderer::new(gl)));
                r.lock().unwrap().load_mesh(gl, m);

                let base = e.base().unwrap().clone();
                s.placement_renderers.push((*i, base, r));
            }
        }

        s.placement_renderers
            .sort_by(|(_, e, _), (_, e2, _)| e.sort_value.cmp(&e2.sort_value));

        s
    }

    pub fn show(&mut self, ui: &mut egui::Ui, map: &ProcessedMap) {
        ui.horizontal(|ui| {
            self.viewer.lock().unwrap().show_toolbar(ui);

            ui.label("  |  ");

            let response = egui::TextEdit::singleline(&mut self.sky_ent)
                .desired_width(76.0)
                .show(ui)
                .response;

            self.textfield_focused = response.has_focus();

            if let Ok(hashcode) = u32::from_str_radix(&self.sky_ent, 16) {
                if !self
                    .placement_renderers
                    .iter()
                    .find(|(hc, _, _)| *hc == hashcode)
                    .is_some()
                {
                    ui.strong(font_awesome::EXCLAMATION_TRIANGLE.to_string())
                        .on_hover_ui(|ui| {
                            ui.label("Entity was not found");
                        });
                }
            } else {
                ui.strong(font_awesome::EXCLAMATION_TRIANGLE.to_string())
                    .on_hover_ui(|ui| {
                        ui.label("String is not formatted as a valid hashcode");
                    });
            }
            ui.label("Sky ent");
        });

        egui::Frame::canvas(ui.style()).show(ui, |ui| self.show_canvas(ui, map));
    }

    fn show_canvas(&mut self, ui: &mut egui::Ui, map: &ProcessedMap) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

        let time = ui.input(|t| t.time);

        let viewer = self.viewer.clone();
        let camera_pos = {
            let mut v = viewer.lock().unwrap();
            if !self.textfield_focused {
                v.update(ui, response);
            }

            let camera: &mut dyn Camera3D = match v.selected_camera {
                CameraType::Fly => &mut v.camera_fly,
                CameraType::Orbit => &mut v.camera_orbit,
            };

            camera.position()
        };
        // TODO(cohae): Why is this necessary?
        let camera_pos = Vec3::new(-camera_pos.x, camera_pos.y, camera_pos.z);

        // TODO(cohae): How do we get out of this situation
        let textures = self.textures.clone(); // FIXME: UUUUGH.
        let map = map.clone(); // FIXME(cohae): ugh.
        let sky_ent = u32::from_str_radix(&self.sky_ent, 16).unwrap_or(u32::MAX);

        let placement_renderers = self.placement_renderers.clone();
        let renderers = self.ref_renderers.clone();
        let cb = egui_glow::CallbackFn::new(move |info, painter| unsafe {
            viewer
                .lock()
                .unwrap()
                .start_render(painter.gl(), info.viewport.aspect_ratio());

            if let Some((_, _, sky_renderer)) =
                placement_renderers.iter().find(|(hc, _, _)| *hc == sky_ent)
            {
                painter.gl().depth_mask(false);

                sky_renderer.lock().unwrap().draw_both(
                    painter.gl(),
                    &viewer.lock().unwrap().uniforms,
                    camera_pos,
                    Vec3::ZERO,
                    Vec3::ONE,
                    time,
                    &textures,
                );

                painter.gl().depth_mask(true);
            }

            // Render base (ref) entities
            for r in &renderers {
                let renderer_lock = r.lock().unwrap();
                renderer_lock.draw_opaque(
                    painter.gl(),
                    &viewer.lock().unwrap().uniforms,
                    Vec3::ZERO,
                    Vec3::ZERO,
                    Vec3::ONE,
                    time,
                    &textures,
                );
            }

            for p in &map.placements {
                if let Some((_, base, r)) = placement_renderers
                    .iter()
                    .find(|(i, _, _)| *i == p.object_ref)
                {
                    let mut rotation: Vec3 = p.rotation.into();
                    let position: Vec3 = p.position.into();
                    if (base.flags & 0x4) != 0 {
                        rotation = look_at(position, camera_pos)
                            .to_euler(glam::EulerRot::XYZ)
                            .into();
                    }

                    let renderer_lock = r.lock().unwrap();
                    renderer_lock.draw_opaque(
                        painter.gl(),
                        &viewer.lock().unwrap().uniforms,
                        position,
                        rotation,
                        p.scale.into(),
                        time,
                        &textures,
                    );
                }
            }

            painter.gl().depth_mask(false);

            for r in &renderers {
                let renderer_lock = r.lock().unwrap();
                renderer_lock.draw_transparent(
                    painter.gl(),
                    &viewer.lock().unwrap().uniforms,
                    Vec3::ZERO,
                    Vec3::ZERO,
                    Vec3::ONE,
                    time,
                    &textures,
                );
            }

            for p in &map.placements {
                if let Some((_, base, r)) = placement_renderers
                    .iter()
                    .find(|(i, _, _)| *i == p.object_ref)
                {
                    let mut rotation: Vec3 = p.rotation.into();
                    let position: Vec3 = p.position.into();
                    if (base.flags & 0x4) != 0 {
                        rotation = look_at(position, camera_pos)
                            .to_euler(glam::EulerRot::XYZ)
                            .into();
                    }

                    let renderer_lock = r.lock().unwrap();
                    renderer_lock.draw_transparent(
                        painter.gl(),
                        &viewer.lock().unwrap().uniforms,
                        position,
                        rotation,
                        p.scale.into(),
                        time,
                        &textures,
                    );
                }
            }
        });
        let callback = egui::PaintCallback {
            rect,
            callback: Arc::new(cb),
        };
        ui.painter().add(callback);
    }
}

fn look_at(center: Vec3, target: Vec3) -> Quat {
    let forward = (target - center).normalize();
    let right = Vec3::Y.cross(forward).normalize();
    let up = forward.cross(right).normalize();

    let rot = Quat::from_mat4(&Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        forward.extend(0.0),
        Vec4::W,
    ));

    rot.mul_quat(Quat::from_rotation_y(180f32.to_radians()))
}
